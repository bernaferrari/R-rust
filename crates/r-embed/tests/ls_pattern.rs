//! `ls(pattern=)` filters like GNU base, and an unbound `name` warns
//! before the search-list lookup.

use r_embed::RSession;

#[test]
fn ls_pattern_filters_names_and_warns_for_non_environment() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
            e <- new.env(parent = emptyenv())
            e$alpha <- 1
            e$attr_two <- 2
            e$beta <- 3
            tiny <- ls(e, pattern = "^a")
            tiny_envir <- ls(envir = e, pattern = "^a")
            ns <- asNamespace("methods")
            all_ns <- ls(ns)
            fil <- ls(ns, pattern = "^attr")
            empty <- ls(pattern = "^not_a_real_name_zzz$")
            base_names <- ls(baseenv())
            top <- ls()
            warns <- character()
            err <- tryCatch(
                withCallingHandlers(
                    ls(not_a_symbol_zzz),
                    warning = function(w) {
                        warns <<- c(warns, conditionMessage(w))
                        invokeRestart("muffleWarning")
                    }
                ),
                error = function(e) conditionMessage(e)
            )
            cat(paste(
                paste0("KIND=", typeof(ls)),
                paste0("TINY=", identical(tiny, c("alpha", "attr_two"))),
                paste0("TINY_ENV=", identical(tiny_envir, c("alpha", "attr_two"))),
                paste0(
                    "FIL_OK=",
                    is.character(fil) &&
                        all(grepl("^attr", fil)) &&
                        identical(fil, grep("^attr", all_ns, value = TRUE)) &&
                        (!("attr" %in% all_ns) || ("attr" %in% fil))
                ),
                paste0("EMPTY=", identical(empty, character(0))),
                paste0(
                    "BASE=",
                    is.character(base_names) &&
                        isTRUE(!is.unsorted(base_names)) &&
                        all(c("c", "mean", "ls") %in% base_names)
                ),
                paste0("TOP=", is.character(top) && isTRUE(!is.unsorted(top))),
                paste0("WMSG=", paste(warns, collapse = " || ")),
                paste0("ERR=", if (is.character(err)) err else "NA"),
                paste0("BODY=", typeof(body(ls))),
                sep = "\n"
            ))
            "#,
        )
        .unwrap_or_else(|err| panic!("ls pattern eval failed: {err}"));
    let report = result.trim();
    assert!(
        report.lines().any(|line| line == "KIND=closure"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "TINY=TRUE"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "TINY_ENV=TRUE"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "FIL_OK=TRUE"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "EMPTY=TRUE"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "BASE=TRUE"),
        "{report}"
    );
    assert!(report.lines().any(|line| line == "TOP=TRUE"), "{report}");
    let warning = report
        .lines()
        .find(|line| line.starts_with("WMSG="))
        .unwrap_or("");
    assert!(
        warning.contains("converted to character string") && warning.contains("not_a_symbol_zzz"),
        "{report}"
    );
    let err = report
        .lines()
        .find(|line| line.starts_with("ERR="))
        .unwrap_or("");
    assert!(
        err.contains("no item called \"not_a_symbol_zzz\" on the search list"),
        "{report}"
    );
}
