#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Deferred warning printing: PrintWarnings and the deferred-warnings
//! entry points.

use super::*;

// ---------------------------------------------------------------------------
// PrintWarnings
// ---------------------------------------------------------------------------

fn warning_checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

struct WarningPrintScope {
    owner: crate::sexp::owner::OwnerPin,
    previous: c_int,
}
impl Drop for WarningPrintScope {
    fn drop(&mut self) {
        // Physical cleanup remains valid after revocation and ambient switch.
        unsafe {
            (*self.owner.as_ptr()).error_state.in_print_warnings = self.previous;
        }
    }
}
struct WarningEntry {
    call: crate::sexp::object::Sexp<'static>,
    name: Option<crate::sexp::object::Sexp<'static>>,
    message: String,
}

/// Render the collected-warnings block exactly like errors.c `PrintWarnings()`
/// and consume the collection state (including the truncated `last.warning`
/// install). Returns `None` when there is nothing to print.
///
/// Callers own the emission channel: `PrintWarnings()` writes the block to
/// stderr like upstream REprintf, while the script-loop flush routes it into
/// the session output stream to keep Rscript's terminal interleaving.
///
/// Rendering (errors.c:615-673): a single warning prints
/// `Warning message:` then `In <dcall> : <msg>` (or `<msg> ` without a call);
/// two to ten print `Warning messages:` with an `N: ` prefix; longer counts
/// collapse to a summary line. `dcall` is `deparse1s()` of the stored call,
/// and a first line that would exceed LONGWARN (6/10 + dcall + msgline1)
/// wraps with `\n ` before the one-space-indented message.
pub(crate) unsafe fn take_warnings_block() -> Option<String> {
    let pin = super::state::error_scope_pin();
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pin.as_ptr()) };
    let affiliation = owner.weak_owner().expect("managed warning owner");
    warning_checked(crate::sexp::owner::with_runtime(&affiliation, |access| {
        let pointer = pin.as_ptr();
        let (count, previous, capacity) = unsafe {
            (
                (*pointer).error_state.collect_warnings,
                (*pointer).error_state.in_print_warnings,
                (*pointer).error_state.nwarnings,
            )
        };
        if count == 0 {
            return None;
        }
        if previous != 0 {
            unsafe {
                (*pointer).error_state.collect_warnings = 0;
                (*pointer).error_state.warnings = crate::sexp::instance::RuntimeValue::empty();
            }
            return Some("Lost warning messages\n".into());
        }
        let scope = WarningPrintScope {
            owner: pin,
            previous,
        };
        unsafe {
            (*pointer).error_state.in_print_warnings = 1;
        }
        let Some(warnings) = (unsafe { (*pointer).error_state.warnings.owned() }) else {
            return None;
        };
        if warnings.typeof_() != SEXPTYPE::VECSXP {
            return None;
        }
        let attributes = warning_checked(warnings.try_attrib());
        let names = if attributes.is_nil() {
            None
        } else {
            let candidate = warning_checked(attributes.try_car());
            (candidate.typeof_() == SEXPTYPE::STRSXP).then_some(candidate)
        };
        let count_usize = usize::try_from(count)
            .unwrap_or_else(|_| crate::sexp::context::r_error("invalid collected warning count"));
        if count as i64 > warnings.len() {
            crate::sexp::context::r_error("collected warnings exceed owning vector length");
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count_usize)
            .unwrap_or_else(|_| crate::sexp::context::r_error("cannot reserve warning snapshots"));
        for index in 0..count as i64 {
            let call = warning_checked(warnings.try_vector_elt(index));
            let name = names
                .as_ref()
                .map(|names| warning_checked(names.try_string_elt(index)));
            let message = name
                .as_ref()
                .map(|name| warning_checked(name.try_as_string()))
                .unwrap_or_default();
            warning_checked(access.require_active());
            entries.push(WarningEntry {
                call,
                name,
                message,
            });
        }
        // All selected children are independent roots before the first
        // deparser allocation can detach the vector or rewrite its slots.
        let mut block = String::new();
        if count == 1 {
            block.push_str("Warning message:\n");
            let entry = &entries[0];
            if entry.call.is_nil() {
                block.push_str(&entry.message);
                block.push_str(" \n");
            } else {
                let dcall = warning_dcall_in_scope(&entry.call, access);
                block.push_str("In ");
                block.push_str(&dcall);
                block.push_str(" :");
                let first = entry.message.split('\n').next().map_or(0, str::len);
                if 6 + dcall.len() + first > LONGWARN {
                    block.push_str("\n ");
                }
                block.push(' ');
                block.push_str(&entry.message);
                block.push('\n');
            }
        } else if count <= 10 {
            block.push_str("Warning messages:\n");
            for (index, entry) in entries.iter().enumerate() {
                if entry.call.is_nil() {
                    block.push_str(&format!("{}: {} \n", index + 1, entry.message));
                } else {
                    let dcall = warning_dcall_in_scope(&entry.call, access);
                    block.push_str(&format!("{}: In {} :", index + 1, dcall));
                    let first = entry.message.split('\n').next().map_or(0, str::len);
                    if 10 + dcall.len() + first > LONGWARN {
                        block.push_str("\n ");
                    }
                    block.push(' ');
                    block.push_str(&entry.message);
                    block.push('\n');
                }
            }
        } else {
            if count < capacity {
                block.push_str(&format!(
                    "There were {} warnings (use warnings() to see them)\n",
                    count
                ));
            } else {
                block.push_str(&format!(
                    "There were {} or more warnings (use warnings() to see the first {})\n",
                    capacity, capacity
                ));
            }
        }
        let domain = access.domain();
        let allocator = warning_checked(access.allocator(&domain));
        let symbol = warning_checked(
            access.with_native(|_| domain.wrap(unsafe { Rf_install(c"last.warning".as_ptr()) })),
        );
        let last = warning_checked(
            allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, count as i64))),
        );
        let last_names = warning_checked(
            allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, count as i64))),
        );
        let mut last = warning_checked(crate::sexp::object::SexpMut::try_from_checked(last));
        let mut last_names =
            warning_checked(crate::sexp::object::SexpMut::try_from_checked(last_names));
        for (index, entry) in entries.iter().enumerate() {
            warning_checked(last.try_set_vector_elt(index as i64, entry.call.clone()));
            let name = entry
                .name
                .clone()
                .unwrap_or_else(|| warning_checked(allocator.character("")));
            warning_checked(last_names.try_set_string_elt(index as i64, name));
            warning_checked(access.require_active());
        }
        let last = last.freeze();
        let last_names = last_names.freeze();
        warning_checked(access.with_native(|_| {
            unsafe {
                let names_symbol = R_NamesSymbol();
                access.require_active()?;
                setAttrib_wrap(last.as_raw(), names_symbol, last_names.as_raw());
                access.require_active()?;
                SET_SYMVALUE(symbol.as_raw(), last.as_raw());
            }
            Ok(())
        }));
        warning_checked(access.require_active());
        unsafe {
            (*pointer).error_state.collect_warnings = 0;
            (*pointer).error_state.warnings = crate::sexp::instance::RuntimeValue::empty();
        }
        drop(scope);
        Some(block)
    }))
}

/// `deparse1s()` of a stored warning call as a Rust string (errors.c uses the
/// same rendering for the `In <call> :` header). Falls back to `<call>` when
/// the deparse yields nothing usable, mirroring the error renderer above.
fn warning_dcall_in_scope(
    call: &crate::sexp::object::Sexp<'static>,
    access: &crate::sexp::owner::RuntimeAccess,
) -> String {
    let domain = access.domain();
    let deparsed = warning_checked(access.with_native(|_| {
        domain.wrap(unsafe { crate::mainutils::deparse::deparse1s(call.as_raw()) })
    }));
    if deparsed.typeof_() != SEXPTYPE::STRSXP || deparsed.len() == 0 {
        return "<call>".into();
    }
    let character = warning_checked(deparsed.try_string_elt(0));
    let text = warning_checked(character.try_as_string());
    warning_checked(access.require_active());
    text
}
pub(crate) unsafe fn warning_dcall(call: SEXP) -> String {
    if call.is_null() {
        return "<call>".into();
    }
    let pin = super::state::error_scope_pin();
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pin.as_ptr()) };
    let call = warning_checked(
        owner
            .sexp(call)
            .and_then(crate::sexp::object::Sexp::into_owned),
    );
    let affiliation = owner.weak_owner().expect("managed warning owner");
    warning_checked(crate::sexp::owner::with_runtime(&affiliation, |access| {
        warning_dcall_in_scope(&call, access)
    }))
}

/// Print collected warnings to stderr — upstream's channel (REprintf).
pub unsafe fn PrintWarnings() {
    unsafe {
        if let Some(block) = take_warnings_block() {
            eprint!("{}", block);
        }
    }
}

/// Flush collected warnings at a top-level statement boundary.
/// GNU writes this block with REprintf (process stderr).
pub unsafe fn print_warnings_at_statement_boundary() {
    unsafe {
        let Some(block) = take_warnings_block() else {
            return;
        };
        crate::sexp::output::capture_stderr(&block);
    }
}

/// do_printDeferredWarnings — print deferred warnings.
pub unsafe fn do_printDeferredWarnings(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if r_show_error_messages() && collect_warnings() > 0 {
            PrintWarnings();
        }
        globals::R_NilValue()
    }
}

/// R_PrintDeferredWarnings — print deferred warnings.
/// Matches C's `static void R_PrintDeferredWarnings(void)`
pub unsafe fn R_PrintDeferredWarnings() {
    unsafe {
        if r_show_error_messages() && collect_warnings() > 0 {
            eprint!("In addition: ");
            PrintWarnings();
        }
    }
}

#[cfg(test)]
mod owned_warning_snapshot_tests {
    use super::*;
    use crate::sexp::{instance::RuntimeValue, session::RSession};
    use std::{cell::Cell, rc::Rc};

    fn warnings_fixture(
        session: &RSession,
    ) -> (
        SEXP,
        SEXP,
        crate::sexp::heap::CheckedNode,
        crate::sexp::heap::CheckedNode,
    ) {
        session.with_active_in(|instance| unsafe {
            super::super::render::setup_warnings();
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let factory = owner.node_factory();
            let warnings = (*instance).error_state.warnings.owned().unwrap();
            let names = warnings.try_attrib().unwrap().try_car().unwrap();
            let call = owner
                .sexp(Rf_lang2(
                    Rf_install(c"original_warning_call".as_ptr()),
                    Rf_ScalarInteger(23),
                ))
                .unwrap()
                .into_owned()
                .unwrap();
            let message = factory.character("original collected message").unwrap();
            let second = factory.character("second collected message").unwrap();
            let call_node = call.allocation().unwrap().clone();
            let message_node = message.allocation().unwrap().clone();
            let mut vector =
                crate::sexp::object::SexpMut::try_from_checked(warnings.clone()).unwrap();
            vector.try_set_vector_elt(0, call).unwrap();
            vector.try_set_vector_elt(1, factory.nil()).unwrap();
            let mut names_writer =
                crate::sexp::object::SexpMut::try_from_checked(names.clone()).unwrap();
            names_writer.try_set_string_elt(0, message).unwrap();
            names_writer.try_set_string_elt(1, second).unwrap();
            (*instance).error_state.collect_warnings = 2;
            (warnings.as_raw(), names.as_raw(), call_node, message_node)
        })
    }

    #[test]
    fn owned_error_warning_snapshots_survive_field_and_child_replacement_during_deparse_gc() {
        let session = RSession::new_for_gc_tests();
        let (warnings, names, original_call, original_message) = warnings_fixture(&session);
        let collected = Rc::new(Cell::new(false));
        let observed = collected.clone();
        session.with_active_in(|instance| unsafe {
            let replacement = crate::sexp::owner::OwnerToken::from_raw(instance)
                .node_factory()
                .character("callback replacement")
                .unwrap()
                .into_owned()
                .unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                (*instance).error_state.warnings = RuntimeValue::empty();
                (*instance).error_state.collect_warnings = 0;
                SET_VECTOR_ELT(warnings, 0, globals::R_NilValue());
                SET_VECTOR_ELT(warnings, 1, globals::R_NilValue());
                SET_STRING_ELT(names, 0, replacement.as_raw());
                SET_STRING_ELT(names, 1, replacement.as_raw());
                crate::sexp::gengc::full_gc_in(instance);
                assert!(
                    original_call.is_live(),
                    "selected call must own an independent root"
                );
                assert!(
                    original_message.is_live(),
                    "selected message must own an independent root"
                );
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let block = take_warnings_block().unwrap();
            assert!(
                collected.get(),
                "the deparser must actually trigger collection"
            );
            assert!(
                block.contains("original_warning_call(23L) : original collected message"),
                "{block}"
            );
            assert!(block.contains("2: second collected message"), "{block}");
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let last = owner
                .sexp(SYMVALUE(Rf_install(c"last.warning".as_ptr())))
                .unwrap();
            assert_eq!(last.len(), 2);
            assert_eq!(last.try_vector_elt(0).unwrap().typeof_(), SEXPTYPE::LANGSXP);
            let last_names = last.try_attrib().unwrap().try_car().unwrap();
            assert_eq!(
                last_names
                    .try_string_elt(0)
                    .unwrap()
                    .try_as_string()
                    .unwrap(),
                "original collected message"
            );
            assert_eq!(
                last_names
                    .try_string_elt(1)
                    .unwrap()
                    .try_as_string()
                    .unwrap(),
                "second collected message"
            );
            assert_eq!((*instance).error_state.collect_warnings, 0);
            assert_eq!((*instance).error_state.in_print_warnings, 0);
        });
    }

    #[test]
    fn owned_error_warning_collection_keeps_initialized_pool_across_collecting_callbacks() {
        let session = RSession::new_for_gc_tests();
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        session.with_active_in(|instance| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                (*instance).error_state.warnings = RuntimeValue::empty();
                (*instance).error_state.collect_warnings = 0;
                crate::sexp::gengc::full_gc_in(instance);
                observed.set(observed.get() + 1);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            super::super::render::vwarningcall_dflt(
                globals::R_NilValue(),
                c"collecting warning".as_ptr(),
                ptr::null_mut(),
            );
            assert!(
                collections.get() >= 3,
                "setup and message allocation must actually collect"
            );
            assert_eq!((*instance).error_state.collect_warnings, 1);
            assert_eq!((*instance).error_state.in_warning, 0);
            let warnings = (*instance).error_state.warnings.owned().unwrap();
            let names = warnings.try_attrib().unwrap().try_car().unwrap();
            assert_eq!(
                names.try_string_elt(0).unwrap().try_as_string().unwrap(),
                "collecting warning"
            );
            let block = take_warnings_block().unwrap();
            assert_eq!(block, "Warning message:\ncollecting warning \n");
            assert_eq!((*instance).error_state.in_print_warnings, 0);
        });
    }

    #[test]
    fn owned_error_warning_entry_owns_override_before_first_option_allocation() {
        let session = RSession::new_for_gc_tests();
        let collected = Rc::new(Cell::new(false));
        let observed = collected.clone();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let call = owner
                .sexp(Rf_lang2(
                    Rf_install(c"original_warning_call".as_ptr()),
                    Rf_ScalarInteger(23),
                ))
                .unwrap()
                .into_owned()
                .unwrap();
            let node = call.allocation().unwrap().clone();
            let raw = call.as_raw();
            (*instance).error_state.warning_call = RuntimeValue::from_owned(call);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                (*instance).error_state.warning_call = RuntimeValue::empty();
                crate::sexp::gengc::full_gc_in(instance);
                assert!(
                    node.is_live(),
                    "incoming warning call must own its actual root"
                );
                observed.set(true);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            super::super::render::vwarningcall_dflt(
                raw,
                c"entry warning".as_ptr(),
                ptr::null_mut(),
            );
            assert!(
                collected.get(),
                "option/setup allocation must invoke collection"
            );
            let warnings = (*instance).error_state.warnings.owned().unwrap();
            assert_eq!(warnings.try_vector_elt(0).unwrap().as_raw(), raw);
            assert_eq!((*instance).error_state.in_warning, 0);
        });
    }

    #[test]
    fn owned_error_warning_integer_levels_and_na_immediate_defaults_match_gnu() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            crate::mainutils::options::R_SetOptionWarn(2);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::super::render::vwarningcall_dflt(
                    globals::R_NilValue(),
                    c"converted warning".as_ptr(),
                    ptr::null_mut(),
                );
            }));
            let payload = outcome.expect_err("warn=2 must convert to an error");
            let error = payload.downcast_ref::<RError>().expect("typed R error");
            assert!(error.message.contains("converted from warning"));
            assert_eq!((*instance).error_state.in_warning, 0);
            crate::mainutils::options::R_SetOptionWarn(crate::sexp::ffi::NA_INTEGER);
            super::super::render::vwarningcall_dflt(
                globals::R_NilValue(),
                c"NA collects".as_ptr(),
                ptr::null_mut(),
            );
            assert_eq!((*instance).error_state.collect_warnings, 1);
            crate::mainutils::options::R_SetOptionWarn(-1);
            set_immediate_warning(true);
            super::super::render::vwarningcall_dflt(
                globals::R_NilValue(),
                c"negative immediate".as_ptr(),
                ptr::null_mut(),
            );
            set_immediate_warning(false);
            assert_eq!((*instance).error_state.collect_warnings, 1);
            assert_eq!((*instance).error_state.in_warning, 0);
        });
    }

    #[test]
    fn owned_error_warning_expression_survives_option_replacement_and_preserves_visibility() {
        let session = RSession::new_for_gc_tests();
        let collected = Rc::new(Cell::new(false));
        let observed = collected.clone();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let expression = owner
                .sexp(Rf_lang3(
                    Rf_install(c"<-".as_ptr()),
                    Rf_install(c"warning_expression_result".as_ptr()),
                    Rf_lang2(Rf_install(c"list".as_ptr()), Rf_ScalarInteger(19)),
                ))
                .unwrap()
                .into_owned()
                .unwrap();
            let expression_node = expression.allocation().unwrap().clone();
            crate::mainutils::options::SetOptionByName("warning.expression", expression.as_raw());
            drop(expression);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    crate::mainutils::options::SetOptionByName(
                        "warning.expression",
                        globals::R_NilValue(),
                    );
                    crate::sexp::gengc::full_gc_in(instance);
                    assert!(
                        expression_node.is_live(),
                        "detached option expression must stay rooted"
                    );
                }
            }));
            (*instance).eval_state.visible = crate::sexp::ffi::TRUE;
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            super::super::render::vwarningcall_dflt(
                globals::R_NilValue(),
                c"option warning".as_ptr(),
                ptr::null_mut(),
            );
            assert!(collected.get(), "option evaluation must actually collect");
            assert_eq!((*instance).eval_state.visible, crate::sexp::ffi::TRUE);
            assert_eq!((*instance).error_state.in_warning, 0);
            assert_eq!((*instance).error_state.collect_warnings, 0);
            let result = owner
                .sexp(crate::sexp::envir::R_findVar(
                    Rf_install(c"warning_expression_result".as_ptr()),
                    (*instance).global_env,
                ))
                .unwrap();
            assert_eq!(result.typeof_(), SEXPTYPE::VECSXP);
            assert_eq!(
                result.try_vector_elt(0).unwrap().integer_elt(0).unwrap(),
                19
            );
            crate::mainutils::options::SetOptionByName("warning.expression", Rf_ScalarInteger(1));
            let invalid = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::super::render::vwarningcall_dflt(
                    globals::R_NilValue(),
                    c"invalid option".as_ptr(),
                    ptr::null_mut(),
                );
            }));
            assert!(invalid.is_err());
            assert_eq!((*instance).error_state.in_warning, 0);
        });
    }

    #[test]
    fn owned_error_warning_expression_can_clear_itself_and_collect_nested_warning() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let expression = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "{ options(warning.expression=NULL); gc(); warning('nested expression warning') }",
                    arena,
                    factory.domain(),
                )
            })
            .unwrap()
            .unwrap();
        let collected = Rc::new(Cell::new(0));
        let observed = collected.clone();
        session.with_active_in(|instance| unsafe {
            crate::mainutils::options::SetOptionByName("warning.expression", expression.as_raw());
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                crate::sexp::gengc::full_gc_in(instance);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            super::super::render::vwarningcall_dflt(
                globals::R_NilValue(),
                c"outer expression warning".as_ptr(),
                ptr::null_mut(),
            );
            assert!(collected.get() > 0);
            assert_eq!((*instance).error_state.in_warning, 0);
            assert_eq!((*instance).error_state.collect_warnings, 1);
            let block = take_warnings_block().unwrap();
            assert!(block.contains("nested expression warning"), "{block}");
            assert!(!block.contains("outer expression warning"), "{block}");
        });
    }

    #[test]
    fn owned_error_warning_print_flag_cleanup_uses_original_runtime_after_revocation() {
        let session = RSession::new_for_gc_tests();
        let _fixture = warnings_fixture(&session);
        let revoked = Rc::new(Cell::new(false));
        let observed = revoked.clone();
        session.with_active_in(|instance| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    crate::sexp::instance::revoke_instance_availability(instance);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| take_warnings_block()));
            assert!(revoked.get());
            assert!(
                outcome.is_err(),
                "revoked execution cannot publish a warning block"
            );
            assert_eq!((*instance).error_state.in_print_warnings, 0);
        });
    }
}
