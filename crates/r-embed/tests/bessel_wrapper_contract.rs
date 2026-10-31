use r_embed::{RSession, RuntimePathPolicy};

const CONTROLS: &str = r#"
for (name in c('besselI','besselJ','besselK','besselY')) {
    f <- get(name)
    stopifnot(identical(typeof(f), 'closure'))
    stopifnot(identical(f(nu=0,x=1),f(1,0)), identical(f(n=0,x=1),f(1,0)))
    error <- tryCatch(f(x=1,x=2,nu=0),error=function(e) conditionMessage(e))
    stopifnot(identical(error,'formal argument "x" matched by multiple actual arguments'))
}
as.logical.bessel_wrapper_probe <- function(x,...) { seen <<- deparse(substitute(x)); TRUE }
seen <- NULL
z <- besselI(nu=0,x=1,expon.scaled=structure(1,class='bessel_wrapper_probe'))
stopifnot(identical(seen,'structure(1, class = "bessel_wrapper_probe")'),abs(z-besselI(1,0,TRUE))<1e-14)
internal_j <- function() .Internal(besselJ(-1,0))
internal_i <- function() .Internal(besselI(-1,0,1))
for (name in c("internal_j", "internal_i")) {
    calls <- list()
    value <- withCallingHandlers(
        if (name == "internal_j") internal_j() else internal_i(),
        warning=function(w) {calls[[length(calls)+1L]] <<- conditionCall(w);invokeRestart("muffleWarning")}
    )
    stopifnot(is.nan(value),length(calls) == 2L, all(vapply(calls, function(call) identical(call, as.call(list(as.name(name)))), TRUE)))
}
cat('bessel-wrapper-contract-ok\n')
"#;

#[test]
fn public_bessel_wrappers() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = session.eval(CONTROLS).unwrap();
        assert!(out.contains("bessel-wrapper-contract-ok"), "{out}");
    }
}

// The primitive generic frame is omitted by current S3 dispatch: rport-vlmeo.
#[test]
#[ignore = "rport-vlmeo: primitive generic call frame is missing"]
fn scaling_method_sees_gnu_primitive_generic_frame() {
    let mut session = RSession::new().unwrap();
    session.eval(r#"
        as.logical.bessel_stack_probe <- function(x,...) {
            calls_seen <<- tail(vapply(sys.calls(), function(call) paste(deparse(call),collapse=''), ''), 2)
            TRUE
        }
        besselI(1,0,structure(1,class='bessel_stack_probe'))
        stopifnot(identical(calls_seen,c('as.logical(expon.scaled)','as.logical.bessel_stack_probe(expon.scaled)')))
    "#).unwrap();
}

#[test]
fn primitive_alias_warning_attribution_when_upstream_disabled() {
    if std::env::var_os("RPORT_UPSTREAM_BASE").as_deref() != Some(std::ffi::OsStr::new("0")) {
        return;
    }
    let mut session = RSession::new().unwrap();
    session
        .eval(
            r#"
        f <- besselJ
        calls <- list()
        value <- withCallingHandlers(f(1,500),warning=function(w) {
            calls[[length(calls)+1L]] <<- conditionCall(w)
            invokeRestart('muffleWarning')
        })
        stopifnot(identical(value,0),length(calls)==1L,identical(calls[[1]],quote(f(1,500))))
    "#,
        )
        .unwrap();
}
