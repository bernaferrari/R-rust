//! Global-environment enumeration contract.
//!
//! GNU R documents the unsorted `ls()` / `as.list(.GlobalEnv)` enumeration
//! order as arbitrary: it depends on the internal hash-table state, which
//! tracks first-bind times (and rehash growth), so rebound loop variables can
//! sit at a different slot than in a build whose insertion history differs.
//! The specified observables are:
//!
//!   1. the association set — every bound name maps to the right value;
//!   2. `ls(sorted = TRUE)` — locale/C-ordered, deterministic;
//!   3. binding kinds — active vs. ordinary per name;
//!   4. unsorted enumeration contains *exactly* the bound names
//!      (order-insensitive set comparison, no duplicates, no omissions).
//!
//! The goldens below are byte-identical to the pinned oracle build
//! (r90451) running the same scripts; see docs/unspecified-observables.md
//! for the classification of the datetime3 exact-build divergence.
use crate::sexp::session::RSession;

/// Builds a representative `.GlobalEnv` history: plain assignments, loop
/// rebinds (same name rebound many times), a rebound growing vector, an
/// `rm()` + re-add, and an active binding.
const HISTORY: &str = r#"
a <- 1L
b <- "beta"
`z-dash` <- 3.5
for (i in 1:3) counter <- i * 2L
keep <- NULL; for (k in c("x","y","z")) keep <- c(keep, k)
rmv <- 10L; rm(rmv); rmv <- "readded"
late <- TRUE
makeActiveBinding("ab", function() 42L, .GlobalEnv)
"#;

/// Probes run inside `local()` so their own helper bindings never leak into
/// the enumeration under test.
const LS_PROBE: &str = r#"
probe <- local({
  n <- ls(.GlobalEnv, sorted = TRUE)
  u <- ls(.GlobalEnv, sorted = FALSE)
  cat("sorted:", paste(n, collapse = ","), "\n")
  for (nm in n) {
    cat(nm, "|", bindingIsActive(nm, .GlobalEnv), "|",
        paste(as.list(.GlobalEnv)[[nm]], collapse = "/"), "\n")
  }
  cat("setequal:", setequal(u, n), "\n")
  cat("dup:", anyDuplicated(u), " len:", length(u), "\n")
})
"#;

const AS_LIST_PROBE: &str = r#"
probe <- local({
  n <- ls(.GlobalEnv, sorted = TRUE)
  al <- as.list(.GlobalEnv)
  cat("aslist-set:", setequal(names(al), n), "\n")
  cat("aslist-dup:", anyDuplicated(names(al)), "\n")
  cat("aslist-counter:", al$counter, "\n")
  cat("aslist-ab:", al$ab, "\n")
  cat("aslist-rmv:", al$rmv, "\n")
})
"#;

#[test]
fn global_env_enumeration_specified_observables_match_oracle() {
    let mut session = RSession::new();
    let (result, output, _visible) =
        session.eval_script_with_output_capture(&format!("{HISTORY}{LS_PROBE}"));
    assert!(
        result.is_ok(),
        "diagnostic script must evaluate cleanly; stderr={:?}",
        output.stderr
    );
    // Trailing spaces after each cat value match oracle Rscript byte-for-byte.
    assert_eq!(
        output.stdout,
        "sorted: a,ab,b,counter,i,k,keep,late,rmv,z-dash \n\
         a | FALSE | 1 \n\
         ab | TRUE | 42 \n\
         b | FALSE | beta \n\
         counter | FALSE | 6 \n\
         i | FALSE | 3 \n\
         k | FALSE | z \n\
         keep | FALSE | x/y/z \n\
         late | FALSE | TRUE \n\
         rmv | FALSE | readded \n\
         z-dash | FALSE | 3.5 \n\
         setequal: TRUE \n\
         dup: 0  len: 10 \n",
        "sorted listing, name->value association set, binding kinds, and \
         order-insensitive unsorted membership must match the oracle build"
    );
}

/// The unsorted listing must be an exact name set when read through
/// `as.list(.GlobalEnv)` — the observable datetime3 prints — with the right
/// values and binding kinds for rebound loop vars, active bindings, and
/// `rm()` + re-add.
#[test]
fn global_env_unsorted_as_list_is_exact_name_set() {
    let mut session = RSession::new();
    let (result, output, _visible) =
        session.eval_script_with_output_capture(&format!("{HISTORY}{AS_LIST_PROBE}"));
    assert!(result.is_ok(), "stderr={:?}", output.stderr);
    let stdout = output.stdout;
    assert!(
        stdout.ends_with(
            "aslist-set: TRUE \naslist-dup: 0 \naslist-counter: 6 \n\
             aslist-ab: 42 \naslist-rmv: readded \n"
        ),
        "as.list(.GlobalEnv) must contain exactly the bound names with the right values and kinds; stdout={stdout:?}"
    );
}
