use r_embed::{RSession, RuntimePathPolicy};

fn initialization_keeps_globals_private(portable: bool) {
    let mut session = if portable {
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
    } else {
        RSession::new()
    }
    .unwrap();
    assert_eq!(
        session
            .eval("identical(ls(envir=.GlobalEnv,all.names=TRUE),character())")
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("f<-'user function'; env<-42L; tab<-list(user=TRUE); library(stats); identical(f,'user function') && identical(env,42L) && identical(tab,list(user=TRUE)) && identical(as.numeric(diff(ts(1:5))),rep(1,4))").unwrap(), "[1] TRUE\n");
}

#[test]
fn default_namespace_initialization_keeps_globals_private() {
    initialization_keeps_globals_private(false);
}

#[test]
fn portable_namespace_initialization_keeps_globals_private() {
    initialization_keeps_globals_private(true);
}

#[test]
fn require_preserves_startup_conditions_quietness_and_repeated_attachment() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/require-startup-public-contract.R"))
                .unwrap(),
            "[1] TRUE\n",
            "portable={portable}"
        );
    }
}
