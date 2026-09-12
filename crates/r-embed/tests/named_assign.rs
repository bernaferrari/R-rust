use r_embed::RSession;

fn eval_pair(session: &mut RSession, code: &str) -> String {
    session
        .eval(code)
        .unwrap_or_else(|err| panic!("{code} failed: {err}"))
        .trim()
        .trim_start_matches("[1]")
        .trim()
        .trim_matches('"')
        .to_string()
}

#[test]
fn colon_subassign_does_not_mutate_other_binding() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        eval_pair(
            &mut session,
            "x <- 1:3; y <- x; x[1] <- 9L; paste(c(paste(y,collapse='-'), paste(x,collapse='-')), collapse='|')",
        ),
        "1-2-3|9-2-3"
    );
}

#[test]
fn materialized_integer_subassign_does_not_mutate_other_binding() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        eval_pair(
            &mut session,
            "x <- c(1L,2L,3L); y <- x; x[1] <- 9L; paste(c(paste(y,collapse='-'), paste(x,collapse='-')), collapse='|')",
        ),
        "1-2-3|9-2-3"
    );
}

#[test]
fn names_subassign_does_not_mutate_other_binding() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        eval_pair(
            &mut session,
            "x <- 1:3; names(x) <- c('a','b','c'); y <- x; names(x)[1] <- 'z'; paste(c(paste(names(y),collapse='-'), paste(names(x),collapse='-')), collapse='|')",
        ),
        "a-b-c|z-b-c"
    );
}

#[test]
fn real_rhs_and_multi_index_subassign_use_default_cow_path() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        eval_pair(
            &mut session,
            "x <- 1:3; y <- x; x[1] <- 9; paste(c(paste(y,collapse='-'), paste(x,collapse='-')), collapse='|')",
        ),
        "1-2-3|9-2-3"
    );
    assert_eq!(
        eval_pair(
            &mut session,
            "x <- 1:3; y <- x; x[c(1L,2L)] <- c(9L,8L); paste(c(paste(y,collapse='-'), paste(x,collapse='-')), collapse='|')",
        ),
        "1-2-3|9-8-3"
    );
}
