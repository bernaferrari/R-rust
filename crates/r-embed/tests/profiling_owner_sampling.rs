#![cfg(not(target_arch = "wasm32"))]

use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn elapsed_rprof_automatically_samples_the_owning_workload() {
    let path = std::env::temp_dir().join(format!("rport-owner-profile-{}.out", std::process::id()));
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let quoted = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = format!(
        r#"
        Rprof("{quoted}", FALSE, 0.001, FALSE, FALSE, FALSE, FALSE, 100L, 10500L, "elapsed")
        sampled_owner_workload <- function(n) {{
            x <- 0
            for(i in seq_len(n)) x <- x + i
            x
        }}
        sampled_owner_workload(20000L)
        Rprof("", FALSE, 0.001, FALSE, FALSE, FALSE, FALSE, 100L, 10500L, "elapsed")
    "#
    );
    session.eval(&source).unwrap();
    let profile = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(profile.contains("sample.interval=1000"), "{profile}");
    assert!(
        profile
            .lines()
            .skip(1)
            .any(|line| line.contains("sampled_owner_workload")),
        "Rprof must sample the evaluated workload without a manual R_WriteProfile call: {profile}"
    );
}

#[test]
fn original_utils_rprof_registration_preserves_start_and_stop_options() {
    let path = std::env::temp_dir().join(format!(
        "rport-registration-profile-{}.out",
        std::process::id()
    ));
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let quoted = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let result = session.eval(&format!(r#"local({{
        info <- getNativeSymbolInfo("Rprof", PACKAGE=getLoadedDLLs()[["utils"]], withRegistrationInfo=TRUE)
        rejected <- tryCatch({{ .External(get("C_Rprof", asNamespace("utils"))); FALSE }}, error=function(e) grepl("10", conditionMessage(e), fixed=TRUE))
        cat(info$numParameters, inherits(info,"ExternalRoutine"), typeof(get("C_Rprof", asNamespace("utils"))), rejected, "\n")
        utils::Rprof("{quoted}", interval=0.001, event="elapsed")
        utils::Rprof(NULL)
    }})"#)).unwrap();
    assert_eq!(result, "10 TRUE externalptr TRUE \n");
    let profile = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    assert!(profile.contains("sample.interval=1000"), "{profile}");
}

#[test]
fn cpu_rprof_samples_cpu_work_and_stop_restart_keeps_files_separate() {
    let first = std::env::temp_dir().join(format!("rport-cpu-profile-{}.out", std::process::id()));
    let second =
        std::env::temp_dir().join(format!("rport-restart-profile-{}.out", std::process::id()));
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session.eval(&format!(r#"
        cpu_profile_workload <- function(n) {{ x <- 0; for(i in seq_len(n)) x <- x + i; invisible(x) }}
        utils::Rprof("{}", interval=0.001, event="cpu")
        cpu_profile_workload(20000L)
        utils::Rprof(NULL)
    "#, first.display())).unwrap();
    let before = std::fs::read(&first).unwrap();
    assert!(String::from_utf8_lossy(&before).contains("cpu_profile_workload"));
    session.eval("cpu_profile_workload(20000L)").unwrap();
    assert_eq!(
        std::fs::read(&first).unwrap(),
        before,
        "stop must disable automatic sampling"
    );
    session
        .eval(&format!(
            r#"
        utils::Rprof("{}", interval=0.002, event="elapsed")
        cpu_profile_workload(20000L)
        utils::Rprof(NULL)
    "#,
            second.display()
        ))
        .unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), before);
    let restarted = std::fs::read_to_string(&second).unwrap();
    assert!(restarted.contains("sample.interval=2000"));
    assert!(restarted.contains("cpu_profile_workload"));
    let _ = std::fs::remove_file(first);
    let _ = std::fs::remove_file(second);
}

#[test]
fn profiling_nested_sessions_keep_their_workload_frames_separate() {
    let left_path =
        std::env::temp_dir().join(format!("rport-profile-left-{}.out", std::process::id()));
    let right_path =
        std::env::temp_dir().join(format!("rport-profile-right-{}.out", std::process::id()));
    let mut left =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let mut right =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    left.eval(&format!(r#"
        left_profile_workload <- function(n) {{ x <- 0; for(i in seq_len(n)) x <- x + i; invisible(x) }}
        utils::Rprof("{}", interval=0.001, event="elapsed")
        left_profile_workload(20000L)
    "#, left_path.display())).unwrap();
    right.eval(&format!(r#"
        right_profile_workload <- function(n) {{ x <- 0; for(i in seq_len(n)) x <- x + i; invisible(x) }}
        utils::Rprof("{}", interval=0.001, event="elapsed")
        right_profile_workload(20000L)
        utils::Rprof(NULL)
    "#, right_path.display())).unwrap();
    left.eval("left_profile_workload(20000L); utils::Rprof(NULL)")
        .unwrap();
    let left_profile = std::fs::read_to_string(&left_path).unwrap();
    let right_profile = std::fs::read_to_string(&right_path).unwrap();
    assert!(left_profile.contains("left_profile_workload"));
    assert!(!left_profile.contains("right_profile_workload"));
    assert!(right_profile.contains("right_profile_workload"));
    assert!(!right_profile.contains("left_profile_workload"));
    let _ = std::fs::remove_file(left_path);
    let _ = std::fs::remove_file(right_path);
}

#[test]
fn original_rprof_settings_preserve_filter_memory_gc_and_line_options() {
    let path =
        std::env::temp_dir().join(format!("rport-settings-profile-{}.out", std::process::id()));
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session.eval(&format!(r#"
        settings_profile_workload <- function(n) {{ x <- 0; for(i in seq_len(n)) x <- x + i; invisible(x) }}
        utils::Rprof("{}", interval=0.001, event="elapsed", memory.profiling=TRUE,
                     gc.profiling=TRUE, line.profiling=TRUE, filter.callframes=TRUE)
        settings_profile_workload(20000L)
        utils::Rprof(NULL)
    "#, path.display())).unwrap();
    let profile = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    assert!(
        profile
            .starts_with("memory profiling: GC profiling: line profiling: sample.interval=1000\n"),
        "{profile}"
    );
    assert!(profile.contains("settings_profile_workload"), "{profile}");
}
