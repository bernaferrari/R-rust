//! GNU R 4.6.1 `switch` NA-character EXPR.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`

use r_embed::RSession;

#[test]
fn switch_na_character_uses_unnamed_default() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(switch(NA_character_, a=1, 2), 2) && identical(withVisible(switch(NA_character_, a=1, 2)), list(value=2, visible=TRUE)) && is.null(switch(NA_character_, a=1))")
            .unwrap()
            .trim(),
        "[1] TRUE",
    );
}
