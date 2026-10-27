use r_embed::{RSession, RuntimePathPolicy};

fn session() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap()
}

#[test]
fn bundled_native_symbol_info_retains_typed_address_and_arity() {
    let mut session = session();
    let actual = session
        .eval(
            r#"local({
            dll <- getLoadedDLLs()[["methods"]]
            info <- getNativeSymbolInfo("R_identC", PACKAGE=dll, withRegistrationInfo=TRUE)
            x <- "typed symbol"
            cat(typeof(info$address), info$numParameters, .Call(info, x, x), is.object(info), is.object(info$address), "\n")
        })"#,
        )
        .unwrap();
    assert_eq!(actual, "externalptr 2 TRUE TRUE TRUE \n");
}

#[test]
fn native_call_rejects_name_lists_without_authenticated_handles() {
    let mut session = session();
    let error = session
        .eval(r#".Call(list("R_identC"), "typed symbol", "typed symbol", PACKAGE="methods")"#)
        .unwrap_err();
    assert!(
        error.to_string().contains("native symbol reference"),
        "{error}"
    );
}

#[test]
fn native_symbol_authority_ignores_forged_metadata_but_checks_package_and_arity() {
    let mut session = session();
    let actual = session.eval(r#"local({
        dlls <- getLoadedDLLs()
        info <- getNativeSymbolInfo("R_identC", PACKAGE=dlls[["methods"]], withRegistrationInfo=TRUE)
        info[["name"]] <- "C_devcur"
        info[["numParameters"]] <- 0L
        cat(.Call(info,"x","x"), .Call(info$address,"x","x"), .Call(unclass(info),"x","x"), "\n")
        wrong <- tryCatch({.Call(info,"x"); FALSE}, error=function(e) grepl("2", conditionMessage(e), fixed=TRUE))
        package <- tryCatch({getNativeSymbolInfo("R_identC", PACKAGE=dlls[["stats"]]); FALSE}, error=function(e) TRUE)
        forced <- tryCatch({getNativeSymbolInfo("R_identC", PACKAGE="methods"); FALSE}, error=function(e) TRUE)
        cat(wrong, package, forced, .Call(info,"x","x",PACKAGE="does-not-exist"), "\n")
    })"#).unwrap();
    assert_eq!(actual, "TRUE TRUE TRUE \nTRUE TRUE TRUE TRUE \n");
}

#[test]
fn lazy_native_symbol_wrapper_preserves_normal_lookup_formals_and_missing_package() {
    let mut session = session();
    let actual = session.eval(r#"local({
        locked <- bindingIsLocked("getNativeSymbolInfo", baseenv())
        f <- get("getNativeSymbolInfo", baseenv())
        cat(typeof(f), identical(names(formals(f)), c("name","PACKAGE","unlist","withRegistrationInfo")), environmentName(environment(f)), "\n")
        message <- tryCatch({getNativeSymbolInfo("rport_unregistered_symbol"); "unexpected"}, error=function(e) conditionMessage(e))
        cat(grepl("no such symbol", message, fixed=TRUE), identical(locked, bindingIsLocked("getNativeSymbolInfo",baseenv())), "\n")
    })"#).unwrap();
    assert_eq!(actual, "closure TRUE base \nTRUE TRUE \n");
}
