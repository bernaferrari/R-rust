use r_embed::RSession;
#[test]
fn browser_files_standard_io_and_isolation() {
    let mut s = RSession::new().unwrap();
    s.import_file("input.csv", b"x,y\n1,2\n").unwrap();
    assert_eq!(
        s.eval("d <- read.csv('input.csv'); d$x[1]+d$y[1]")
            .unwrap()
            .trim(),
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
    assert_eq!(
        s.eval(
            "Sys.setFileTime('created.txt', as.POSIXct(1700000000, origin = '1970-01-01', tz = 'UTC')) && file.info('created.txt')$size == 6 && as.numeric(file.mtime('created.txt')) == 1700000000 && file.exists('created.txt') && file.access('created.txt', 0) == 0 && is.na(file.info('missing.txt')$size)",
        )
        .unwrap()
        .trim(),
        "[1] TRUE"
    );
    s.eval("con <- file('created.txt', 'w'); close(con)")
        .unwrap();
    assert_eq!(s.export_file("created.txt").unwrap(), b"");
    s.eval("con <- file('later.txt'); open(con, 'w'); writeLines('later', con); close(con)")
        .unwrap();
    assert_eq!(s.export_file("later.txt").unwrap(), b"later\n");
    assert_eq!(
        s.eval("con <- file('later.txt', 'r'); x <- readLines(con); close(con); x")
            .unwrap()
            .trim(),
        "[1] \"later\""
    );
    assert!(s.eval("file('missing', 'r+')").is_err());
    assert!(s.eval("readLines('/etc/hosts')").is_err());

    let probe =
        std::env::temp_dir().join(format!("rport-browser-host-probe-{}", std::process::id()));
    std::fs::write(&probe, b"HOSTSECRET").unwrap();
    struct DeleteOnDrop(std::path::PathBuf);
    impl Drop for DeleteOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _probe = DeleteOnDrop(probe.clone());
    let probe_r = probe.to_string_lossy().replace('\\', "/");
    let temp_r = std::env::temp_dir()
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .replace('\\', "/");
    assert!(
        !probe_r.contains('\'') && !temp_r.contains('\''),
        "host probe paths must be single-quote free"
    );
    assert_eq!(
        s.eval(&format!(
            "identical(list.files(), c('created.txt', 'later.txt')) && identical(list.files(character(0)), character(0)) && identical(list.files('missing-virtual'), character(0)) && identical(list.files('{temp_r}'), character(0))"
        ))
        .unwrap()
        .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        s.eval(&format!(
            "file.copy('later.txt', 'copied.txt') && identical(readLines('copied.txt'), 'later') && identical(file.copy('{probe_r}', 'nope.txt'), FALSE) && !file.exists('nope.txt')"
        ))
        .unwrap()
        .trim(),
        "[1] TRUE"
    );
    assert!(!std::path::Path::new("nope.txt").exists());
    let shown = s
        .eval(".Internal(file.show('later.txt', '', '', FALSE, ''))")
        .unwrap();
    assert!(
        shown.contains("later\n"),
        "file.show did not read store bytes: {shown:?}"
    );
    let missing_show = s
        .eval(&format!(
            ".Internal(file.show('{probe_r}', '', '', TRUE, ''))"
        ))
        .unwrap();
    assert!(
        missing_show.contains("Cannot open file"),
        "missing file.show fell through: {missing_show:?}"
    );
    assert!(
        !missing_show.contains("HOSTSECRET"),
        "file.show read the host: {missing_show:?}"
    );
    assert_eq!(std::fs::read(&probe).unwrap(), b"HOSTSECRET");
    assert_eq!(
        s.eval(&format!(
            "identical(unlink('copied.txt'), TRUE) && !file.exists('copied.txt') && identical(unlink('copied.txt'), FALSE) && identical(unlink('{probe_r}', recursive = TRUE), FALSE) && identical(unlink('missing-virtual', recursive = TRUE), FALSE)"
        ))
        .unwrap()
        .trim(),
        "[1] TRUE"
    );
    assert_eq!(std::fs::read(&probe).unwrap(), b"HOSTSECRET");
    assert_eq!(
        s.eval(
            "writeLines('nested', 'dir/child.txt'); identical(list.files('dir'), character(0)) && 'dir/child.txt' %in% list.files() && identical(readLines('dir/child.txt'), 'nested') && identical(unlink('dir', recursive = TRUE), FALSE) && identical(readLines('dir/child.txt'), 'nested') && identical(unlink('dir/child.txt'), TRUE) && identical(unlink(c('later.txt', 'no-such')), c(TRUE, FALSE))",
        )
        .unwrap()
        .trim(),
        "[1] TRUE"
    );
    assert!(s.export_file("later.txt").is_err());
    assert_eq!(
        s.eval(
            "is.null(.Internal(file.show('created.txt', '', '', TRUE, ''))) && !file.exists('created.txt') && !('created.txt' %in% list.files())",
        )
        .unwrap()
        .trim(),
        "[1] TRUE"
    );
    assert!(s.export_file("created.txt").is_err());
    assert_eq!(std::fs::read(&probe).unwrap(), b"HOSTSECRET");
}
