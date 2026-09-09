use r_embed::RSession;

#[test]
fn qr_lapack_true_matches_gnu_shape_rank_pivot_and_values() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            "x <- matrix(c(1,2,3,4,5,6), nrow=3, ncol=2); \
             q <- qr(x, LAPACK=TRUE); \
             c(identical(class(q), 'qr'), identical(dim(q$qr), c(3L,2L)), \
               identical(q$rank, 2L), identical(q$pivot, c(2L,1L)), \
               isTRUE(attr(q, 'useLAPACK')), \
               max(abs(as.vector(q$qr) - c(-8.77496438739, 0.391390523557, \
                 0.469668628268, -3.64673844671, -0.837435789359, \
                 0.802528216176))) < 1e-10, \
               max(abs(q$qraux - c(1.45584230584, 1.21650687589))) < 1e-10)",
        )
        .expect("qr(LAPACK=TRUE)");
    assert_eq!(result, "[1] TRUE TRUE TRUE TRUE TRUE TRUE TRUE");
}

#[test]
fn qr_default_linpack_and_lapack_rank_behavior_remain_distinct() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            "x <- matrix(c(1,2,3,2,4,6), nrow=3); \
             q0 <- qr(x); q1 <- qr(x, LAPACK=TRUE); \
             c(identical(q0$rank, 1L), identical(q0$pivot, c(1L,2L)), \
               identical(q1$rank, 2L), identical(q1$pivot, c(2L,1L)))",
        )
        .expect("default and LAPACK QR");
    assert_eq!(result, "[1] TRUE TRUE TRUE TRUE");
}

#[test]
fn qr_lapack_rejects_zero_row_input_with_recoverable_error() {
    let mut session = RSession::new().expect("session");
    let error = session
        .eval("qr(matrix(character(), nrow=0, ncol=2), LAPACK=TRUE)")
        .expect_err("zero-row LAPACK QR must fail");
    assert!(error.to_string().contains("DGEQP3"), "{error}");
    assert_eq!(session.eval("1 + 1").expect("session recovery"), "[1] 2");
}

#[test]
fn qr_handles_wide_empty_vector_and_tolerance_contracts() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
       w<-qr(matrix(1:8,2,4)); z<-qr(matrix(numeric(),0,2)); v<-qr(-1:-3);
       x<-matrix(c(1,0,0,1,1e-8,0),3);
       c(identical(dim(w$qr),c(2L,4L)),length(w$qraux)==4L,w$rank==2L,
         identical(w$pivot,1:4), z$rank==0L,identical(z$pivot,1:2),
         identical(dim(v$qr),c(3L,1L)),v$qr[1,1]>0,
         qr(x)$rank==1L,qr(x,1e-12)$rank==2L,
         qr(LAPACK=FALSE,tol=1e-12,x=x)$rank==2L)
    })",
        )
        .unwrap();
    assert_eq!(
        result.trim(),
        "[1] TRUE TRUE TRUE TRUE TRUE TRUE TRUE TRUE TRUE TRUE TRUE"
    );
}

#[test]
fn qr_observes_time_and_allocation_limits_and_recovers() {
    let mut session = RSession::new().unwrap();
    session.eval("x<-matrix(1,2000,80)").unwrap();
    let mut limits = session.resource_limits();
    limits.max_execution_time_ms = 1;
    session.set_resource_limits(limits).unwrap();
    let error = session.eval("qr(x)").unwrap_err();
    assert!(error.to_string().contains("time limit"), "{error}");
    limits.max_execution_time_ms = 0;
    limits.max_alloc_bytes = 1;
    session.set_resource_limits(limits).unwrap();
    let error = session.eval("qr(x,LAPACK=TRUE)").unwrap_err();
    assert!(error.to_string().contains("allocation"), "{error}");
    limits.max_alloc_bytes = 0;
    session.set_resource_limits(limits).unwrap();
    assert_eq!(
        session.eval("qr(matrix(1:6,3,2))$rank").unwrap().trim(),
        "[1] 2"
    );
}
