use super::*;
use crate::sexp::{object::Sexp, owner::OwnerToken, session::RSession};

unsafe fn frame(name: &str) -> crate::sexp::context::ContextGuard {
    unsafe { frame_in(name, R_NilValue(), R_NilValue()) }
}
unsafe fn frame_in(
    name: &str,
    environment: SEXP,
    parent: SEXP,
) -> crate::sexp::context::ContextGuard {
    unsafe {
        let owner = OwnerToken::current().unwrap();
        let factory = owner.node_factory();
        let symbol = std::ffi::CString::new(name).unwrap();
        let symbol = factory.wrap(Rf_install(symbol.as_ptr())).unwrap();
        let call = factory.call(&symbol, &factory.nil()).unwrap();
        crate::sexp::context::begin_context_guard(
            ctxt_flags::CTXT_FUNCTION,
            call.as_raw(),
            environment,
            parent,
            None,
            R_NilValue(),
            R_NilValue(),
        )
    }
}

#[cfg(not(miri))]
#[test]
fn owner_profile_includes_the_current_leaf_frame() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let path =
            std::env::temp_dir().join(format!("rport-profile-leaf-{}.out", std::process::id()));
        let filename = owner
            .node_factory()
            .character(path.to_str().unwrap())
            .unwrap();
        R_InitProfiling(
            filename.as_raw(),
            FALSE,
            1.0,
            FALSE,
            FALSE,
            FALSE,
            FALSE,
            100,
            PROFBUFSIZ as i32,
            rpe_type::RPE_ELAPSED,
        );
        let _outer = frame("outer_profile_frame");
        let _inner = frame("leaf_profile_frame");
        R_WriteProfile(0);
        R_EndProfiling();
        let output = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert!(
            output.contains("\"leaf_profile_frame\" \"outer_profile_frame\""),
            "{output}"
        );
    });
}

unsafe fn source_provider(owner: OwnerToken<'_>) -> Sexp<'static> {
    unsafe { source_provider_body(owner, "function() { 1L + 2L; \"owner-profile.R\" }") }
}
unsafe fn source_provider_body(owner: OwnerToken<'_>, body: &str) -> Sexp<'static> {
    unsafe {
        let factory = owner.node_factory();
        let nil = factory.nil();
        let environment = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                nil.as_raw(),
                nil.as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            ))
            .unwrap();
        let function = owner
            .with_arena(|arena| crate::eval::parser::parse(body, arena, factory.clone()))
            .unwrap()
            .unwrap();
        let function = factory
            .wrap(crate::eval::eval::Rf_eval(
                function.as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            ))
            .unwrap();
        crate::sexp::envir::make_active_binding_raw(
            environment.as_raw(),
            Rf_install(c"filename".as_ptr()),
            function.as_raw(),
        );
        let srcref = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(71))
            .unwrap();
        crate::sexp::attrib_core::setAttrib(
            srcref.as_raw(),
            Rf_install(c"srcfile".as_ptr()),
            environment.as_raw(),
        );
        let buffer = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, 256)))
            .unwrap();
        let instance = owner.as_ptr();
        with_profiling_state(|state| {
            state.profile_generation += 1;
            state.profiling = 1;
            state.line_profiling = 1;
            state.srcfile_bufcount = 8;
            state.sref =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, srcref.as_raw());
            state.srcfiles_buffer =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, buffer.as_raw());
        });
        srcref.into_owned().unwrap()
    }
}

#[test]
fn owner_profile_prevents_recursive_sampling_and_retains_cleared_contexts() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let source = source_provider(owner);
        let source_node = source.allocation().unwrap().clone();
        let _outer = frame("outer_profile_frame");
        let _inner = frame("leaf_profile_frame");
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            (*instance).memory_state.gc_force_gap = 0;
            assert!((*instance).eval_state.profiling.sampling_in_progress);
            R_WriteProfile(0);
            (*instance).context_stack.clear();
            crate::sexp::gengc::full_gc_in(instance);
            assert!(source_node.is_live());
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        R_WriteProfile(0);
        (*instance).memory_state.gc_force_gap = 0;
        assert_eq!(calls.get(), 1);
        assert!(!(*instance).eval_state.profiling.sampling_in_progress);
        assert_eq!((*instance).eval_state.profiling.srcfiles.len(), 1);
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_revocation_cleans_the_original_sampling_flag_only() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let original = RSession::new_for_gc_tests();
    let replacement = Rc::new(RefCell::new(None::<RSession>));
    let notifications = Rc::new(Cell::new(0));
    let owner = original.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    original.with_active(|| unsafe {
        let instance = owner.as_ptr();
        let pin = owner.pin().unwrap().unwrap();
        let _source = source_provider(owner);
        let _outer = frame("outer_profile_frame");
        let _inner = frame("leaf_profile_frame");
        let observed = notifications.clone();
        let target = Rc::downgrade(&replacement);
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if observed.replace(1) != 0 {
                return;
            }
            (*instance).memory_state.gc_force_gap = 0;
            crate::sexp::instance::revoke_instance_availability(instance);
            *target.upgrade().unwrap().borrow_mut() = Some(RSession::new_for_gc_tests());
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        crate::eval::runtime::set_visible(FALSE);
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| R_WriteProfile(0)));
        assert!(denied.is_err());
        assert_eq!(notifications.get(), 1);
        assert!(!(*pin.as_ptr()).eval_state.profiling.sampling_in_progress);
        assert_eq!(crate::sexp::globals::R_Visible_in(pin.as_ptr()), FALSE);
    });
    drop(original);
    assert_eq!(weak.allocation_strong_count(), 0);
    replacement.borrow().as_ref().unwrap().with_active(|| {
        assert_eq!(crate::eval::runtime::visible(), TRUE);
        with_profiling_state(|state| {
            assert_eq!(state.profiling, 0);
            assert!(!state.sampling_in_progress);
            assert!(state.profile_schedule.is_none());
        });
    });
}

#[test]
fn owner_profile_stop_during_a_source_callback_discards_old_metadata() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let _source = source_provider(owner);
        let old_generation = with_profiling_state(|state| state.profile_generation);
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if observed.replace(1) != 0 {
                return;
            }
            (*instance).memory_state.gc_force_gap = 0;
            R_EndProfiling();
            // A nested profiling start gets a new generation; the old sample
            // must neither fill its line table nor clear its state on unwind.
            with_profiling_state(|state| {
                state.profile_generation = state.profile_generation.wrapping_add(1);
                state.profiling = 1;
                state.memory_peak_bytes = 4242;
            });
            R_WriteProfile(0);
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        R_WriteProfile(0);
        (*instance).memory_state.gc_force_gap = 0;
        assert_eq!(calls.get(), 1);
        with_profiling_state(|state| {
            assert_eq!(state.profile_generation, old_generation + 2);
            assert_eq!(state.profiling, 1);
            assert_eq!(state.memory_peak_bytes, 4242);
            assert!(state.srcfiles.is_empty());
            assert!(!state.sampling_in_progress);
        });
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_cancellation_precedes_sampling_and_still_allows_stop() {
    let mut session = RSession::new_for_gc_tests();
    let token = crate::sexp::session::CancellationToken::new();
    session.set_cancellation_token(Some(token.clone()));
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let _source = source_provider(owner);
        let factory = owner.node_factory();
        let expression = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(1))
            .unwrap();
        let env = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let mut schedule = ProfileSchedule::new(1000, rpe_type::RPE_ELAPSED).unwrap();
        schedule.make_due();
        with_profiling_state(|state| state.profile_schedule = Some(schedule));
        token.request();
        let cancelled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::eval_expr(expression, env)
        }));
        assert!(cancelled.is_err());
        with_profiling_state(|state| {
            assert!(state.srcfiles.is_empty());
            assert!(!state.sampling_in_progress);
            assert!(
                state.profile_schedule.as_mut().unwrap().due(),
                "cancellation must not consume the pending sample"
            );
        });
        R_EndProfiling();
        assert_eq!(R_Profiling_active(), 0);
        token.reset();
    });
}

#[test]
fn owner_profile_wasm_boundary_refuses_start_and_keeps_stop_a_noop() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let name = factory.strings(&["unavailable-profile.out"]).unwrap();
        let args = factory
            .pairlist_cell(&name, &factory.nil(), &factory.nil())
            .unwrap();
        let start = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            do_Rprof_wasm(R_NilValue(), R_NilValue(), args.as_raw(), R_NilValue())
        }));
        let error = start
            .unwrap_err()
            .downcast::<crate::sexp::context::RError>()
            .unwrap();
        assert!(error.message.contains("not available on this platform"));
        let empty = factory.strings(&[""]).unwrap();
        let args = factory
            .pairlist_cell(&empty, &factory.nil(), &factory.nil())
            .unwrap();
        assert_eq!(
            do_Rprof_wasm(R_NilValue(), R_NilValue(), args.as_raw(), R_NilValue()),
            R_NilValue()
        );
        with_profiling_state(|state| {
            assert_eq!(state.profile_outfile, -1);
            assert_eq!(state.profiling, 0);
            assert!(state.profile_schedule.is_none());
        });
    });
}

unsafe fn profile_arguments(
    owner: OwnerToken<'_>,
    filename: Sexp<'static>,
    interval: Sexp<'static>,
) -> Sexp<'static> {
    unsafe {
        let factory = owner.node_factory();
        let false_value = factory.domain().logical(false).into_owned().unwrap();
        let files = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(100))
            .unwrap()
            .into_owned()
            .unwrap();
        let bytes = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(
                PROFBUFSIZ as i32,
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        let event = factory.strings(&["elapsed"]).unwrap().into_owned().unwrap();
        let values = [
            filename,
            false_value.clone(),
            interval,
            false_value.clone(),
            false_value.clone(),
            false_value.clone(),
            false_value,
            files,
            bytes,
            event,
        ];
        let mut args = factory.nil().into_owned().unwrap();
        for value in values.iter().rev() {
            args = factory
                .pairlist_cell(value, &args, &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap();
        }
        args
    }
}

#[test]
fn owner_profile_argument_provider_revocation_cannot_start_a_replacement_profiler() {
    use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
    use crate::sexp::object::SexpResult;
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };
    struct IntervalProvider {
        original: Weak<RefCell<Option<RSession>>>,
        replacement: Weak<RefCell<Option<RSession>>>,
        calls: Rc<Cell<usize>>,
    }
    impl AltrepClass for IntervalProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            _: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            context.gc()?;
            drop(self.original.upgrade().unwrap().borrow_mut().take());
            *self.replacement.upgrade().unwrap().borrow_mut() = Some(RSession::new_for_gc_tests());
            Ok(AltrepElement::Real(0.001))
        }
    }
    let original = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let replacement = Rc::new(RefCell::new(None::<RSession>));
    let calls = Rc::new(Cell::new(0));
    let (args, weak) = {
        let borrowed = original.borrow();
        let session = borrowed.as_ref().unwrap();
        let owner = session.owner_token().unwrap();
        let class = session
            .register_altrep_class(
                "profiling_interval_owner",
                IntervalProvider {
                    original: Rc::downgrade(&original),
                    replacement: Rc::downgrade(&replacement),
                    calls: calls.clone(),
                },
            )
            .unwrap();
        let interval = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let filename = owner
            .node_factory()
            .strings(&["/tmp/rport-must-not-open-after-revocation.out"])
            .unwrap()
            .into_owned()
            .unwrap();
        (
            unsafe { profile_arguments(owner, filename, interval) },
            owner.weak_owner().unwrap(),
        )
    };
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        do_Rprof(R_NilValue(), R_NilValue(), args.as_raw(), R_NilValue())
    }));
    assert!(denied.is_err());
    assert_eq!(calls.get(), 1);
    assert_eq!(weak.allocation_strong_count(), 0);
    replacement.borrow().as_ref().unwrap().with_active(|| {
        with_profiling_state(|state| {
            assert_eq!(state.profile_outfile, -1);
            assert_eq!(state.profiling, 0);
            assert!(state.profile_schedule.is_none());
        });
    });
}

#[cfg(not(miri))]
#[test]
fn owner_profile_filename_snapshot_survives_provider_mutation_and_collection() {
    use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
    use crate::sexp::object::{SexpMut, SexpResult};
    struct IntervalProvider {
        filename: String,
        original: crate::sexp::heap::CheckedNode,
    }
    impl AltrepClass for IntervalProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            _: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            let replacement = context.string(&self.filename)?;
            let mut source = SexpMut::try_from_checked(context.data1()?)?;
            source.try_set_string_elt(0, replacement)?;
            drop(source);
            context.gc()?;
            assert!(
                self.original.is_live(),
                "the already-read filename must remain rooted after its source changes"
            );
            Ok(AltrepElement::Real(0.001))
        }
    }
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let first =
            std::env::temp_dir().join(format!("rport-profile-original-{}.out", std::process::id()));
        let changed =
            std::env::temp_dir().join(format!("rport-profile-mutated-{}.out", std::process::id()));
        let factory = owner.node_factory();
        let filename = factory
            .strings(&[first.to_str().unwrap()])
            .unwrap()
            .into_owned()
            .unwrap();
        let original = filename
            .try_string_elt(0)
            .unwrap()
            .allocation()
            .unwrap()
            .clone();
        let class = session
            .register_altrep_class(
                "profiling_interval_mutation",
                IntervalProvider {
                    filename: changed.to_string_lossy().into_owned(),
                    original,
                },
            )
            .unwrap();
        let interval = AltrepBuilder::new(class)
            .data1(filename.clone())
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let args = profile_arguments(owner, filename, interval);
        do_Rprof(R_NilValue(), R_NilValue(), args.as_raw(), R_NilValue());
        R_EndProfiling();
        assert!(first.exists());
        assert!(!changed.exists());
        let _ = std::fs::remove_file(first);
    });
}

#[test]
fn profile_workspace_denial_skips_the_sample_before_source_providers() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let _source = source_provider(owner);
        let _outer = frame("outer_profile_frame");
        let _inner = frame("leaf_profile_frame");
        owner.full_gc().unwrap();
        let observed = Rc::new(Cell::new(0));
        let counter = observed.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            counter.set(counter.get() + 1);
            (*instance).memory_state.gc_force_gap = 0;
        }));
        let (budget, bytes, workspace) = owner.with_arena(|arena| {
            let before = (arena.budget(), arena.total_bytes_allocated(), arena.workspace_bytes());
            arena.set_budget(crate::sexp::memory::ArenaBudget::new(before.1 + before.2, 0));
            before
        }).unwrap();
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let sampled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| R_WriteProfile(0)));
        (*instance).memory_state.gc_force_gap = 0;
        owner.with_arena(|arena| arena.set_budget(budget)).unwrap();
        assert!(sampled.is_ok(), "sampling admission must not replace the workload's result with a profiling allocation error");
        assert_eq!(observed.get(), 0, "no profiling source provider may execute when snapshot workspace cannot be admitted");
        assert_eq!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap(), workspace);
        assert!(owner.with_arena(|arena| arena.total_bytes_allocated()).unwrap() <= bytes);
        assert!(with_profiling_state(|state| state.srcfiles.is_empty()));
        assert!(!with_profiling_state(|state| state.sampling_in_progress));
        R_EndProfiling();
    });
}

#[test]
fn profile_workspace_source_table_replacement_admits_both_live_backings() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let first = factory.character("first").unwrap();
        let second = factory.character("second").unwrap();
        let buffer = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 32)))
            .unwrap();
        let instance = owner.as_ptr();
        with_profiling_state(|state| {
            state.srcfiles_buffer =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, buffer.as_raw());
            state.srcfile_bufcount = 1;
            state.line_profiling = 1;
        });
        let held = owner
            .with_arena(|arena| arena.reserve_workspace(128).unwrap())
            .unwrap();
        assert_eq!(getFilenum(&first), 1);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            128 + std::mem::size_of::<usize>()
        );
        with_profiling_state(|state| state.srcfile_bufcount = 2);
        let budget = owner
            .with_arena(|arena| {
                let old = arena.budget();
                arena.set_budget(crate::sexp::memory::ArenaBudget::new(
                    arena.total_bytes_allocated() + 128 + 2 * std::mem::size_of::<usize>(),
                    0,
                ));
                old
            })
            .unwrap();
        assert_eq!(
            getFilenum(&second),
            0,
            "the new table alone fits; old+new backing must still be admitted"
        );
        with_profiling_state(|state| {
            assert_eq!(state.srcfiles, [0]);
            assert_eq!(state.srcfile_bytes_used, 6);
        });
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            128 + std::mem::size_of::<usize>()
        );
        owner.with_arena(|arena| arena.set_budget(budget)).unwrap();
        assert_eq!(getFilenum(&second), 2);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            128 + 2 * std::mem::size_of::<usize>()
        );
        R_EndProfiling();
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            128
        );
        assert_eq!(with_profiling_state(|state| state.srcfiles.capacity()), 0);
        drop(held);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    });
}

#[cfg(not(miri))]
#[test]
fn owner_profile_filter_keeps_parent_function_frames() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let nil = factory.nil();
        let outer_env = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                nil.as_raw(),
                nil.as_raw(),
                global.as_raw(),
            ))
            .unwrap();
        let inner_env = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                nil.as_raw(),
                nil.as_raw(),
                outer_env.as_raw(),
            ))
            .unwrap();
        let _top = crate::sexp::context::begin_context_guard(
            ctxt_flags::CTXT_TOPLEVEL,
            nil.as_raw(),
            global.as_raw(),
            global.as_raw(),
            None,
            nil.as_raw(),
            nil.as_raw(),
        );
        let _outer = frame_in("outer_profile_frame", outer_env.as_raw(), global.as_raw());
        let _inner = frame_in("leaf_profile_frame", inner_env.as_raw(), outer_env.as_raw());
        let path =
            std::env::temp_dir().join(format!("rport-profile-filter-{}.out", std::process::id()));
        let filename = factory.character(path.to_str().unwrap()).unwrap();
        R_InitProfiling(
            filename.as_raw(),
            FALSE,
            1.0,
            FALSE,
            FALSE,
            FALSE,
            TRUE,
            100,
            PROFBUFSIZ as i32,
            rpe_type::RPE_ELAPSED,
        );
        R_WriteProfile(0);
        R_EndProfiling();
        let output = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert!(
            output.contains("\"leaf_profile_frame\" \"outer_profile_frame\""),
            "{output}"
        );
    });
}

#[test]
fn owner_profile_filter_advances_synthetic_parent_contexts_after_collection() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let nil = factory.nil();
        let outer_env = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                nil.as_raw(),
                nil.as_raw(),
                global.as_raw(),
            ))
            .unwrap();
        let inner_env = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                nil.as_raw(),
                nil.as_raw(),
                outer_env.as_raw(),
            ))
            .unwrap();
        let top = crate::sexp::context::begin_context_guard(
            ctxt_flags::CTXT_TOPLEVEL,
            nil.as_raw(),
            global.as_raw(),
            global.as_raw(),
            None,
            nil.as_raw(),
            nil.as_raw(),
        );
        let outer = frame_in("outer_profile_frame", outer_env.as_raw(), global.as_raw());
        let inner = frame_in("leaf_profile_frame", inner_env.as_raw(), outer_env.as_raw());
        let synthetic = frame_in(
            "synthetic_profile_frame",
            inner_env.as_raw(),
            inner_env.as_raw(),
        );
        with_profiling_state(|state| {
            state.filter_callframes = 1;
            state.profiling = 1;
        });
        owner.full_gc().unwrap();
        assert_eq!(get_R_ToplevelContext(), top.context());
        assert_eq!(
            findProfContext(synthetic.context(), ptr::null_mut()),
            inner.context()
        );
        assert_eq!(
            findProfContext(inner.context(), ptr::null_mut()),
            outer.context()
        );
        assert!(findProfContext(outer.context(), ptr::null_mut()).is_null());
        let before = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        R_WriteProfile(0);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            before
        );
        assert!(!with_profiling_state(|state| state.sampling_in_progress));
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_preserves_visibility_when_source_providers_reenter() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let _source = source_provider(owner);
        crate::eval::runtime::set_visible(FALSE);
        R_WriteProfile(0);
        assert_eq!(
            crate::eval::runtime::visible(),
            FALSE,
            "metadata evaluation must not change the workload's visibility"
        );
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_preserves_visibility_when_a_source_provider_errors() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let _source = source_provider_body(owner, "function() stop(\"profiling source sentinel\")");
        crate::eval::runtime::set_visible(FALSE);
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| R_WriteProfile(0)));
        let payload = failed.unwrap_err();
        let message = if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
            error.message.clone()
        } else if let Some(crate::sexp::context::RSignal::Error { message }) =
            payload.downcast_ref::<crate::sexp::context::RSignal>()
        {
            message.clone()
        } else {
            panic!("unexpected source error payload")
        };
        assert!(message.contains("profiling source sentinel"), "{message}");
        assert_eq!(crate::eval::runtime::visible(), FALSE);
        assert!(!with_profiling_state(|state| state.sampling_in_progress));
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_filter_keeps_an_oldest_function_without_a_toplevel_marker() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let nil = factory.nil();
        let global = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let environment = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                nil.as_raw(),
                nil.as_raw(),
                global.as_raw(),
            ))
            .unwrap();
        let function = frame_in(
            "oldest_profile_function",
            environment.as_raw(),
            global.as_raw(),
        );
        let expression = crate::sexp::context::begin_context_guard(
            ctxt_flags::CTXT_CCODE,
            nil.as_raw(),
            environment.as_raw(),
            global.as_raw(),
            None,
            nil.as_raw(),
            nil.as_raw(),
        );
        with_profiling_state(|state| {
            state.profiling = 1;
            state.filter_callframes = 1;
        });
        owner.full_gc().unwrap();
        assert!(get_R_ToplevelContext().is_null());
        assert_eq!(
            findProfContext(expression.context(), ptr::null_mut()),
            function.context()
        );
        R_WriteProfile(0);
        assert!(!with_profiling_state(|state| state.sampling_in_progress));
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_typed_eval_returns_source_errors_in_its_result() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let _source = source_provider_body(
            owner,
            "function() stop(\"typed profiling source sentinel\")",
        );
        let factory = owner.node_factory();
        let expression = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(42))
            .unwrap();
        let environment = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let mut schedule = ProfileSchedule::new(1000, rpe_type::RPE_ELAPSED).unwrap();
        schedule.make_due();
        with_profiling_state(|state| state.profile_schedule = Some(schedule));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::eval_safe(expression, environment)
        }));
        let error = result
            .expect("typed evaluation must catch profiler source errors")
            .unwrap_err();
        assert!(error.contains("typed profiling source sentinel"), "{error}");
        assert!(!with_profiling_state(|state| state.sampling_in_progress));
        R_EndProfiling();
    });
}

#[test]
fn owner_profile_typed_eval_checks_cancellation_before_metadata_allocation() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    let token = crate::sexp::session::CancellationToken::new();
    session.set_cancellation_token(Some(token.clone()));
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let _source = source_provider(owner);
        let factory = owner.node_factory();
        let expression = factory
            .wrap(crate::sexp::constructors::Rf_ScalarInteger(42))
            .unwrap();
        let environment = factory.wrap(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let mut schedule = ProfileSchedule::new(1000, rpe_type::RPE_ELAPSED).unwrap();
        schedule.make_due();
        with_profiling_state(|state| state.profile_schedule = Some(schedule));
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            (*instance).memory_state.gc_force_gap = 0;
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        token.request();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::eval_safe(expression, environment)
        }));
        (*instance).memory_state.gc_force_gap = 0;
        let error = result
            .expect("typed evaluation must catch cooperative cancellation")
            .unwrap_err();
        assert!(error.contains("operation cancelled"), "{error}");
        assert_eq!(
            calls.get(),
            0,
            "no metadata allocation may precede cancellation"
        );
        assert!(with_profiling_state(|state| state
            .profile_schedule
            .as_mut()
            .unwrap()
            .due()));
        R_EndProfiling();
        token.reset();
    });
}
