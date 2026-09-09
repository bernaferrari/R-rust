use r_embed::RSession;

#[test]
fn vector_spearman_matches_gnu_ties_and_missing_modes() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("cor(c(1,1,3), c(2,4,4), method='spearman')")
            .unwrap(),
        "[1] 0.5"
    );
    assert_eq!(
        session
            .eval("is.na(cor(c(1,NA,3), c(1,2,3), method='spearman'))")
            .unwrap(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("cor(c(1,NA,3), c(1,2,3), method='spearman', use='complete.obs')")
            .unwrap(),
        "[1] 1"
    );
    assert_eq!(
        session
            .eval("is.na(cor(c(NA,2), c(1,2), method='spearman', use='na.or.complete'))")
            .unwrap(),
        "[1] TRUE"
    );
}

#[test]
fn vector_spearman_matches_gnu_edge_cases_and_rejections() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("suppressWarnings(is.na(cor(c(1,1), c(2,3), method='spearman')))")
            .unwrap(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("cor(c(1,Inf,3), c(1,2,3), method='spearman')")
            .unwrap(),
        "[1] 0.5"
    );
    assert!(
        session
            .eval("cor(matrix(1:4, nrow=2), method='spearman')")
            .is_err()
    );
    assert!(
        session
            .eval("cor(c(1,2), c(1,2), method='kendall')")
            .is_err()
    );
    assert!(
        session
            .eval("cor(c(1,2), c(1), method='spearman')")
            .is_err()
    );
    assert!(
        session
            .eval("cor(c(NA,NA), c(1,2), method='spearman', use='all.obs')")
            .is_err()
    );
    assert!(
        session
            .eval("cor(c(NA,NA), c(1,2), method='spearman', use='complete.obs')")
            .is_err()
    );
    assert_eq!(
        session
            .eval("is.na(cor(c(NA,NA), c(1,2), method='spearman', use='na.or.complete'))")
            .unwrap(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("cor(c(-0,0,1), c(1,1,2), method='spearman')")
            .unwrap(),
        "[1] 1"
    );
}

#[test]
fn constant_rank_vectors_signal_gnu_zero_variance_warning() {
    let mut session = RSession::new().unwrap();
    let value = session.eval("message <- ''; value <- withCallingHandlers(cor(c(1,1), c(2,3), method='spearman'), warning=function(e) { message <<- conditionMessage(e); invokeRestart('muffleWarning') }); identical(message,'the standard deviation is zero') && is.na(value)").unwrap();
    assert_eq!(value, "[1] TRUE");
    assert_eq!(
        session
            .eval("is.na(cor(1,2,method='spearman',use='complete.obs'))")
            .unwrap(),
        "[1] TRUE"
    );
}
