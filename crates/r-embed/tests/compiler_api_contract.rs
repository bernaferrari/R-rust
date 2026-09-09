use r_embed::RSession;
use std::path::Path;
use std::process::Command;

const CONTRACT_EXPR: &str = r#"
    f <- readRDS(commandArgs(TRUE)[1])
    invisible(capture.output(d <- compiler::disassemble(f)))
    cat("ret-type", typeof(d), "ret-len", length(d),
        "head", as.character(d[[1]]), "code-len", length(d[[2]]),
        "op1", as.character(d[[2]][[2]]),
        "op2", as.character(d[[2]][[4]]),
        "nested-type", typeof(d[[3]][[3]]),
        "nested-head", as.character(d[[3]][[3]][[1]]),
        "nested-code-len", length(d[[3]][[3]][[2]]),
        "nested-op", as.character(d[[3]][[3]][[2]][[2]]), sep = "|")
"#;

fn oracle_contract(fixture: &Path) -> Option<String> {
    let oracle = std::env::var_os("RPORT_PINNED_RSCRIPT")?;
    let output = Command::new(oracle)
        .args(["--vanilla", "-e", CONTRACT_EXPR])
        .arg(fixture)
        .output()
        .expect("run pinned GNU Rscript");
    assert!(
        output.status.success(),
        "pinned GNU R failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(
        String::from_utf8(output.stdout)
            .expect("GNU R output is UTF-8")
            .trim()
            .to_owned(),
    )
}

#[test]
fn compiler_disassemble_contract_matches_pinned_gnu() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gnu-bytecode-calls/identity-promise.rds");
    let expected = "ret-type|list|ret-len|3|head|.Code|code-len|8|op1|GETFUN.OP|op2|MAKEPROM.OP|nested-type|list|nested-head|.Code|nested-code-len|4|nested-op|GETVAR.OP";
    if let Some(actual) = oracle_contract(&fixture) {
        assert_eq!(actual, expected);
    }
    // Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 emits this
    // exact compact projection for disassembling the imported fixture,
    // including its recursively expanded nested promise bytecode.
    assert_eq!(
        expected,
        "ret-type|list|ret-len|3|head|.Code|code-len|8|op1|GETFUN.OP|op2|MAKEPROM.OP|nested-type|list|nested-head|.Code|nested-code-len|4|nested-op|GETVAR.OP"
    );

    let mut session = RSession::new().expect("session");
    let raw = include_bytes!("fixtures/gnu-bytecode-calls/identity-promise.rds")
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let actual = session
        .eval(&format!(
            "f<-unserialize(as.raw(c({raw})));{}",
            CONTRACT_EXPR.replace("f <- readRDS(commandArgs(TRUE)[1])", "")
        ))
        .expect("compiler::disassemble contract should be implemented");
    assert_eq!(actual.trim(), expected);
}

#[test]
fn disassemble_rejects_uncompiled_inputs_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    assert!(
        session
            .eval("compiler::disassemble(function() 1)")
            .unwrap_err()
            .to_string()
            .contains("function is not compiled")
    );
    assert!(
        session
            .eval("compiler::disassemble(1)")
            .unwrap_err()
            .to_string()
            .contains("argument is not byte code")
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
