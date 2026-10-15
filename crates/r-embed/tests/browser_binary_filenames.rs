//! GNU filename shorthand through the real public per-session file backend.
use r_embed::{RSession, RValue, RuntimePathPolicy};

fn portable() -> RSession {
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        .expect("portable base session");
    session.enable_browser_files();
    session
}

fn check(session: &mut RSession, program: &str) {
    assert_eq!(
        session.eval_result(program).expect(program).value,
        RValue::Logical(Some(true))
    );
}

#[test]
fn browser_binary_filename_read_uses_original_session_store_and_recovers() {
    let mut left = portable();
    let mut right = portable();
    left.import_file("binary/input.bin", &[0, 1, 127, 128, 255, 0])
        .unwrap();
    right.import_file("binary/input.bin", &[3, 4]).unwrap();
    check(
        &mut left,
        "{con<-file('binary/input.bin','rb');x<-readBin(con,'raw',n=6L);close(con);identical(x,as.raw(c(0,1,127,128,255,0)))}",
    );
    check(
        &mut left,
        "identical(readBin('binary/input.bin','raw',n=6L),as.raw(c(0,1,127,128,255,0)))",
    );
    check(
        &mut left,
        "identical(readBin('binary/input.bin','integer',n=3L,size=2L,signed=FALSE,endian='little'),c(256L,32895L,255L))",
    );
    check(
        &mut right,
        "identical(readBin('binary/input.bin','raw',n=8L),as.raw(c(3,4)))",
    );
    check(
        &mut left,
        "identical(readBin(as.raw(c(4,5,6)),'raw',n=2L),as.raw(c(4,5)))",
    );
    check(
        &mut left,
        "identical(tryCatch(readBin(character(),'raw'),error=function(e)conditionMessage(e)),\"invalid 'description' argument\")&&identical(tryCatch(readBin(NA_character_,'raw'),error=function(e)conditionMessage(e)),\"invalid 'description' argument\")",
    );
    assert!(
        left.eval("readBin('binary/missing.bin','raw',n=1L)")
            .is_err()
    );
    check(
        &mut left,
        "identical(readBin('binary/input.bin','raw',n=0L),raw())&&identical(1L+1L,2L)",
    );
}

#[test]
fn browser_binary_filename_write_is_invisible_and_stays_in_original_store() {
    let mut left = portable();
    let mut right = portable();
    check(
        &mut left,
        "{x<-withVisible(writeBin(as.raw(c(0,1,255)),'binary/output.bin'));is.null(x$value)&&!x$visible}",
    );
    assert_eq!(left.export_file("binary/output.bin").unwrap(), [0, 1, 255]);
    assert!(right.export_file("binary/output.bin").is_err());
    check(
        &mut left,
        "{writeBin(c(1L,256L),'binary/output.bin',size=2L,endian='big');identical(readBin('binary/output.bin','integer',n=2L,size=2L,endian='big'),c(1L,256L))}",
    );
    assert_eq!(left.export_file("binary/output.bin").unwrap(), [0, 1, 1, 0]);
    assert!(left.eval("writeBin(as.raw(1),'../outside.bin')").is_err());
    check(
        &mut left,
        "identical(readBin('binary/output.bin','raw',n=4L),as.raw(c(0,1,1,0)))&&identical(1L+1L,2L)",
    );
}

#[test]
fn browser_binary_filename_write_matches_gnu_validation_and_truncation_stages() {
    let mut session = portable();
    // Independently confirmed against oracle/r-oracle.json (GNU r90451).
    // Wrapper failures precede opening. Internal failures follow truncation.
    for (operation, message, remaining) in [
        (
            "writeBin(1L,'binary/output.bin',endian='bad')",
            "invalid 'endian' argument",
            2,
        ),
        (
            "writeBin(list(1L),'binary/output.bin')",
            "can only write vector objects",
            2,
        ),
        (
            "writeBin(structure(1L,x=2L),'binary/output.bin')",
            "can only write vector objects",
            2,
        ),
        (
            "writeBin(1L,'binary/output.bin',size=3L)",
            "size 3 is unknown on this machine",
            0,
        ),
        (
            "writeBin(as.raw(1),'binary/output.bin',size=2L)",
            "size changing is not supported for raw vectors",
            0,
        ),
        (
            "writeBin(1+2i,'binary/output.bin',size=8L)",
            "size changing is not supported for complex vectors",
            0,
        ),
        (
            "writeBin(1L,'binary/output.bin',useBytes=NA)",
            "invalid 'useBytes' argument",
            0,
        ),
        (
            "(function(){old<-options(warn=2);on.exit(options(old));writeBin(1L,'binary/output.bin',size='x',useBytes=NA)})()",
            "(converted from warning) NAs introduced by coercion",
            0,
        ),
        (
            "(function(){old<-options(warn=2);on.exit(options(old));writeBin('a','binary/output.bin',size='x')})()",
            "(converted from warning) NAs introduced by coercion",
            0,
        ),
    ] {
        session.import_file("binary/output.bin", &[7, 8]).unwrap();
        check(
            &mut session,
            &format!(
                "identical(tryCatch({operation},error=function(e){{gc();conditionMessage(e)}}),\"{message}\")"
            ),
        );
        assert_eq!(
            session.export_file("binary/output.bin").unwrap().len(),
            remaining,
            "{operation}"
        );
        check(&mut session, "identical(1L+1L,2L)");
    }
    check(
        &mut session,
        "{writeBin(integer(),'binary/output.bin',size=3L);identical(readBin('binary/output.bin','raw',n=8L),raw())}",
    );
    check(
        &mut session,
        "{writeBin('a','binary/output.bin',size=3L);identical(readBin('binary/output.bin','raw',n=8L),as.raw(c(97,0)))}",
    );
    check(
        &mut session,
        "identical(tryCatch(writeBin(1L,raw(),size=3L),error=function(e)conditionMessage(e)), 'size 3 is unknown on this machine')",
    );
    check(
        &mut session,
        "{writeBin(as.raw(c(7,8)),'binary/output.bin');con<-file('binary/output.bin','ab');message<-tryCatch(writeBin(1L,con,size=3L),error=function(e)conditionMessage(e));close(con);identical(message,'size 3 is unknown on this machine')&&identical(readBin('binary/output.bin','raw',n=8L),as.raw(c(7,8)))}",
    );
    check(
        &mut session,
        "{con<-file('binary/output.bin','w');message<-tryCatch(readBin(con,'raw',n=1L),error=function(e)conditionMessage(e));close(con);identical(message,'can only read from a binary connection')}",
    );
    check(
        &mut session,
        "{con<-file('binary/output.bin','w');message<-tryCatch(writeBin(1L,con,size=3L),error=function(e)conditionMessage(e));close(con);identical(message,'can only write to a binary connection')}",
    );
    check(
        &mut session,
        "{x<-withVisible(writeBin(as.raw(c(1,2)),raw()));x$visible&&identical(x$value,as.raw(c(1,2)))}",
    );
    check(
        &mut session,
        "{writeBin(c('a','b'),'binary/output.bin');con<-file('binary/output.bin','rb');zero<-readBin(con,'character',n=0L);first<-readBin(con,'character',n=1L);second<-readBin(con,'character',n=1L);last<-readBin(con,'character',n=1L);close(con);identical(zero,character())&&identical(first,'a')&&identical(second,'b')&&identical(last,character())}",
    );
}
