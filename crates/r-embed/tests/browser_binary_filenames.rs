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
