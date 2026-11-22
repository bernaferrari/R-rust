use r_embed::RSession;

#[test]
fn native_writetable_matches_gnu_and_keeps_long_quoted_fields() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"write_native <- function(x, nr, nc, rn=NULL, quote=integer(), escape=FALSE, dec='.') {
  con <- textConnection('out','w',local=TRUE)
  .External2('C_writetable', x, con, nr, nc, rn, ';', '\n', 'missing', dec, quote, escape, PACKAGE='utils')
  close(con)
  out
}
frame <- data.frame(f=factor(c('a"b',NA)),p=c(1.234567890123456,NaN),
                    z=c(1.25+2.5i,3+0i),l=c(TRUE,FALSE))
text <- paste0(strrep('x',8189L), '"tail')
stopifnot(
  identical(write_native(frame,2L,4L,c('1','2'),c(0L,1L),dec=','),
            c('"1";"a""b";1,23456789012346;1,25+2,5i;TRUE',
              '"2";missing;missing;3+0i;FALSE')),
  identical(write_native(matrix(c(1.25,2.5,3.75,4),2L),2L,2L,dec=','),
            c('1,25;3,75','2,5;4')),
  identical(write_native(matrix(text),1L,1L,quote=c(1L,9L)),
            paste0('"',strrep('x',8189L),'""tail"')),
  identical(write_native(matrix(text),1L,1L,quote=c(1L),escape=TRUE),
            paste0('"',strrep('x',8189L),intToUtf8(92L),'"tail"')))
cat('writetable-checked: PASS\n')"#
            )
            .unwrap(),
        "writetable-checked: PASS\n"
    );
}
