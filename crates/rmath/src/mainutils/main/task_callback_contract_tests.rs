#[test]
fn task_callback_registry_admission_precedes_growth_at_zero_headroom() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let fun = source(&session, "function(...) TRUE");
    let held = owner
        .with_arena(|arena| arena.reserve_workspace(128).unwrap())
        .unwrap();
    let (budget, bytes, workspace) = owner
        .with_arena(|arena| {
            let before = (
                arena.budget(),
                arena.total_bytes_allocated(),
                arena.workspace_bytes(),
            );
            arena.set_budget(crate::sexp::memory::ArenaBudget::new(
                before.1 + before.2,
                0,
            ));
            before
        })
        .unwrap();
    let result = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            Rf_addTaskCallback(fun.as_raw(), R_NilValue())
        }))
    });
    owner.with_arena(|arena| arena.set_budget(budget)).unwrap();
    assert!(
        result.is_err(),
        "registry backing must be admitted before allocating native slots"
    );
    session.with_active(|| unsafe {
        assert!((*owner.as_ptr()).main_state.task_callbacks.is_empty());
        assert_eq!((*owner.as_ptr()).main_state.task_callbacks.capacity(), 0);
    });
    assert_eq!(
        owner
            .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
            .unwrap(),
        (bytes, workspace)
    );
    drop(held);
}

#[test]
fn task_callback_public_names_and_current_positions_match_gnu() {
    let session = RSession::new_without_default_packages();
    let result = source(
        &session,
        r#"local({
      f<-function(...) TRUE
      a<-addTaskCallback(f,name='left'); b<-addTaskCallback(f,name='right')
      removeTaskCallback(1L); c<-addTaskCallback(f,name='after')
      answer<-list(indices=c(unname(a),unname(b),unname(c)), labels=c(names(a),names(b),names(c)), current=getTaskCallbackNames())
      removeTaskCallback(1L); removeTaskCallback(1L)
      answer
    })"#,
    );
    let indices = result.try_vector_elt(0).unwrap();
    assert_eq!(
        (
            indices.integer_elt(0),
            indices.integer_elt(1),
            indices.integer_elt(2)
        ),
        (Some(1), Some(2), Some(2))
    );
    let names = result.try_vector_elt(1).unwrap();
    assert_eq!(
        (
            names.string_value_elt(0),
            names.string_value_elt(1),
            names.string_value_elt(2)
        ),
        (
            Some(Some("left".into())),
            Some(Some("right".into())),
            Some(Some("after".into()))
        )
    );
    let current = result.try_vector_elt(2).unwrap();
    assert_eq!(
        (current.string_value_elt(0), current.string_value_elt(1)),
        (Some(Some("right".into())), Some(Some("after".into())))
    );
}

#[test]
fn task_callback_public_explicit_null_is_present_and_omitted_data_is_missing() {
    let session = RSession::new_without_default_packages();
    source(
        &session,
        r#"{callback_data_seen<-character();callback_f<-function(expr,value,ok,visible,data){gc();callback_data_seen<<-c(callback_data_seen,if(missing(data))'missing' else if(is.null(data))'NULL' else as.character(data));TRUE};addTaskCallback(callback_f,name='omit');addTaskCallback(callback_f,data=NULL,name='null');addTaskCallback(callback_f,data=17L,name='value')}"#,
    );
    session
        .with_active(|| unsafe { Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE) });
    let seen = source(&session, "paste(callback_data_seen,collapse=',')");
    assert_eq!(
        seen.string_value_elt(0),
        Some(Some("missing,NULL,17".into()))
    );
    source(
        &session,
        "{removeTaskCallback('omit');removeTaskCallback('null');removeTaskCallback('value')}",
    );
}

#[test]
fn task_callback_public_positions_survive_iteration_mutation() {
    let session = RSession::new_without_default_packages();
    source(
        &session,
        r#"{callback_order<-character();addTaskCallback(function(...){callback_order<<-c(callback_order,'A');removeTaskCallback(2L);addTaskCallback(function(...){callback_order<<-c(callback_order,'D');FALSE},name='D');TRUE},name='A');addTaskCallback(function(...){callback_order<<-c(callback_order,'B');TRUE},name='B');addTaskCallback(function(...){gc();callback_order<<-c(callback_order,'C');removeTaskCallback('A');FALSE},name='C')}"#,
    );
    session
        .with_active(|| unsafe { Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE) });
    let order = source(&session, "paste(callback_order,collapse=',')");
    assert_eq!(order.string_value_elt(0), Some(Some("A,C,D".into())));
    assert_eq!(
        source(&session, "length(getTaskCallbackNames())").integer_elt(0),
        Some(0)
    );
}

#[test]
fn task_callback_typed_null_name_and_current_position_use_original_owner() {
    let left = RSession::new_for_gc_tests();
    let owner = left.owner_token().unwrap();
    let factory = owner.node_factory();
    let fun = source(
        &left,
        "function(expr,value,succeeded,visible,data){callback_received<<-missing(data);FALSE}",
    );
    let name = factory.strings(&["explicit_null"]).unwrap();
    let used = factory.domain().logical(true);
    let result = left.with_active(|| {
        factory
            .wrap(unsafe {
                R_add_task_callback(
                    fun.as_raw(),
                    factory.nil().as_raw(),
                    used.as_raw(),
                    name.as_raw(),
                )
            })
            .unwrap()
            .into_owned()
            .unwrap()
    });
    assert_eq!(
        result.integer_elt(0),
        Some(0),
        "native registration returns zero-based current position"
    );
    let other = RSession::new_for_gc_tests();
    other
        .with_active(|| unsafe { Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE) });
    left.with_active(|| unsafe { Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE) });
    assert_eq!(
        source(&left, "callback_received").logical_elt(0),
        Some(FALSE)
    );
    left.with_active(|| {
        let value = factory.wrap(unsafe { R_task_callback_names() }).unwrap();
        assert_eq!(value.len(), 0);
    });
}

#[test]
fn task_callback_registry_denied_growth_preserves_old_storage_and_held_ticket() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let fun = source(&session, "function(...) TRUE");
    for _ in 0..4 {
        session.with_active(|| unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) });
    }
    let held = owner
        .with_arena(|a| a.reserve_workspace(128).unwrap())
        .unwrap();
    let before = session.with_active(|| unsafe {
        let s = &(*owner.as_ptr()).main_state;
        (
            s.task_callbacks.len(),
            s.task_callbacks.capacity(),
            s.next_task_callback_id,
        )
    });
    let (budget, bytes, workspace) = owner
        .with_arena(|a| {
            let old = (a.budget(), a.total_bytes_allocated(), a.workspace_bytes());
            let replacement = before.1 * 2 * std::mem::size_of::<ToplevelTaskCallback>();
            a.set_budget(crate::sexp::memory::ArenaBudget::new(
                old.1 + old.2 + replacement - 1,
                0,
            ));
            old
        })
        .unwrap();
    let result = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            Rf_addTaskCallback(fun.as_raw(), R_NilValue())
        }))
    });
    owner.with_arena(|a| a.set_budget(budget)).unwrap();
    assert!(result.is_err());
    let after = session.with_active(|| unsafe {
        let s = &(*owner.as_ptr()).main_state;
        (
            s.task_callbacks.len(),
            s.task_callbacks.capacity(),
            s.next_task_callback_id,
        )
    });
    assert_eq!(before, after);
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (bytes, workspace)
    );
    drop(held);
    assert_eq!(
        owner.with_arena(|a| a.workspace_bytes()).unwrap(),
        workspace - 128
    );
}

#[test]
fn task_callback_duplicate_names_metadata_and_native_selectors_match_gnu() {
    let session = RSession::new_without_default_packages();
    let result = source(
        &session,
        r#"local({
      f<-function(...)TRUE
      a<-addTaskCallback(f,name='same');b<-addTaskCallback(f,name='same')
      native_remove<-.Call(.C_R_removeTaskCallback,TRUE)
      remaining<-getTaskCallbackNames()
      removeTaskCallback('same')
      malformed<-inherits(try(addTaskCallback(f,name=c('a','b')),silent=TRUE),'try-error')
      after_error<-getTaskCallbackNames();removeTaskCallback('a')
      counts<-c(.C_R_addTaskCallback$numParameters,.C_R_removeTaskCallback$numParameters,.C_R_getTaskCallbackNames$numParameters)
      bad_arity<-inherits(try(.Call(.C_R_getTaskCallbackNames,1L),silent=TRUE),'try-error')
      hidden<-inherits(try(getNativeSymbolInfo('R_addTaskCallback'),silent=TRUE),'try-error')
      list(native_remove,remaining,malformed,after_error,counts,bad_arity,hidden,length(getTaskCallbackNames()))
    })"#,
    );
    assert_eq!(result.vector_elt(0).unwrap().logical_elt(0), Some(TRUE));
    assert_eq!(
        result.vector_elt(1).unwrap().string_value_elt(0),
        Some(Some("same".into()))
    );
    assert_eq!(result.vector_elt(2).unwrap().logical_elt(0), Some(TRUE));
    assert_eq!(
        result.vector_elt(3).unwrap().string_value_elt(0),
        Some(Some("a".into()))
    );
    let counts = result.vector_elt(4).unwrap();
    assert_eq!(
        (
            counts.integer_elt(0),
            counts.integer_elt(1),
            counts.integer_elt(2)
        ),
        (Some(4), Some(1), Some(0))
    );
    assert_eq!(result.vector_elt(5).unwrap().logical_elt(0), Some(TRUE));
    assert_eq!(result.vector_elt(6).unwrap().logical_elt(0), Some(TRUE));
    assert_eq!(result.vector_elt(7).unwrap().integer_elt(0), Some(0));
}

#[test]
fn task_callback_name_snapshot_survives_collecting_removal_and_releases_roots() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let fun = source(&session, "function(...) TRUE");
    let name = factory.strings(&["original_name_backing"]).unwrap();
    let name_node = name
        .try_string_elt(0)
        .unwrap()
        .allocation()
        .unwrap()
        .clone();
    let used = factory.domain().logical(false);
    let index = session.with_active(|| {
        factory
            .wrap(unsafe {
                R_add_task_callback(
                    fun.as_raw(),
                    factory.nil().as_raw(),
                    used.as_raw(),
                    name.as_raw(),
                )
            })
            .unwrap()
            .into_owned()
            .unwrap()
    });
    drop(index);
    drop(name);
    owner.full_gc().unwrap();
    assert!(name_node.is_live());
    let selector = integer(&session, 1);
    let observed = Rc::new(Cell::new(false));
    let check = observed.clone();
    session.with_active_in(|instance| unsafe {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if !check.replace(true) {
                R_remove_task_callback(selector.as_raw());
            }
            (*instance).memory_state.gc_force_gap = 0;
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
    });
    let names = session.with_active(|| {
        factory
            .wrap(unsafe { R_task_callback_names() })
            .unwrap()
            .into_owned()
            .unwrap()
    });
    assert!(observed.get());
    assert_eq!(
        names.string_value_elt(0),
        Some(Some("original_name_backing".into()))
    );
    owner.full_gc().unwrap();
    assert!(name_node.is_live());
    drop(names);
    owner.full_gc().unwrap();
    assert!(
        !name_node.is_live(),
        "retired registry and snapshots must release the actual name owner"
    );
}

#[test]
fn task_callback_name_snapshot_revocation_drops_only_original_workspace() {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let original = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = original.pin().unwrap();
    let instance = pin.as_ptr();
    let fun = source(facade.borrow().as_ref().unwrap(), "function(...) TRUE");
    let (ledger, held, before) = facade
        .borrow()
        .as_ref()
        .unwrap()
        .with_active_in(|_| unsafe {
            let owner = OwnerToken::from_raw(instance);
            Rf_addTaskCallback(fun.as_raw(), R_NilValue());
            let ledger = owner
                .with_arena(|a| a.heap_identity().byte_ledger())
                .unwrap();
            let held = owner
                .with_arena(|a| a.reserve_workspace(128).unwrap())
                .unwrap();
            let before = ledger.workspace_bytes_for_test();
            (ledger, held, before)
        });
    let replacement = RSession::new_for_gc_tests();
    let replacement_owner = replacement.owner_token().unwrap();
    let replacement_held = replacement_owner
        .with_arena(|a| a.reserve_workspace(256).unwrap())
        .unwrap();
    let callback_facade = Rc::downgrade(&facade);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                (*instance).memory_state.gc_force_gap = 0;
                drop(callback_facade.upgrade().unwrap().borrow_mut().take());
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            R_task_callback_names();
        });
    }));
    assert!(
        result.is_err(),
        "revoked snapshots must not publish an R value"
    );
    assert!(facade.borrow().is_none());
    assert!(!original.is_live());
    assert_eq!(
        ledger.workspace_bytes_for_test(),
        before,
        "failed snapshot must release its original ticket"
    );
    assert_eq!(
        replacement_owner
            .with_arena(|a| a.workspace_bytes())
            .unwrap(),
        256
    );
    drop(pin);
    assert_eq!(
        ledger.workspace_bytes_for_test(),
        128,
        "registry storage drops before its original admission"
    );
    drop(held);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    drop(replacement_held);
}

#[test]
fn task_callback_lazy_metadata_survives_gc_reentry_lock_and_forces_once() {
    for binding in [c".C_R_getTaskCallbackNames", c"getTaskCallbackNames"] {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let owner = session.owner_token().unwrap();
            let base = crate::sexp::globals::R_BaseEnv();
            install_task_callback_symbols(base);
            let symbol = Rf_install(binding.as_ptr());
            let promise = owner
                .sexp(crate::sexp::envir::R_findVarInFrame(base, symbol))
                .unwrap()
                .into_owned()
                .unwrap();
            assert_eq!(promise.typeof_(), SEXPTYPE::PROMSXP);
            crate::sexp::envir::lock_binding_raw(base, symbol);
            owner.full_gc().unwrap();
            let attempts = Rc::new(Cell::new(0));
            let observed = attempts.clone();
            let recursive = Rc::new(RefCell::new(None::<String>));
            let recursive_seen = recursive.clone();
            let expression = owner.sexp(symbol).unwrap().into_owned().unwrap();
            let environment = owner.sexp(base).unwrap().into_owned().unwrap();
            let promise_node = promise.allocation().unwrap().clone();
            let weak = owner.weak_owner().unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                // Entry bookkeeping may collect before forcing begins. Exercise
                // recursive lookup only once the ordinary promise is evaluating.
                if promise_node
                    .heap_identity()
                    .node_snapshot(&promise_node)
                    .unwrap()
                    .sxpinfo
                    .gp()
                    != 1
                {
                    return;
                }
                let pin = weak.pin().unwrap();
                (*pin.as_ptr()).memory_state.gc_force_gap = 0;
                if observed.replace(1) == 0 {
                    *recursive_seen.borrow_mut() =
                        crate::eval::eval::eval_safe(expression.clone(), environment.clone()).err();
                }
            }));
            (*owner.as_ptr()).memory_state.gc_force_gap = 1;
            (*owner.as_ptr()).memory_state.gc_force_wait = 1;
            let first = owner
                .sexp(crate::eval::eval::Rf_eval(symbol, base))
                .unwrap()
                .into_owned()
                .unwrap();
            assert_eq!(attempts.get(), 1);
            assert!(
                recursive
                    .borrow()
                    .as_ref()
                    .is_some_and(|error| error.contains("promise already under evaluation")),
                "{:?}",
                recursive.borrow()
            );
            if binding.to_bytes().starts_with(b".") {
                assert_eq!(first.typeof_(), SEXPTYPE::VECSXP);
                assert_eq!(first.try_vector_elt(3).unwrap().integer_elt(0), Some(0));
            } else {
                assert_eq!(first.typeof_(), SEXPTYPE::CLOSXP);
            }
            assert_eq!(
                crate::sexp::accessors::PRVALUE(promise.as_raw()),
                first.as_raw()
            );
            owner.full_gc().unwrap();
            let second = crate::eval::eval::Rf_eval(symbol, base);
            assert_eq!(first.as_raw(), second);
            assert!(crate::sexp::envir::binding_is_locked_raw(base, symbol));
            let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::sexp::envir::defineVar(symbol, R_NilValue(), base)
            }));
            assert!(denied.is_err());
            for name in [c".C_R_addTaskCallback", c".C_R_removeTaskCallback"] {
                let value = owner
                    .sexp(crate::sexp::envir::R_findVarInFrame(
                        base,
                        Rf_install(name.as_ptr()),
                    ))
                    .unwrap();
                assert_eq!(value.typeof_(), SEXPTYPE::PROMSXP);
                assert_eq!(
                    crate::sexp::accessors::PRVALUE(value.as_raw()),
                    crate::sexp::globals::R_UnboundValue()
                );
            }
        });
    }
}

#[test]
fn task_callback_lazy_metadata_revocation_never_publishes_into_replacement() {
    for binding in [c".C_R_addTaskCallback", c"addTaskCallback"] {
        let original = RSession::new_for_gc_tests();
        let replacement = Rc::new(RefCell::new(None::<RSession>));
        let target = Rc::downgrade(&replacement);
        let owner = original.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        original.with_active(|| unsafe {
            let base = crate::sexp::globals::R_BaseEnv();
            install_task_callback_symbols(base);
            let symbol = Rf_install(binding.as_ptr());
            let promise = owner
                .sexp(crate::sexp::envir::R_findVarInFrame(base, symbol))
                .unwrap()
                .into_owned()
                .unwrap();
            let unbound = crate::sexp::globals::R_UnboundValue();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(1) != 0 {
                    return;
                }
                let pin = weak.pin().unwrap();
                (*pin.as_ptr()).memory_state.gc_force_gap = 0;
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
                *target.upgrade().unwrap().borrow_mut() = Some(RSession::new_for_gc_tests());
            }));
            (*owner.as_ptr()).memory_state.gc_force_gap = 1;
            (*owner.as_ptr()).memory_state.gc_force_wait = 1;
            let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::eval::eval::Rf_eval(symbol, base)
            }));
            assert!(denied.is_err());
            assert_eq!(calls.get(), 1);
            assert_eq!(crate::sexp::accessors::PRVALUE(promise.as_raw()), unbound);
            let node = promise.allocation().unwrap();
            assert_eq!(
                node.heap_identity()
                    .node_snapshot(node)
                    .unwrap()
                    .sxpinfo
                    .gp(),
                2
            );
        });
        let replacement = replacement.borrow();
        let replacement = replacement.as_ref().unwrap();
        replacement.with_active(|| unsafe {
            let owner = replacement.owner_token().unwrap();
            let base = crate::sexp::globals::R_BaseEnv();
            install_task_callback_symbols(base);
            let value = source(replacement, binding.to_str().unwrap());
            if binding.to_bytes().starts_with(b".") {
                assert_eq!(value.try_vector_elt(3).unwrap().integer_elt(0), Some(4));
            } else {
                assert_eq!(value.typeof_(), SEXPTYPE::CLOSXP);
            }
            assert!(owner.node_factory().character("live replacement").is_ok());
        });
    }
}

#[test]
fn task_callback_iteration_cursor_selects_without_heap_storage_at_zero_headroom() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let fun = source(&session, "function(...) TRUE");
    let first = session.with_active(|| unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) });
    let second = session.with_active(|| unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) });
    let held = owner
        .with_arena(|a| a.reserve_workspace(128).unwrap())
        .unwrap();
    let (budget, bytes, workspace) = owner
        .with_arena(|a| {
            let before = (a.budget(), a.total_bytes_allocated(), a.workspace_bytes());
            a.set_budget(crate::sexp::memory::ArenaBudget::new(
                before.1 + before.2,
                0,
            ));
            before
        })
        .unwrap();
    let weak = owner.weak_owner().unwrap();
    let result = crate::sexp::owner::with_runtime(&weak, |access| {
        let mut cursor = TaskCallbackCursor::default();
        assert_eq!(cursor.next(access)?.unwrap().id, first);
        assert_eq!(cursor.next(access)?.unwrap().id, second);
        assert!(cursor.next(access)?.is_none());
        let observed = access.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))?;
        Ok::<_, crate::sexp::object::SexpError>((cursor.native_capacity(), observed))
    })
    .unwrap()
    .unwrap();
    owner.with_arena(|a| a.set_budget(budget)).unwrap();
    assert_eq!(
        result.1,
        (bytes, workspace),
        "selection does not consume the held original-ledger ticket"
    );
    assert_eq!(
        result.0, 0,
        "iteration must not allocate uncharged native backing at zero headroom"
    );
    assert!(
        !std::mem::needs_drop::<TaskCallbackCursor>(),
        "the cursor must not retain a hidden heap owner"
    );
    drop(held);
}

#[test]
fn task_callback_cursor_restart_and_cancellation_preserve_original_registration() {
    let mut session = RSession::new_for_gc_tests();
    source(&session, "cursor_hits<-0L");
    let fun = source(&session, "function(...){cursor_hits<<-cursor_hits+1L;TRUE}");
    let id = session.with_active(|| unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) });
    let token = crate::sexp::session::CancellationToken::cancelled();
    session.set_cancellation_token(Some(token.clone()));
    let denied = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE)
        }))
    });
    assert!(denied.is_err());
    session.with_active_in(|instance| unsafe {
        assert!(!(*instance).main_state.running_toplevel_handlers);
        assert_eq!((*instance).main_state.task_callbacks.len(), 1);
    });
    token.reset();
    assert_eq!(source(&session, "cursor_hits").integer_elt(0), Some(0));
    for _ in 0..2 {
        session.with_active(|| unsafe {
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE)
        });
    }
    assert_eq!(source(&session, "cursor_hits").integer_elt(0), Some(2));
    let selector = integer(&session, id);
    session.with_active(|| assert_eq!(unsafe { Rf_removeTaskCallback(selector.as_raw()) }, TRUE));
    session
        .with_active(|| unsafe { Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE) });
    assert_eq!(source(&session, "cursor_hits").integer_elt(0), Some(2));
    let other = RSession::new_for_gc_tests();
    source(&other, "cursor_hits<-17L");
    let other_fun = source(&other, "function(...){cursor_hits<<-cursor_hits+1L;FALSE}");
    other.with_active(|| unsafe { Rf_addTaskCallback(other_fun.as_raw(), R_NilValue()) });
    other
        .with_active(|| unsafe { Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE) });
    assert_eq!(source(&other, "cursor_hits").integer_elt(0), Some(18));
    assert_eq!(source(&session, "cursor_hits").integer_elt(0), Some(2));
}

#[test]
fn task_callback_cursor_identity_overflow_and_regression_reject_without_publication() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let fun = source(&session, "function(...) TRUE");
    let first = session.with_active(|| unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) });
    for corrupt in [c_int::MAX, first - 1] {
        let before = session.with_active_in(|instance| unsafe {
            let s = &mut (*instance).main_state;
            s.next_task_callback_id = corrupt;
            (s.task_callbacks.len(), s.task_callbacks.capacity())
        });
        let result = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                Rf_addTaskCallback(fun.as_raw(), R_NilValue())
            }))
        });
        assert!(result.is_err());
        session.with_active_in(|instance| unsafe {
            let s = &(*instance).main_state;
            assert_eq!(
                (s.task_callbacks.len(), s.task_callbacks.capacity()),
                before
            );
            assert_eq!(s.task_callbacks[0].id, first);
        });
    }
    session.with_active_in(|instance| unsafe {
        (*instance).main_state.next_task_callback_id = first;
    });
    let second = session.with_active(|| unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) });
    assert!(second > first);
    let weak = owner.weak_owner().unwrap();
    crate::sexp::owner::with_runtime(&weak, |access| {
        let mut cursor = TaskCallbackCursor::default();
        assert_eq!(cursor.next(access)?.unwrap().id, first);
        assert_eq!(cursor.next(access)?.unwrap().id, second);
        assert!(cursor.next(access)?.is_none());
        Ok::<_, crate::sexp::object::SexpError>(())
    })
    .unwrap()
    .unwrap();
}
