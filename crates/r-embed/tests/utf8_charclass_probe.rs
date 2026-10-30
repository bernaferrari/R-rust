use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn utf8_charclass_matches_gnu() {
    let mut results = Vec::new();
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let expression = concat!(
            "{ x<-'Aá١²́😀9'; ",
            "err<-function(x,cl) tryCatch(utils::charClass(x,cl), error=conditionMessage); ",
            "identical(utils::charClass(x,'alpha'),c(TRUE,TRUE,TRUE,FALSE,FALSE,FALSE,FALSE)) && ",
            "identical(utils::charClass(x,'digit'),c(FALSE,FALSE,FALSE,FALSE,FALSE,FALSE,TRUE)) && ",
            "identical(utils::charClass(x,'alnum'),c(TRUE,TRUE,TRUE,FALSE,FALSE,FALSE,TRUE)) && ",
            "identical(utils::charClass(c(65L,225L,NA_integer_),'alpha'),c(TRUE,TRUE,NA)) && ",
            "identical(utils::charClass(c(-1L,NA_integer_,1114112L,65L),'alpha'),c(NA,NA,FALSE,TRUE)) && ",
            "identical(utils::charClass(c(65,57),'alpha'),c(TRUE,FALSE)) && ",
            "identical(utils::charClass('','alpha'),logical(0)) && ",
            "identical(utils::charClass(NA_character_,'alpha'),c(TRUE,TRUE)) && ",
            "identical(err('a','bogus'),'character class \\\"bogus\\\" is invalid') && ",
            "identical(err(c('a','b'),'alpha'),\"argument 'x' must be a length-1 character vector\") && ",
            "identical(err('a',character(0)),\"argument 'class' must be a character string\") && ",
            "nchar(err('a',paste(rep('x',1100),collapse='')),type='bytes')==999 }"
        );
        results.push((
            portable,
            session
                .eval(expression)
                .unwrap_or_else(|e| format!("ERROR: {e}")),
        ));
    }
    assert_eq!(
        results,
        vec![
            (false, "[1] TRUE\n".to_string()),
            (true, "[1] TRUE\n".to_string())
        ]
    );
}
