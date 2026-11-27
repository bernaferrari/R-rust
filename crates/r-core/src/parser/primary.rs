use super::*;

impl<'h, 'i> Parser<'h, 'i> {
    // -----------------------------------------------------------------------
    // Primary expressions (atoms and control structures)
    // -----------------------------------------------------------------------

    pub fn parse_primary(&mut self) -> Result<ValueId, ParseError> {
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
                    let sym = self.install_symbol("break");
                    self.lang2(sym, ValueId::NIL)
                }
            }
            Token::KwNext => {
                self.advance();
                {
                    let sym = self.install_symbol("next");
                    self.lang2(sym, ValueId::NIL)
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
                    return Ok(ValueId::NIL);
                }
                let expr = self.parse_expr()?;
                self.skip_newlines();
                self.expect(&Token::RParen)?;
                {
                    // Parentheses are part of R's language object, not merely
                    // parser grouping.  In particular, evaluating the `(`
                    // primitive makes its value visible, so `(x <- 1)` is
                    // auto-printed even though the inner assignment is not.
                    let sym = self.install_symbol("(");
                    self.lang2(sym, expr)
                }
            }
            _ => self.parse_atom(),
        }
    }
}
