use r_embed::{RSession, RuntimePathPolicy};

// Exact IEEE-754 fixtures, independently checked with pinned GNU R r90451.
const CONTROLS: &str = r#"
from_bytes <- function(x) readBin(as.raw(x), "double", n=1L, size=8L, endian="little")
quiet_na <- from_bytes(c(0xa2,7,0,0,0,0,0xf8,0x7f))
nan1 <- from_bytes(c(1,0,0,0,0,0,0xf8,0x7f))
nan2 <- from_bytes(c(2,0,0,0,0,0,0xf8,0x7f))
negative_quiet_na <- from_bytes(c(0xa2,7,0,0,0,0,0xf8,0xff))
negative_nan1 <- from_bytes(c(1,0,0,0,0,0,0xf8,0xff))
positive_zero <- from_bytes(rep(0,8))
negative_zero <- from_bytes(c(rep(0,7),0x80))
stopifnot(is.na(quiet_na), !is.nan(quiet_na), is.nan(nan1), is.nan(nan2),
          identical(writeBin(quiet_na, raw(), size=8L, endian="little"), as.raw(c(0xa2,7,0,0,0,0,0xf8,0x7f))))
for (num in c(TRUE, FALSE)) {
    for (single in c(TRUE, FALSE)) {
        stopifnot(identical(identical(NA_real_, quiet_na, num.eq=num, single.NA=single), single),
                  identical(identical(quiet_na, NA_real_, num.eq=num, single.NA=single), single),
                  identical(identical(quiet_na, negative_quiet_na, num.eq=num, single.NA=single), single),
                  identical(identical(nan1, negative_nan1, num.eq=num, single.NA=single), single),
                  !identical(quiet_na, nan1, num.eq=num, single.NA=single),
                  !identical(nan1, quiet_na, num.eq=num, single.NA=single),
                  identical(identical(nan1, nan2, num.eq=num, single.NA=single), single),
                  identical(nan1, nan1, num.eq=num, single.NA=single),
                  identical(identical(positive_zero, negative_zero, num.eq=num, single.NA=single), num))
    }
}
stopifnot(identical(c(1,quiet_na,2), c(1,NA_real_,2)),
          identical(list(quiet_na), list(NA_real_)),
          !identical(c(1,quiet_na,2), c(1,NA_real_,2), single.NA=FALSE),
          !identical(quiet_na, Inf), !identical(quiet_na, -Inf))
y <- approx(1:3, c(10, NA, 30), xout=c(1,1.5,2,2.5,3), na.rm=FALSE)$y
stopifnot(identical(y, c(10, NA_real_, NA_real_, NA_real_, 30)))
cat("identical-numeric-na-contract-ok\n")
"#;

#[test]
fn public_numeric_identical_contract_matches_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("identical-numeric-na-contract-ok"),
            "{output}"
        );
    }
}
