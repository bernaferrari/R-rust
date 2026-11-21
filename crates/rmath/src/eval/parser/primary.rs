use super::*;

impl<'arena, 'session> Parser<'arena, 'session> {
    // -----------------------------------------------------------------------
    // Primary expressions (atoms and control structures)
    // -----------------------------------------------------------------------

    pub(crate) fn parse_primary(&mut self) -> Result<SEXP, ParseError> {
        match self.peek().clone() {
            // Keywords
            Token::KwIf => self.parse_if(),
            Token::KwFor => self.parse_for(),
            Token::KwWhile => self.parse_while(),
            Token::KwRepeat => self.parse_repeat(),
            Token::KwFunction | Token::KwLambda => self.parse_function(),
            Token::KwBreak => {
                self.advance();
                {
                    let sym = crate::sexp::symbol::install_str("break");
                    self.lang2(sym, R_NilValue())
                }
            }
            Token::KwNext => {
                self.advance();
                {
                    let sym = crate::sexp::symbol::install_str("next");
                    self.lang2(sym, R_NilValue())
                }
            }
            // Block: { expr; expr; ... }
            Token::LBrace => self.parse_block(),
            // Parenthesized expr or tuple
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    // Empty parens → NULL
                    self.advance();
                    return Ok(R_NilValue());
                }
                let expr = self.parse_expr()?;
                self.skip_newlines();
                self.expect(&Token::RParen)?;
                {
                    // Parentheses are part of R's language object, not merely
                    // parser grouping.  In particular, evaluating the `(`
                    // primitive makes its value visible, so `(x <- 1)` is
                    // auto-printed even though the inner assignment is not.
                    let sym = crate::sexp::symbol::install_str("(");
                    self.lang2(sym, expr)
                }
            }
            _ => self.parse_atom(),
        }
    }
}
