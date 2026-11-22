use r_embed::RSession;

#[test]
fn native_typeconvert_matches_gnu_inference_and_loss_modes() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"convert <- function(x, as.is=TRUE, dec='.', numerals='allow.loss')
  .External2('C_typeconvert', x, 'NA', as.is, dec, numerals, TRUE, PACKAGE='utils')
warn <- ''
rounded <- withCallingHandlers(
  convert(c('9007199254740993','5'), numerals='warn.loss'),
  warning=function(w) { warn <<- conditionMessage(w); invokeRestart('muffleWarning') })
stopifnot(
  identical(convert(c('1','NA','-2')), c(1L,NA_integer_,-2L)),
  identical(convert(c('TRUE','NA','F')), c(TRUE,NA,FALSE)),
  identical(convert(c('-1,25','2,5'), dec=','), c(-1.25,2.5)),
  identical(convert(c('1  2i','2i')), c(1+2i,0+2i)),
  identical(convert(c('42 ','0x1.2.3')), c(42,18.1875)),
  identical(convert(c('9007199254740993','5'), numerals='no.loss'), c('9007199254740993','5')),
  identical(rounded, c(9007199254740992,5)),
  identical(warn, 'accuracy loss in conversion from "9007199254740993" to numeric'),
  identical(convert(c('b','a','b','NA'), as.is=FALSE), factor(c('b','a','b',NA))))
cat('typeconvert-checked: PASS\n')"#
            )
            .unwrap(),
        "typeconvert-checked: PASS\n"
    );
}
