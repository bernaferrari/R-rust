use r_embed::RSession;

const MODEL_FRAME_TS_SCRIPT: &str = r##"
    x1 <- ts(c(1:10, NA))
    y1 <- ts(rnorm(11))
    x1_before <- x1
    y1_before <- y1
    mf <- model.frame(y1 ~ x1)
    stopifnot(nrow(mf) == 10L)
    stopifnot(identical(class(mf$x1), "integer"))
    stopifnot(identical(class(mf$y1), "numeric"))
    stopifnot(is.null(attr(mf$x1, "tsp")), is.null(attr(mf$y1, "tsp")))
    stopifnot(identical(x1, x1_before), identical(y1, y1_before))
    fit <- lm(y1 ~ x1)
    stopifnot(nobs(fit) == 10L)

    custom_ts <- structure(ts(1:3), class=c("foo", "ts"))
    response <- 1:3
    custom_frame <- model.frame(response ~ custom_ts)
    stopifnot(identical(class(custom_frame$custom_ts), "foo"))
    stopifnot(is.null(attr(custom_frame$custom_ts, "tsp")))
    stopifnot(identical(class(custom_ts), c("foo", "ts")))
    stopifnot(!is.null(attr(custom_ts, "tsp")))

    factor_value <- factor(c("a", "b", "a"))
    date_value <- as.Date("2020-01-01") + 0:2
    ordinary_frame <- model.frame(response ~ factor_value + date_value)
    stopifnot(identical(class(ordinary_frame$factor_value), "factor"))
    stopifnot(identical(levels(ordinary_frame$factor_value), levels(factor_value)))
    stopifnot(identical(class(ordinary_frame$date_value), "Date"))

    extra_ts <- ts(4:6)
    extra_before <- extra_ts
    dot_frame <- model.frame(response ~ factor_value,
        data=list(response=response, factor_value=factor_value), extra=extra_ts)
    stopifnot("(extra)" %in% names(dot_frame))
    stopifnot(is.null(attr(dot_frame[["(extra)"]], "tsp")))
    stopifnot(!("ts" %in% class(dot_frame[["(extra)"]])))
    stopifnot(identical(extra_ts, extra_before))
    cat("model-frame-ts-attributes: PASS\n")
    "##;

fn assert_model_frame_ts_attributes(session: &mut RSession) {
    assert_eq!(
        session.eval(MODEL_FRAME_TS_SCRIPT).unwrap(),
        "model-frame-ts-attributes: PASS\n"
    );
}

#[test]
fn native_model_frame_drops_ts_metadata_without_mutating_inputs() {
    let mut session = RSession::new().unwrap();
    assert_model_frame_ts_attributes(&mut session);
}

#[test]
fn portable_model_frame_drops_ts_metadata_without_mutating_inputs() {
    let mut session = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_model_frame_ts_attributes(&mut session);
}
