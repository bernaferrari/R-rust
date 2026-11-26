//! Regression contracts for workspace save/load round-trips and `$` on NULL.

#[test]
fn save_image_roundtrips_global_workspace() {
    let mut session = r_embed::RSession::new().unwrap();
    let path = format!(
        "{}/rport-wstest-{}.RData",
        std::env::temp_dir().display(),
        std::process::id()
    );
    std::fs::remove_file(&path).ok();
    // `.GlobalEnv` inside base-namespace closures (save.image) must resolve the
    // live session workspace, not the upstream loader's captured staging copy.
    let code = format!(
        "zz_ws_x <- 1:3; zz_ws_env <- new.env(); \
         save.image(file='{path}'); \
         e <- new.env(); nm <- load('{path}', envir=e); \
         paste(paste(sort(nm), collapse=','), \
               identical(e$zz_ws_x, 1:3), is.environment(e$zz_ws_env))"
    );
    let result = session.eval(&code).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(
        result.contains("zz_ws_env,zz_ws_x TRUE TRUE"),
        "unexpected roundtrip: {result}"
    );
}

#[test]
fn dollar_on_null_returns_null() {
    let mut session = r_embed::RSession::new().unwrap();
    let result = session
        .eval("paste(is.null(NULL$abc), is.null(NULL$version))")
        .unwrap();
    assert!(result.contains("TRUE TRUE"), "unexpected: {result}");
}

#[test]
fn save_list_roundtrips_multiple_objects() {
    let mut session = r_embed::RSession::new().unwrap();
    let path = format!(
        "{}/rport-wstest-multi-{}.RData",
        std::env::temp_dir().display(),
        std::process::id()
    );
    std::fs::remove_file(&path).ok();
    let code = format!(
        "zz_ws_a <- 1:3; zz_ws_b <- new.env(); \
         save(list=c('zz_ws_a','zz_ws_b'), file='{path}'); \
         rm(zz_ws_a, zz_ws_b); nm <- load('{path}'); \
         paste(paste(sort(nm), collapse=','), identical(zz_ws_a, 1:3))"
    );
    let result = session.eval(&code).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(
        result.contains("zz_ws_a,zz_ws_b TRUE"),
        "unexpected roundtrip: {result}"
    );
}
