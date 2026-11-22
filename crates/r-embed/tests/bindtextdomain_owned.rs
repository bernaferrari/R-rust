#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
#[test]
fn binding_updates_are_local_to_the_r_session() {
    use r_embed::RSession;

    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"d <- 'rport-owned-binding'
stopifnot(
  identical(bindtextdomain(NULL),TRUE),
  is.null(bindtextdomain('')),
  identical(bindtextdomain(d,'/rport/first'),'/rport/first'),
  identical(bindtextdomain(d),'/rport/first'),
  identical(bindtextdomain(d,'/rport/second'),'/rport/second'),
  identical(bindtextdomain(d),'/rport/second'),
  identical(gettext('rport text',domain=d),'rport text'),
  identical(ngettext(2,'one','many',domain=d),'many'))
cat('bound: PASS\n')"#
            )
            .unwrap(),
        "bound: PASS\n"
    );
    session.close();

    let mut fresh = RSession::new().unwrap();
    assert_eq!(
        fresh.eval("cat(bindtextdomain('rport-owned-binding'))").unwrap(),
        "/"
    );
}
