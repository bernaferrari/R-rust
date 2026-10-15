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
fn browser_anonymous_files_match_pinned_gnu_update_cursors_and_text_io() {
    let mut session = portable();
    check(
        &mut session,
        include_str!("../../rmath/src/mainutils/connections/file/anonymous-contract.R"),
    );
    assert!(session.list_files().unwrap().is_empty());
}

#[test]
fn browser_anonymous_files_are_session_owned_and_recover_after_limits_and_close() {
    let mut left = portable();
    let mut right = portable();
    check(
        &mut left,
        "{connection<-file('', 'w+b');writeBin(as.raw(c(7,8)),connection);TRUE}",
    );
    check(
        &mut right,
        "{connection<-file('', 'w+b');writeBin(as.raw(3),connection);identical(readBin(connection,'raw',n=8L),as.raw(3))}",
    );
    check(
        &mut left,
        "{seek(connection,1048577,rw='write');message<-tryCatch(writeBin(as.raw(9),connection),error=function(e){gc();conditionMessage(e)});seek(connection,0,rw='read');identical(message,'anonymous file exceeds 1048576 byte limit')&&identical(readBin(connection,'raw',n=8L),as.raw(c(7,8)))}",
    );
    check(
        &mut left,
        "{close(connection);failed<-tryCatch(readBin(connection,'raw',n=1L),error=function(e)TRUE);identical(failed,TRUE)&&identical(1L+1L,2L)}",
    );
    check(
        &mut right,
        "{seek(connection,0,rw='read');identical(readBin(connection,'raw',n=8L),as.raw(3))}",
    );
    check(&mut right, "{close(connection);TRUE}");
    assert!(left.list_files().unwrap().is_empty());
    assert!(right.list_files().unwrap().is_empty());
}
