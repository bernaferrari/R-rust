#[test]
fn probe_one() {
    let case = std::env::var("PROBE_CASE").unwrap_or_else(|_| "500_findinterval_options".to_string());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance");
    let source = std::fs::read_to_string(root.join(format!("cases/{case}.R"))).expect("case source");
    let golden = std::fs::read_to_string(root.join(format!("golden/{case}.out"))).unwrap_or_default();
    let mut session = r_runtime::value::session::Session::new();
    match session.eval_script(&source) {
        Ok(output) => {
            std::fs::write("/tmp/probe_one.txt", format!("---GOT---\n{output}\n---WANT---\n{golden}")).unwrap();
        }
        Err(e) => {
            std::fs::write("/tmp/probe_one.txt", format!("ERROR\n{e:?}\n---WANT---\n{golden}")).unwrap();
        }
    }
}
