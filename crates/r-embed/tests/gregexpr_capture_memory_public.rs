use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn gregexpr_capture_attribution_stays_charged_through_r_result_construction() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session.eval("p<-strrep('()',50L);subject<-strrep('a',10000L);invisible(gregexpr(p,'a',perl=TRUE));gc();invisible(NULL)").unwrap();
    let before = session.memory_usage().unwrap();
    let hidden_and_result_minimum = 10_000
        * (50 * std::mem::size_of::<Option<(usize, usize)>>()
            + 2 * std::mem::size_of::<usize>()
            + 102 * std::mem::size_of::<i32>());
    let headroom = hidden_and_result_minimum - 512 * 1024;
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + u64::try_from(headroom).unwrap();
    session.set_resource_limits(limits).unwrap();
    let error = session
        .eval("invisible(gregexpr(p,subject,perl=TRUE))")
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("R allocation failed: memory or node budget exceeded"),
        "{message}"
    );
    assert_eq!(
        session.resource_limits().max_alloc_bytes,
        limits.max_alloc_bytes
    );
    assert_eq!(session.eval("nchar(subject)").unwrap(), "[1] 10000\n");
    assert_eq!(session.eval("nchar(p)").unwrap(), "[1] 100\n");
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
}

#[test]
fn gregexpr_capture_matrices_survive_collection_during_publication() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let source = r#"local({gctorture(TRUE);on.exit(gctorture(FALSE));z<-gregexpr('(?<g>a)',c('a','zaa',NA_character_),perl=TRUE);identical(as.integer(z[[1L]]),1L)&&identical(as.integer(attr(z[[1L]],'capture.start')),1L)&&identical(as.integer(z[[2L]]),c(2L,3L))&&identical(as.integer(attr(z[[2L]],'capture.length')),c(1L,1L))&&is.na(z[[3L]][[1L]])})"#;
    assert_eq!(session.eval(source).unwrap(), "[1] TRUE\n");
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn gregexpr_capture_metadata_matches_gnu_for_utf8_and_unset_groups() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!(
                "fixtures/gregexpr-capture-attribution-contract.R"
            ))
            .unwrap(),
        include_str!("fixtures/gregexpr-capture-attribution-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}
