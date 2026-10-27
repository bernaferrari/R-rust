use r_embed::RSession;

#[test]
fn s4_environment_dollar_assignment_updates_xdata() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ methods::setClass('NlenzBox', contains='environment'); x <- methods::new('NlenzBox'); x$slot_value <- 303L; slot <- methods::slot(x, '.xData'); found <- exists('slot_value', envir=slot, inherits=FALSE); paste(is.environment(slot), found, if (found) get('slot_value', envir=slot) else -1L) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] \"TRUE TRUE 303\"");
}

#[test]
fn ordinary_environment_dollar_assignment_is_unchanged() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ e <- new.env(); e$x <- 7L; paste(exists('x', e, inherits=FALSE), get('x', e), is.environment(e)) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] \"TRUE 7 TRUE\"");
}

#[test]
fn explicit_s4_environment_dollar_method_still_dispatches() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ methods::setClass('NlenzMethodBox', contains='environment'); called <- FALSE; methods::setMethod('$<-', 'NlenzMethodBox', function(x, name, value) { called <<- TRUE; gc(); methods::slot(x, '.xData')[[name]] <- value; x }); z <- methods::new('NlenzMethodBox'); z$a <- 9L; slot <- methods::slot(z, '.xData'); found <- exists('a', slot, inherits=FALSE); paste(called, found, if (found) get('a', slot) else -1L, is.environment(slot)) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] \"TRUE TRUE 9 TRUE\"");
}

#[test]
fn inherited_s4_environment_dollar_method_dispatches() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ methods::setClass('NlenzDollarParent', contains='environment'); methods::setClass('NlenzDollarChild', contains='NlenzDollarParent'); called <- FALSE; methods::setMethod('$<-', 'NlenzDollarParent', function(x,name,value) { called <<- TRUE; gc(); methods::slot(x, '.xData')[[name]] <- 31L; x }); z <- methods::new('NlenzDollarChild'); z$a <- 1L; paste(called, get('a', methods::slot(z, '.xData'))) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] \"TRUE 31\"");
}

#[test]
fn multi_signature_s4_bracket_method_dispatches() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ methods::setClass('NlenzMultiParent', contains='numeric'); methods::setClass('NlenzMultiChild', contains='NlenzMultiParent'); methods::setMethod('[', signature(x='NlenzMultiParent', i='numeric'), function(x,i,...,drop=TRUE) { gc(); 73L }); x <- methods::new('NlenzMultiChild', .Data=1:3); x[1] })")
        .unwrap();
    assert_eq!(result.trim(), "[1] 73");
}

#[test]
fn user_any_method_for_custom_s4_generic_dispatches() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ methods::setClass('NlenzAnyClass', slots=c(value='numeric')); methods::setGeneric('NlenzAnyGeneric', function(x) standardGeneric('NlenzAnyGeneric')); methods::setMethod('NlenzAnyGeneric', 'ANY', function(x) { gc(); 47L }); NlenzAnyGeneric(methods::new('NlenzAnyClass', value=1)) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] 47");
}

#[test]
fn sealed_primitive_any_method_redefinition_errors_like_gnu() {
    let error = RSession::new()
        .unwrap()
        .eval("methods::setMethod('$<-', 'ANY', function(x,name,value) x)")
        .expect_err("GNU seals the primitive `$<-` ANY method");
    assert!(error.to_string().contains("sealed"), "unexpected error: {error}");
}

#[test]
fn s3_dollar_replacement_keeps_precedence() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ z <- structure(list(), class='NlenzS3'); called <- FALSE; `$<-.NlenzS3` <- function(x,name,value) { called <<- TRUE; x[[name]] <- value; x }; z$a <- 5L; paste(called, z$a) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] \"TRUE 5\"");
}

#[test]
fn s4_bracket_method_dispatches_through_shared_primitive_path() {
    let result = RSession::new()
        .unwrap()
        .eval("local({ methods::setClass('NlenzBracketBox', contains='numeric'); called <- FALSE; methods::setMethod('[', 'NlenzBracketBox', function(x,i,...,drop=TRUE) { called <<- TRUE; gc(); 99L }); z <- methods::new('NlenzBracketBox', .Data=1:3); value <- z[1]; paste(called, methods::hasMethod('[', 'NlenzBracketBox'), value) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] \"TRUE TRUE 99\"");
}

#[test]
fn s4_primitive_methods_remain_owner_local_across_session_switches() {
    let mut first = RSession::new().unwrap();
    let mut second = RSession::new().unwrap();
    let define = |session: &mut RSession, method_value: i32| {
        let code = format!(
            "local({{ methods::setClass('NlenzOwnerBox', contains='environment'); methods::setMethod('$<-', 'NlenzOwnerBox', function(x,name,value) {{ methods::slot(x, '.xData')[[name]] <- {method_value}L; x }}); z <- methods::new('NlenzOwnerBox'); z$a <- 1L; get('a', methods::slot(z, '.xData')) }})"
        );
        session.eval(&code).unwrap()
    };
    assert_eq!(define(&mut first, 101).trim(), "[1] 101");
    assert_eq!(define(&mut second, 202).trim(), "[1] 202");
    let first_again = first
        .eval("z <- methods::new('NlenzOwnerBox'); z$a <- 303L; get('a', methods::slot(z, '.xData'))")
        .unwrap();
    assert_eq!(first_again.trim(), "[1] 101");
}
