use r_embed::RSession;
#[test]
fn loess_runs_through_formula_dispatch_and_predict() {
    let mut s = RSession::new().unwrap();
    let answer=s.eval("x <- seq(0,1,length.out=15); y <- sin(5*x)+x^2; f <- loess(y~x); c(round(f$fitted[1],8),round(predict(f)[1],8))").unwrap();
    assert_eq!(answer, "[1] -0.02619684 -0.02619684\n");
    assert_eq!(s.eval("class(f)").unwrap(), "[1] \"loess\"\n");
}

#[test]
fn loess_data_weights_subset_model_and_matrix_predictors() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("d<-data.frame(x=seq(0,1,length.out=30)); d$y<-sin(d$x*5); d$w<-1+d$x; f<-loess(y~x,data=d,weights=w,subset=c(30:1,1),model=TRUE); c(f$n,nrow(f$model),length(predict(f,newdata=data.frame(x=c(.2,.5)))))").unwrap(), "[1] 31 31  2\n");
    assert_eq!(s.eval("class(f$model)").unwrap(), "[1] \"data.frame\"\n");
    assert_eq!(
        s.eval("f$x[1,1] == 1 && f$x[31,1] == 0").unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(s.eval("z<-cbind(d$x,cos(d$x*3)); g<-loess(y~z,data=d,span=1); length(predict(g,newdata=data.frame(z=I(z))))").unwrap(), "[1] 30\n");
}

#[test]
fn loess_errors_recover_and_mutated_models_are_validated() {
    let mut s = RSession::new().unwrap();
    s.eval("x<-seq(0,1,length.out=30);y<-sin(x*5);f<-loess(y~x)")
        .unwrap();
    for code in ["loess(y~x,span=0)", "loess(y~x,parametric=TRUE)"] {
        assert!(s.eval(code).is_err(), "{code}");
        assert_eq!(s.eval("1+1").unwrap(), "[1] 2\n");
    }
    assert_eq!(
        s.eval("p<-predict(f);f$pars$span<-NaN;f$divisor<-numeric(0);identical(p,predict(f))")
            .unwrap(),
        "[1] TRUE\n"
    );
}

#[test]
fn portable_loess_cached_fitted_values_and_numerical_admission() {
    let mut s = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_eq!(s.eval("x<-seq(0,1,length.out=30);y<-sin(x*5);f<-loess(y~x);p<-predict(f);f$pars$span<-NaN;f$divisor<-numeric(0);identical(p,predict(f))").unwrap(), "[1] TRUE\n");
    for code in [
        "f<-loess(y~x);f$pars$span<-NaN;predict(f,newdata=.2)",
        "f<-loess(y~x);f$divisor<-numeric(0);predict(f,newdata=.2)",
    ] {
        assert!(s.eval(code).is_err(), "{code}");
        assert_eq!(s.eval("1+1").unwrap(), "[1] 2\n");
    }
}

#[test]
fn loess_none_statistics_matches_gnu_under_both_policies() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
                Vec::new(),
                std::env::temp_dir(),
            ))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(s.eval(r#"identical(tryCatch(predict.loess(list(fitted=1L)),error=function(e)conditionMessage(e)), 'first argument must be a "loess" object')"#).unwrap(), "[1] TRUE\n");
        for surface in ["direct", "interpolate"] {
            let code = format!(
                "x<-seq(0,1,length.out=15);y<-sin(5*x)+x^2;f<-loess(y~x,control=loess.control(surface='{surface}',statistics='none'));identical(c(f$trace.hat,f$one.delta,f$two.delta,is.infinite(f$s)),c(0,0,0,1)) && identical(predict(f),f$fitted)"
            );
            assert_eq!(
                s.eval(&code).unwrap(),
                "[1] TRUE\n",
                "portable={portable},surface={surface}"
            );
        }
    }
}

#[test]
fn loess_exact_prediction_uncertainty_and_none_statistics() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("x<-seq(0,1,length.out=15); y<-sin(5*x)+x^2; f<-loess(y~x,control=loess.control(surface='direct',statistics='exact')); p<-predict(f,newdata=c(.03,.27),se=TRUE); round(p$se.fit,8)").unwrap(), "[1] 0.02951868 0.02180607\n");
    assert_eq!(s.eval("g<-loess(y~x,control=loess.control(statistics='none')); c(g$trace.hat,g$one.delta,g$two.delta,is.infinite(g$s))").unwrap(), "[1] 0 0 0 1\n");
}

#[test]
fn loess_objects_survive_collection_and_serialization() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("gctorture(TRUE); x<-seq(0,1,length.out=15); y<-sin(5*x)+x^2; f<-loess(y~x,model=TRUE); g<-unserialize(serialize(f,NULL)); invisible(gc()); p<-predict(g,newdata=c(.2,.5),se=TRUE); gctorture(FALSE); c(length(p$fit),nrow(g$model),all(is.finite(p$se.fit)))").unwrap(), "[1]  2 15  1\n");
}

#[test]
fn loess_workspace_error_preserves_the_session() {
    let mut s = RSession::new().unwrap();
    let error = s.eval("x<-seq(0,1,length.out=5000); y<-sin(x); loess(y~x,control=loess.control(surface='direct'))").unwrap_err();
    assert!(
        error.to_string().contains("LOESS workspace limit exceeded"),
        "{error}"
    );
    assert_eq!(s.eval("2+2").unwrap(), "[1] 4\n");
}

#[test]
fn loess_can_be_cancelled_without_poisoning_the_session() {
    use r_embed::CancellationToken;
    use std::{sync::mpsc, thread, time::Duration};
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut s = RSession::new().unwrap();
        s.eval("x<-seq(0,1,length.out=1500);y<-sin(x*5)").unwrap();
        ready_tx.send(()).unwrap();
        let result = s.eval_result_cancellable(
            "loess(y~x,control=loess.control(surface='direct',statistics='exact'))",
            &worker_token,
        );
        result_tx
            .send((result.err().map(|e| e.to_string()), s.eval("2+2").unwrap()))
            .unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    thread::sleep(Duration::from_millis(50));
    token.cancel();
    let (error, recovered) = result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("LOESS must observe cancellation during numerical work");
    assert!(error.unwrap().contains("operation cancelled"));
    assert_eq!(recovered, "[1] 4\n");
    worker.join().unwrap();
}

#[test]
fn unavailable_loess_predictions_are_na_not_nan() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("x<-seq(0,1,length.out=30);y<-sin(x);f<-loess(y~x);p<-predict(f,c(NA,NaN,Inf,-1),se=TRUE);c(all(is.na(p$fit)),any(is.nan(p$fit)),all(is.na(p$se.fit)),any(is.nan(p$se.fit)))").unwrap(), "[1]  TRUE FALSE  TRUE FALSE\n");
}
