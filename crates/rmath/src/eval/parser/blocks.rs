use super::*;

impl<'arena, 'session> Parser<'arena, 'session> {
    // -----------------------------------------------------------------------
    // Blocks
    // -----------------------------------------------------------------------

    /// { expr; expr; ... }
    pub(crate) fn parse_block(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume '{'
        self.skip_terminators();

        let mut exprs = Vec::new();
        while self.peek() != &Token::RBrace && self.peek() != &Token::Eof {
            exprs.push(self.parse_expr()?);
            self.skip_terminators();
        }
        self.expect(&Token::RBrace)?;

        if exprs.is_empty() {
            unsafe {
                let brace_sym = Rf_install(c"{".as_ptr());
                self.lang2(brace_sym, R_NilValue())
            }
        } else if exprs.len() == 1 {
            unsafe {
                let brace_sym = Rf_install(c"{".as_ptr());
                self.lang2(brace_sym, exprs.pop().unwrap_or_else(|| R_NilValue()))
            }
        } else {
            unsafe {
                let brace_sym = Rf_install(c"{".as_ptr());
                let nil = R_NilValue();
                let mut list = self.cons(exprs.pop().unwrap_or(nil), nil)?;
                while let Some(e) = exprs.pop() {
                    list = self.cons(e, list)?;
                }
                let call = self.cons(brace_sym, list)?;
                if !call.is_null() {
                    crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                }
                Ok(call)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Atoms
    // -----------------------------------------------------------------------

    pub(crate) fn parse_atom(&mut self) -> Result<SEXP, ParseError> {
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
                unsafe {
                    let sym = Rf_install(c"...".as_ptr());
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
                    "TRUE" => self.scalar_logical(TRUE),
                    "FALSE" => self.scalar_logical(FALSE),

                    "NULL" => unsafe { Ok(R_NilValue()) },
                    "NA" => self.scalar_logical(NA_LOGICAL),
                    "Inf" => self.scalar_real(f64::INFINITY),
                    "NaN" => self.scalar_real(f64::NAN),
                    "NA_real_" => self.scalar_real(crate::sexp::ffi::NA_REAL),
                    "NA_integer_" => self.scalar_integer(crate::sexp::ffi::NA_INTEGER),
                    "NA_complex_" => self
                        .scalar_complex_parts(crate::sexp::ffi::NA_REAL, crate::sexp::ffi::NA_REAL),
                    "NA_character_" => self.scalar_na_string(),
                    _ => self.install_symbol(&name),
                }
            }
            _ => Err(self.unexpected_at(self.pos)),
        }
    }
    // -----------------------------------------------------------------------

    pub(crate) fn parse_arglist(&mut self) -> Result<Vec<(Option<String>, SEXP)>, ParseError> {
        let mut args = Vec::new();
        self.skip_newlines();
        if self.peek() == &Token::RParen {
            return Ok(args);
        }

        loop {
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                args.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                self.advance();
                self.skip_newlines();
                if self.peek() == &Token::RParen {
                    args.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
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
                    args.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
            } else {
                break;
            }
        }

        Ok(args)
    }

    pub(crate) fn parse_arg(&mut self) -> Result<(Option<String>, SEXP), ParseError> {
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
                            unsafe { crate::sexp::globals::R_MissingArg() }
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
                unsafe {
                    let sym = Rf_install(c"...".as_ptr());
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
