use r_embed::RSession;

#[test]
fn edit_grob_dispatches_custom_edit_details_across_class_chain() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval("library(grid); editDetails.grob <- function(x,specs){x$edited <- 'inherited'; x}; editDetails.myGrob <- function(x,specs){x$edited <- specs; x}; g <- structure(list(foo=1,name='x',gp=gpar(),vp=NULL),class=c('myGrob','grob','gDesc')); h <- editGrob(g,gp=gpar(col='red'),foo=2); derived <- structure(g,class=c('derivedGrob','grob','gDesc')); j <- editGrob(derived,foo=3); identical(g$edited,NULL) && identical(h$foo,2) && identical(h$edited$foo,2) && identical(h$gp$col,'red') && identical(j$edited,'inherited')")
        .unwrap();
    assert_eq!(value, "[1] TRUE");
}

#[test]
fn edit_grob_custom_edit_details_works_through_nested_gpath_and_preserves_original() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval("library(grid); editDetails.myGrob <- function(x,specs){x$edited <- specs$foo; x}; leaf <- structure(list(foo=1,name='leaf',gp=gpar(),vp=NULL),class=c('myGrob','grob','gDesc')); g <- grobTree(grobTree(leaf,name='inner'),name='root'); h <- editGrob(g,'leaf',foo=7); identical(g$children$inner$children$leaf$foo,1) && identical(getGrob(h,'leaf')$edited,7)")
        .unwrap();
    assert_eq!(value, "[1] TRUE");
}

#[test]
fn edit_grob_propagates_custom_errors_without_mutating_original() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval("library(grid); editDetails.badGrob <- function(x,specs) stop('custom failure'); g <- structure(list(foo=1,name='x',gp=gpar(),vp=NULL),class=c('badGrob','grob','gDesc')); failed <- tryCatch({editGrob(g,foo=2); FALSE}, error=function(e) grepl('custom failure',conditionMessage(e))); failed && identical(g$foo,1)")
        .unwrap();
    assert_eq!(value, "[1] TRUE");
}

#[test]
fn edit_details_preserves_caller_scope_and_gnu_custom_return_contract() {
    let mut session = RSession::new().unwrap();
    let value = session.eval(r#"
        library(grid)
        f <- function() {
            editDetails.localGrob <- function(x,specs) x$foo + specs$foo
            g <- structure(list(foo=1,name='x',gp=gpar(),vp=NULL),class=c('localGrob','grob','gDesc'))
            editGrob(g,foo=3)
        }
        editDetails.unknownGrob <- function(x,specs) is.null(x$unknown) && identical(specs$unknown,2)
        g <- structure(list(foo=1,name='x',gp=gpar(),vp=NULL),class=c('unknownGrob','grob','gDesc'))
        cat(f(), suppressWarnings(editGrob(g,unknown=2)))
    "#).unwrap();
    assert_eq!(value.trim(), "6 TRUE");
}
