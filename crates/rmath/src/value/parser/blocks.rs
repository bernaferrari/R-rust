use super::*;

impl<'h, 'i> Parser<'h, 'i> {
    // -----------------------------------------------------------------------
    // Blocks
    // -----------------------------------------------------------------------

    /// { expr; expr; ... }
    pub(crate) fn parse_block(&mut self) -> Result<ValueId, ParseError> {
        let lbrace_span = self.spans.get(self.pos).copied().unwrap_or((0, 0));
        self.advance(); // consume '{'
        self.skip_terminators();

        let mut exprs = Vec::new();
        let mut stmt_spans = Vec::new();
        while self.peek() != &Token::RBrace && self.peek() != &Token::Eof {
            let stmt_start = self
                .spans
                .get(self.pos)
                .map(|&(a, _)| a)
                .unwrap_or(lbrace_span.0);
            let expr = self.parse_expr()?;
            let stmt_end = if self.pos > 0 {
                self.spans
                    .get(self.pos - 1)
                    .map(|&(_, b)| b)
                    .unwrap_or(stmt_start)
            } else {
                stmt_start
            };
            exprs.push(expr);
            stmt_spans.push((stmt_start, stmt_end));
            self.skip_terminators();
        }
        self.expect(&Token::RBrace)?;

        let call = if exprs.is_empty() {
            {
                let brace_sym = self.install_symbol("{");
                self.lang2(brace_sym, ValueId::NIL)?
            }
        } else if exprs.len() == 1 {
            {
                let brace_sym = self.install_symbol("{");
                self.lang2(brace_sym, exprs.pop().unwrap_or(ValueId::NIL))?
            }
        } else {
            {
                let brace_sym = self.install_symbol("{");
                let nil = ValueId::NIL;
                let mut list = self.cons(exprs.pop().unwrap_or(nil), nil)?;
                while let Some(e) = exprs.pop() {
                    list = self.cons(e, list)?;
                }
                self.lang_cons(brace_sym, list)?
            }
        };

        Ok(call)
    }

    // -----------------------------------------------------------------------
    // Atoms
    // -----------------------------------------------------------------------

    pub(crate) fn parse_atom(&mut self) -> Result<ValueId, ParseError> {
        // An operand position never terminates on a newline (gram.l's
        // EatLines): `1 +\n2` continues on the next line and `1 +\n` runs
        // out of input, exactly like upstream.
        self.skip_newlines();
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                self.scalar_real(n)
            }
            Token::Complex(n) => {
                self.advance();
                self.scalar_complex(n)
            }
            Token::Int(n) => {
                self.advance();
                self.scalar_integer(n)
            }
            Token::Str(s) => {
                self.advance();
                self.scalar_string(&s)
            }
            Token::StrBytes(bytes) => {
                self.advance();
                self.scalar_bytes(&bytes)
            }
            Token::DotDotDot => {
                self.advance();
                {
                    let sym = self.install_symbol("...");
                    Ok(sym)
                }
            }
            Token::Placeholder => {
                self.advance();
                self.placeholder_node()
            }
            Token::Ident(ref name) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "TRUE" => self.scalar_logical(1),
                    "FALSE" => self.scalar_logical(0),

                    "NULL" => Ok(ValueId::NIL),
                    "NA" => self.scalar_logical(crate::value::object::NA_INTEGER),
                    "Inf" => self.scalar_real(f64::INFINITY),
                    "NaN" => self.scalar_real(f64::NAN),
                    "NA_real_" => self.scalar_real(f64::from_bits(crate::value::object::NA_REAL_BITS)),
                    "NA_integer_" => self.scalar_integer(crate::value::object::NA_INTEGER),
                    "NA_complex_" => self
                        .scalar_complex_parts(f64::from_bits(crate::value::object::NA_REAL_BITS), f64::from_bits(crate::value::object::NA_REAL_BITS)),
                    "NA_character_" => self.scalar_na_string(),
                    _ => Ok(self.install_symbol(&name)),
                }
            }
            _ => Err(self.unexpected_at(self.pos)),
        }
    }
    // -----------------------------------------------------------------------

    pub(crate) fn parse_arglist(&mut self) -> Result<Vec<(Option<String>, ValueId)>, ParseError> {
        let mut args = Vec::new();
        self.skip_newlines();
        if self.peek() == &Token::RParen {
            return Ok(args);
        }

        loop {
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                args.push((None, ValueId::MISSING));
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    args.push((None, ValueId::MISSING));
                    break;
                }
                continue;
            }
            let (name, val) = self.parse_arg()?;
            args.push((name, val));
            self.skip_newlines();

            if self.peek() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    args.push((None, ValueId::MISSING));
                    break;
                }
            } else {
                break;
            }
        }

        Ok(args)
    }

    pub(crate) fn parse_arg(&mut self) -> Result<(Option<String>, ValueId), ParseError> {
        match self.peek().clone() {
            Token::Ident(name) | Token::Str(name) => {
                let saved = self.pos;
                let name = name.clone();
                self.advance();
                match self.peek() {
                    Token::Assign => {
                        self.advance();
                        self.skip_newlines();
                        let val = if self.peek() == &Token::Comma || self.peek() == &Token::RParen {
                            ValueId::MISSING
                        } else {
                            self.parse_expr()?
                        };
                        Ok((Some(name), val))
                    }
                    _ => {
                        self.pos = saved;
                        let val = self.parse_expr()?;
                        Ok((None, val))
                    }
                }
            }
            Token::DotDotDot => {
                self.advance();
                {
                    let sym = self.install_symbol("...");
                    Ok((None, sym))
                }
            }
            _ => {
                let val = self.parse_expr()?;
                Ok((None, val))
            }
        }
    }
}
