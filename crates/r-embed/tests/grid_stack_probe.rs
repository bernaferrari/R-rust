use r_embed::RSession;
#[test]
fn probe() {
    let path = std::env::var("RPORT_GRID_PROBE_FILE").expect("temporary diagnostic input");
    let code = std::fs::read_to_string(path).unwrap();
    let mut session = RSession::new().unwrap();
    eprintln!("[DEBUG-grid-stack] constructed");
    session.render_with_dimensions(&code, 192, 96).unwrap();
    eprintln!("[DEBUG-grid-stack] rendered");
    eprintln!("{}", session.eval("cat(parity)").unwrap());
}
