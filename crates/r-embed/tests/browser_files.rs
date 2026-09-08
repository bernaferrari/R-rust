use r_embed::RSession;
#[test]
fn browser_files_standard_io_and_isolation() {
    let mut s = RSession::new().unwrap();
    s.import_file("input.csv", b"x,y\n1,2\n").unwrap();
    assert_eq!(
        s.eval("d <- read.csv('input.csv'); d$x[1]+d$y[1]").unwrap(),
        "[1] 3"
    );
    s.import_file("script.R", b"answer <- 42\n").unwrap();
    s.eval("source('script.R'); answer").unwrap();
    s.eval("writeLines('saved','output.txt')").unwrap();
    assert_eq!(s.export_file("output.txt").unwrap(), b"saved\n");
    s.eval("writeLines('replaced','input.csv')").unwrap();
    assert_eq!(s.export_file("input.csv").unwrap(), b"replaced\n");
    s.eval("con <- file('output.txt', 'a'); writeLines('more', con); close(con)")
        .unwrap();
    assert_eq!(s.export_file("output.txt").unwrap(), b"saved\nmore\n");
    let mut fresh = RSession::new().unwrap();
    assert!(fresh.export_file("output.txt").is_err());
}
#[test]
fn browser_file_limits() {
    let mut s = RSession::new().unwrap();
    assert!(s.import_file("../x", b"x").is_err());
    assert!(s.import_file("x", &vec![0; 1024 * 1024 + 1]).is_err());
}

#[test]
fn browser_mode_creates_files_without_import_and_opens_deferred_connections() {
    let mut s = RSession::new().unwrap();
    s.enable_browser_files();
    s.eval("writeLines('first', 'created.txt')").unwrap();
    assert_eq!(s.export_file("created.txt").unwrap(), b"first\n");
    s.eval("con <- file('created.txt', 'w'); close(con)")
        .unwrap();
    assert_eq!(s.export_file("created.txt").unwrap(), b"");
    s.eval("con <- file('later.txt'); open(con, 'w'); writeLines('later', con); close(con)")
        .unwrap();
    assert_eq!(s.export_file("later.txt").unwrap(), b"later\n");
    assert_eq!(
        s.eval("con <- file('later.txt', 'r'); x <- readLines(con); close(con); x")
            .unwrap(),
        "[1] \"later\""
    );
    assert!(s.eval("file('missing', 'r+')").is_err());
    assert!(s.eval("readLines('/etc/hosts')").is_err());
}
