use r_embed::{RResourceLimits, RSession, RuntimePathPolicy};

fn eval(code: &str) -> String {
    RSession::new()
        .unwrap()
        .eval(code)
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn continuation_walks_three_declared_classes() {
    assert_eq!(
        eval(
            "local({ setClass('A'); setClass('B',contains='A'); setClass('C',contains='B'); setGeneric('f',function(x)standardGeneric('f')); setMethod('f','A',function(x)'A'); setMethod('f','B',function(x)paste('B',callNextMethod())); setMethod('f','C',function(x)paste('C',callNextMethod())); f(new('C')) })"
        ),
        "[1] \"C B A\""
    );
}

#[test]
fn continuation_reaches_any_fallback() {
    assert_eq!(
        eval(
            "local({ setGeneric('f',function(x)standardGeneric('f')); setMethod('f','ANY',function(x)'any'); setMethod('f','numeric',function(x)paste('numeric',callNextMethod())); f(1) })"
        ),
        "[1] \"numeric any\""
    );
}

#[test]
fn helper_can_continue_the_active_method() {
    assert_eq!(
        eval(
            "local({ setGeneric('f',function(x)standardGeneric('f')); setMethod('f','ANY',function(x)paste('A',x)); helper<-function(...)callNextMethod(...); setMethod('f','numeric',function(x)helper(x)); f(2) })"
        ),
        "[1] \"A 2\""
    );
}

#[test]
fn supplied_arguments_are_rematched_for_the_next_method() {
    assert_eq!(
        eval(
            "local({ setGeneric('f',function(x,y=1)standardGeneric('f')); setMethod('f','ANY',function(x,y)paste('any',x,y)); setMethod('f','numeric',function(x,y)callNextMethod(x,y=9)); f(2,3) })"
        ),
        "[1] \"any 2 9\""
    );
}

#[test]
fn terminal_continuation_reports_an_error() {
    assert_eq!(
        eval(
            "local({ setGeneric('f',function(x)standardGeneric('f')); setMethod('f','numeric',function(x)callNextMethod()); grepl('invalid|next method',tryCatch(f(1),error=function(e)conditionMessage(e)),ignore.case=TRUE) })"
        ),
        "[1] TRUE"
    );
}

#[test]
fn repeated_continuations_restart_from_the_outer_method() {
    assert_eq!(
        eval(
            "local({ setClass('A'); setClass('B',contains='A'); setClass('C',contains='B'); setGeneric('f',function(x)standardGeneric('f')); setMethod('f','A',function(x)'A'); setMethod('f','B',function(x)paste('B',callNextMethod())); setMethod('f','C',function(x)c(callNextMethod(),callNextMethod())); f(new('C')) })"
        ),
        "[1] \"B A\" \"B A\""
    );
}

#[test]
fn continuation_metadata_survives_gc_torture() {
    assert_eq!(
        eval(
            "local({ setClass('A'); setClass('B',contains='A'); setClass('C',contains='B'); setGeneric('f',function(x)standardGeneric('f')); setMethod('f','A',function(x)'A'); setMethod('f','B',function(x)paste('B',callNextMethod())); setMethod('f','C',function(x)paste('C',callNextMethod())); gctorture(TRUE); on.exit(gctorture(FALSE)); f(new('C')) })"
        ),
        "[1] \"C B A\""
    );
}

#[test]
fn next_method_return_runs_its_cleanup_before_resuming_caller() {
    assert_eq!(
        eval(
            "local({trace<-character();setGeneric('f',function(x)standardGeneric('f'));setMethod('f','ANY',function(x){on.exit({gc();trace<<-c(trace,'inner')});return(5L)});setMethod('f','numeric',function(x){on.exit(trace<<-c(trace,'outer'));value<-callNextMethod();trace<<-c(trace,'resumed');value});value<-f(1);identical(value,5L)&&identical(trace,c('inner','resumed','outer'))})"
        ),
        "[1] TRUE"
    );
}

#[test]
fn s4_next_method_preserves_original_subset_call_and_named_drop() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/s4-next-method-public-contract.R"))
                .unwrap(),
            include_str!("fixtures/s4-next-method-public-contract.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn s4_next_method_fits_browser_limits_after_package_workflows() {
    macro_rules! contract {
        ($name:literal) => {
            include_str!(concat!("fixtures/", $name, ".R"))
        };
    }
    let preceding = [
        contract!("complex-print-public-contract"),
        contract!("stats-namespace-public-contract"),
        contract!("get-lazy-mode-public-contract"),
        contract!("correlation-public-contract"),
        contract!("palette-public-contract"),
        contract!("tempfile-public-contract"),
        contract!("bincode-public-contract"),
        contract!("print-gap-public-contract"),
        contract!("arima0-public-contract"),
        contract!("utils-console-public-contract"),
        contract!("mapply-public-contract"),
        contract!("sample-condition-contract"),
        contract!("rep-len-admission-contract"),
        contract!("calling-error-handler-public-contract"),
        contract!("calling-warning-handler-public-contract"),
    ];
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        session
            .set_resource_limits(RResourceLimits {
                max_eval_depth: 1000,
                max_execution_time_ms: 15000,
                max_alloc_bytes: 64 * 1024 * 1024,
                max_arena_nodes: 500000,
            })
            .unwrap();
        for (phase, code) in preceding.into_iter().enumerate() {
            session
                .eval(code)
                .unwrap_or_else(|error| panic!("portable={portable}; prefix={phase}; {error}"));
        }
        let before = session.arena_stats();
        let started = std::time::Instant::now();
        let result = session
            .eval(include_str!("fixtures/s4-next-method-public-contract.R"))
            .unwrap_or_else(|error| {
                panic!(
                    "portable={portable}; elapsed={:?}; before={before:?}; after={:?}; {error}",
                    started.elapsed(),
                    session.arena_stats()
                )
            });
        println!(
            "warm S4 portable={portable}; elapsed={:?}; before={before:?}; after={:?}",
            started.elapsed(),
            session.arena_stats()
        );
        assert_eq!(
            result,
            include_str!("fixtures/s4-next-method-public-contract.out"),
            "portable={portable}; {:?}",
            session.arena_stats()
        );
        assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
    }
}
