use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// tryCatch(..., warning = ) exiting handlers must catch warnings
    /// raised in the body; unmatched classes must not catch.
    #[test]
    fn test_try_catch_warning_handler() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            "tryCatch(warning('x'), warning = function(e) paste('caught:', conditionMessage(e)))",
        );
        let value = result
            .map(|s| unsafe {
                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::STRING_ELT(s.as_raw(), 0),
                ))
                .to_string_lossy()
                .into_owned()
            })
            .unwrap_or_default();
        assert_eq!(value, "caught: x");
        // The caught warning is neither printed nor collected.
        assert!(!output.stderr.contains("Warning message"));

        // An error-only frame does not catch warnings: the warning falls
        // through to the default print path and tryCatch returns the
        // warning() return value — the message string, invisibly (stock
        // do_warning returns CAR(args)).
        let (result, output, _) = session.eval_script_with_output_capture(
            "identical(tryCatch(warning('y'), error = function(e) 'E'), 'y')",
        );
        let identical = result
            .map(|s| unsafe { crate::sexp::accessors::LOGICAL(s.as_raw()).read() == TRUE })
            .unwrap_or(false);
        assert!(identical);
        // The uncaught warning is deferred into the collection buffer
        // (rendered by the REPL tail / result assembly, not the captured
        // streams of this raw session API) — it must be pending, with the
        // rendered block available, and the buffer must be drained after.
        assert!(unsafe { crate::mainutils::errors::collect_warnings() } > 0);
        let block = unsafe { crate::mainutils::errors::take_warnings_block() };
        assert!(block.is_some_and(|b| b.contains("Warning message")));
    }

    /// suppressWarnings keeps muting warnings (capture-based), including
    /// with an exiting warning handler in scope of the wider script.
    #[test]
    fn test_suppress_warnings_still_mutes() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, output, _) =
            session.eval_script_with_output_capture("suppressWarnings(warning('quiet'))");
        assert!(result.is_ok());
        assert!(!output.stderr.contains("quiet"));
    }

    #[test]
    fn signalled_condition_is_session_local_and_gc_rooted() {
        let first = crate::sexp::session::RSession::new();
        unsafe {
            let condition = simple_error_condition("survives gc");
            set_signalled_condition(condition);
        }

        first.gc();
        let retained = signalled_condition();
        assert!(!retained.is_null());
        let message = unsafe { condition_message_of(retained) };
        assert_eq!(message.as_deref(), Some("survives gc"));

        // RSession::new installs a fresh instance on this same thread. A
        // thread-local slot would incorrectly expose `retained` here.
        let second = crate::sexp::session::RSession::new();
        assert!(signalled_condition().is_null());
        drop(second);
        drop(first);
    }

    #[test]
    fn mathlib_warning_guard_roots_nested_calls_and_restores_owner() {
        let first = crate::sexp::session::RSession::new();
        let outer_call = unsafe { crate::sexp::constructors::Rf_ScalarInteger(11) };
        let outer = crate::mainutils::errors::mathlib_warning_call_guard(outer_call);
        let nested_call = unsafe { crate::sexp::constructors::Rf_ScalarInteger(22) };
        let nested = crate::mainutils::errors::mathlib_warning_call_guard(nested_call);

        first.gc();
        let current = crate::mainutils::errors::mathlib_warning_call();
        assert_eq!(
            unsafe { crate::sexp::accessors::INTEGER_ELT(current, 0) },
            22
        );
        drop(nested);
        let restored = crate::mainutils::errors::mathlib_warning_call();
        assert_eq!(
            unsafe { crate::sexp::accessors::INTEGER_ELT(restored, 0) },
            11
        );

        // Dropping while another session is ambient must still restore the
        // owner session, rather than writing the current session's slot.
        let second = crate::sexp::session::RSession::new();
        drop(outer);
        first.with_active(|| {
            assert!(crate::mainutils::errors::mathlib_warning_call().is_null());
        });
        drop(second);
        drop(first);
    }
}

#[cfg(test)]
mod owned_calling_handler_tests {

    use std::{cell::Cell, rc::Rc};

    #[test]
    fn owned_calling_handler_entries_survive_collection_during_later_handler_construction() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = crate::sexp::object::SessionNodeFactory::new(owner);
        let global = session.global_env().unwrap();
        let expression = owner.with_arena(|arena| crate::eval::parser::parse(
            "{ hits <- 0L; withCallingHandlers({ warning('collected'); 41L }, warning=function(w) { hits <<- hits + 1L; gc() }, simpleWarning={ gc(); function(w) { hits <<- hits + 10L; gc() } }); hits }",
            arena, factory.clone(),
        )).unwrap().unwrap();
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        let (handlers, restarts) = session.with_active_in(|instance| unsafe {
            (
                (*instance).error_state.handler_stack.as_raw(),
                (*instance).error_state.restart_stack.as_raw(),
            )
        });
        let protections = crate::sexp::protect::R_ProtectCount();
        let result = factory
            .wrap(unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), global.as_raw()) })
            .unwrap();
        assert_eq!(result.integer_elt(0), Some(11));
        assert!(collections.get() > 0);
        session.with_active_in(|instance| unsafe {
            assert_eq!((*instance).error_state.handler_stack.as_raw(), handlers);
            assert_eq!((*instance).error_state.restart_stack.as_raw(), restarts);
        });
        assert_eq!(crate::sexp::protect::R_ProtectCount(), protections);
    }
}

#[cfg(test)]
mod owned_restart_transfer_tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    fn assert_true(session: &mut crate::sexp::session::RSession, script: &str) {
        let (result, _, _) = session.eval_code_with_output_capture(script);
        let result = result.unwrap_or_else(|error| panic!("{script}: {error}"));
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn owned_restart_tickets_preserve_nested_rethrow_and_multiple_restart_routing() {
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        assert_true(
            &mut session,
            "identical(withRestarts(withRestarts(invokeRestart('outer',41L),inner=function(x)stop('wrong restart')),outer=function(x){gc();x+1L}),42L)",
        );
        assert_true(
            &mut session,
            "identical(withRestarts(invokeRestart('first',3L),first=function(x)invokeRestart('second',x+1L),second=function(y){gc();y+4L}),8L)",
        );
        assert_true(
            &mut session,
            "withRestarts(invokeRestart('data',quote(answer)),data=function(x){gc();identical(x,quote(answer))})",
        );
        assert_true(
            &mut session,
            "identical(withCallingHandlers({warning('muffled');message('muffled');9L},warning=function(w)invokeRestart('muffleWarning'),message=function(m)invokeRestart('muffleMessage')),9L)",
        );
    }

    #[test]
    fn owned_restart_handler_keeps_transferred_arguments_after_stack_replacement_and_gc() {
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        // The real handler changes this flag only after its transfer is caught
        // and removed from the installed dynamic restart stack.
        assert_true(&mut session, "entered_restart_handler<-FALSE;TRUE");
        let (global, flag) = session.with_active(|| unsafe {
            (
                restart_owned(crate::sexp::globals::R_GlobalEnv()),
                restart_owned(Rf_install(c"entered_restart_handler".as_ptr())),
            )
        });
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| unsafe {
            let entered = crate::sexp::envir::R_findVarInFrame(global.as_raw(), flag.as_raw());
            if !entered.is_null() && entered != R_UnboundValue() && LOGICAL_ELT(entered, 0) == TRUE
            {
                set_restart_stack(R_NilValue());
                crate::sexp::gengc::full_gc();
                observed.set(observed.get() + 1);
            }
        }));
        assert_true(
            &mut session,
            "withRestarts(invokeRestart('collect',list(quote(answer),c(17L,29L))),collect=function(x){entered_restart_handler<<-TRUE;gc();identical(x[[1L]],quote(answer))&&identical(x[[2L]],c(17L,29L))})",
        );
        assert!(collections.get() > 0);
        assert_true(
            &mut session,
            "entered_restart_handler<-FALSE;identical(1L+1L,2L)",
        );
    }
}

#[cfg(test)]
mod owned_error_trycatch_tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn owned_error_nested_trycatch_preserves_custom_conditions_through_collecting_finally() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = session.global_env().unwrap();
        let flag = factory
            .wrap(unsafe { Rf_install(c"owned_finally_collecting".as_ptr()) })
            .unwrap()
            .into_owned()
            .unwrap();
        let callback_global = global.clone().into_owned().unwrap();
        let callbacks = Rc::new(Cell::new(0));
        let observed = callbacks.clone();
        session.with_active_in(|instance| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                let entered =
                    crate::sexp::envir::R_findVarInFrame(callback_global.as_raw(), flag.as_raw());
                if !entered.is_null()
                    && entered != R_UnboundValue()
                    && LOGICAL_ELT(entered, 0) == TRUE
                {
                    (*instance).error_state.signalled_condition =
                        crate::sexp::instance::RuntimeValue::empty();
                    crate::sexp::gengc::full_gc_in(instance);
                    observed.set(observed.get() + 1);
                }
            }));
        });
        for (signal, family) in [
            ("message", "message"),
            ("warning", "warning"),
            ("stop", "error"),
        ] {
            let script = format!(
                "{{owned_finally_collecting<-FALSE;owned_finally_ran<-FALSE;tryCatch(tryCatch({signal}(structure(list(message='owned payload',call=NULL,extra=41L),class=c('ownedCustom','{family}','condition'))), unrelated=function(e)FALSE, finally={{owned_finally_collecting<<-TRUE;gc();owned_finally_collecting<<-FALSE;owned_finally_ran<<-TRUE}}), ownedCustom=function(e){{gc();identical(class(e),c('ownedCustom','{family}','condition'))&&identical(conditionMessage(e),'owned payload')&&identical(e$extra,41L)&&owned_finally_ran}})}}"
            );
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(&script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            let result = factory
                .wrap(unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), global.as_raw()) })
                .unwrap();
            assert_eq!(
                result.logical_elt(0),
                Some(TRUE),
                "{signal} must preserve its exact custom condition and run finally"
            );
            session.with_active_in(|instance| unsafe {
                assert!((*instance).error_state.signalled_condition.is_null());
                assert!((*instance).error_state.try_catch_handler_classes.is_empty());
                assert!((*instance).error_state.try_catch_nframes.is_empty());
            });
        }
        assert!(
            callbacks.get() >= 3,
            "each finally must actually invoke collection"
        );
    }

    #[test]
    fn owned_error_trycatch_finally_runs_when_matched_handler_unwinds() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = session.global_env().unwrap();
        let expression = owner.with_arena(|arena| crate::eval::parser::parse(
            "{owned_handler_finally<-FALSE;tryCatch(tryCatch(stop('original'),error=function(e)stop(structure(list(message='handler failure',call=NULL,extra=23L),class=c('handlerFailure','error','condition'))),finally={gc();owned_handler_finally<<-TRUE}),handlerFailure=function(e){gc();owned_handler_finally&&identical(conditionMessage(e),'handler failure')&&identical(e$extra,23L)})}",
            arena, factory.domain(),
        )).unwrap().unwrap();
        let result = factory
            .wrap(unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), global.as_raw()) })
            .unwrap();
        assert_eq!(result.logical_elt(0), Some(TRUE));
        session.with_active_in(|instance| unsafe {
            assert!((*instance).error_state.signalled_condition.is_null());
            assert!((*instance).error_state.try_catch_handler_classes.is_empty());
            assert!((*instance).error_state.try_catch_nframes.is_empty());
        });
    }

    #[test]
    fn owned_error_trycatch_finally_new_condition_takes_precedence() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = session.global_env().unwrap();
        let expression = owner.with_arena(|arena| crate::eval::parser::parse(
            "tryCatch(tryCatch(stop(structure(list(message='pending failure',call=NULL),class=c('pendingFailure','error','condition'))),unrelated=function(e)FALSE,finally={gc();stop(structure(list(message='finally failure',call=NULL,extra=29L),class=c('finallyFailure','error','condition')))}),finallyFailure=function(e){gc();identical(conditionMessage(e),'finally failure')&&identical(e$extra,29L)},pendingFailure=function(e)FALSE)",
            arena, factory.domain(),
        )).unwrap().unwrap();
        let result = factory
            .wrap(unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), global.as_raw()) })
            .unwrap();
        assert_eq!(result.logical_elt(0), Some(TRUE));
        session.with_active_in(|instance| unsafe {
            assert!((*instance).error_state.signalled_condition.is_null());
            assert!((*instance).error_state.try_catch_handler_classes.is_empty());
            assert!((*instance).error_state.try_catch_nframes.is_empty());
        });
    }

    #[test]
    fn owned_error_trycatch_roots_handlers_results_and_conditions_across_finally_gc() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = session.global_env().unwrap();
        let observed = Rc::new(Cell::new(0));
        let callbacks = observed.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            callbacks.set(callbacks.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        for script in [
            "tryCatch(stop('owned'), error=function(e){gc();41L}, warning={gc();function(w)-1L}, finally=gc())",
            "tryCatch({gc();c(41L,42L)}, error=function(e)-1L, finally=gc())",
            "tryCatch(stop(simpleError('own condition')), error=function(e){gc();conditionMessage(e)}, finally=gc())",
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            let result = factory
                .wrap(unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), global.as_raw()) })
                .unwrap();
            if result.typeof_() == SEXPTYPE::STRSXP {
                assert!(
                    result
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq(b"own condition")
                        .unwrap()
                );
            } else {
                assert_eq!(result.integer_elt(0), Some(41));
            }
            session.with_active_in(|instance| unsafe {
                assert!((*instance).error_state.try_catch_handler_classes.is_empty());
                assert!((*instance).error_state.try_catch_nframes.is_empty());
                assert!((*instance).error_state.signalled_condition.is_null());
            });
        }
        assert!(observed.get() > 0);
    }
}
