use r_embed::RSession;

#[test]
fn named_zone_parsing_matches_gnu_without_changing_host_timezone() {
    let host_tz = std::env::var_os("TZ");
    let mut session = RSession::new().unwrap();
    // Pinned GNU bac583951b: winter/summer use EST/EDT and isdst 0/1.
    let result = session.eval("x <- strptime(c('1970-01-01 00:00:00','2024-07-01 12:00:00'),'%Y-%m-%d %H:%M:%S',tz='America/New_York'); cat(x$hour,x$isdst,x$zone)").unwrap();
    assert_eq!(result.trim(), "0 12 0 1 EST EDT");
    assert_eq!(std::env::var_os("TZ"), host_tz);
    let utc = session.eval("x <- strptime('1970-01-01 00:00:00','%Y-%m-%d %H:%M:%S',tz='UTC'); cat(x$hour,x$isdst,x$zone)").unwrap();
    assert_eq!(utc.trim(), "0 0 UTC");
    assert_eq!(std::env::var_os("TZ"), host_tz);
}
