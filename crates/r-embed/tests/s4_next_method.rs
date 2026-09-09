use r_embed::RSession;

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
