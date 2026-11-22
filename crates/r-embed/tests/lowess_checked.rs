use r_embed::RSession;

#[test]
fn lowess_matches_gnu_and_rejects_mismatched_lengths() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"x <- c(0,0,1,2,4,6,8,9)
y <- c(0,1,2,16,4,7,7,9)
want0 <- c(-0.51030887013748938,-0.51030887013748938,7.7448455649312553,16,7.529066533718443,6.1768066526093364,7.8206720789573643,8.585980799247011)
want3 <- c(0.49951904043617423,0.49951904043617423,8.2497595202180865,16,11.5,6.8967822924755531,7.8836262505126209,8.4787811199950482)
stopifnot(max(abs(lowess(x,y,f=.67,iter=0,delta=2)$y-want0)) < 1e-12, max(abs(lowess(x,y,f=.67,iter=3,delta=2)$y-want3)) < 1e-12)
err <- tryCatch(lowess(1:3,1:2),error=function(e) conditionMessage(e))
stopifnot(identical(err,"'x' and 'y' lengths differ"))
cat('lowess-safe: PASS\n')"#
            )
            .unwrap(),
        "lowess-safe: PASS\n"
    );
}

#[test]
fn native_lowess_rejects_mismatched_lengths() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"err <- tryCatch(.Call(stats:::C_lowess, c(1.,2.,3.), c(1.,2.), .67, 0L, 0.), error=function(e) conditionMessage(e))
stopifnot(identical(err,"'x' and 'y' lengths differ"))
cat('lowess-native: PASS\n')"#
            )
            .unwrap(),
        "lowess-native: PASS\n"
    );
}
