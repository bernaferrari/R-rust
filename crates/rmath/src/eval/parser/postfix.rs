use super::*;

impl<'arena, 'session> Parser<'arena, 'session> {
    // -----------------------------------------------------------------------
    // Postfix operations (left-associative)
    // -----------------------------------------------------------------------

    /// Parse postfix operations: function calls, subscript, member access.
    /// These are all left-associative and chain together.
    pub(crate) fn parse_postfix(&mut self) -> Result<SEXP, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                // Function call: f(args)
                Token::LParen => {
                    self.advance();
                    let args = self.parse_arglist()?;
                    self.expect(&Token::RParen)?;

                    unsafe {
                        let nil = R_NilValue();
                        let mut arg_list = nil;
                        for (name, val) in args.into_iter().rev() {
                            let cell = self.cons(val, arg_list)?;
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n)?;
                                crate::sexp::accessors::SETTAG(cell, sym);
                            }
                            arg_list = cell;
                        }
                        let call = self.cons(expr, arg_list)?;
                        if !call.is_null() {
                            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                        }
                        expr = call;
                    }
                }
                // Subscript: x[i], x[i, j]; empty slots (leading/trailing/
                // doubled commas) become missing arguments, like gram.y xxsub0.
                Token::LBracket => {
                    self.advance();
                    let slots = self.parse_subscript_slots(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;

                    unsafe {
                        let bracket_sym = Rf_install(c"[".as_ptr());
                        let nil = R_NilValue();
                        let mut arg_list = nil;
                        for (name, val) in slots.into_iter().rev() {
                            let cell = self.cons(val, arg_list)?;
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n)?;
                                crate::sexp::accessors::SETTAG(cell, sym);
                            }
                            arg_list = cell;
                        }
                        arg_list = self.cons(expr, arg_list)?;
                        let call = self.cons(bracket_sym, arg_list)?;
                        if !call.is_null() {
                            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                        }
                        expr = call;
                    }
                }
                // Double subscript: x[[i]], x[[i, j]] — multiple comma-
                // separated slots allowed, empty slots become missing args.
                // Closes are consumed positionally as two `]` tokens (the
                // lexer never fuses `]]`): the first closes the slot list,
                // the second closes the `[[` opener.
                Token::LDoubleBracket => {
                    self.advance();
                    let slots = self.parse_subscript_slots(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;

                    unsafe {
                        let dbracket_sym = Rf_install(c"[[".as_ptr());
                        let nil = R_NilValue();
                        let mut arg_list = nil;
                        for (name, val) in slots.into_iter().rev() {
                            let cell = self.cons(val, arg_list)?;
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n)?;
                                crate::sexp::accessors::SETTAG(cell, sym);
                            }
                            arg_list = cell;
                        }
                        arg_list = self.cons(expr, arg_list)?;
                        let call = self.cons(dbracket_sym, arg_list)?;
                        if !call.is_null() {
                            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                        }
                        expr = call;
                    }
                }
                // Dollar: x$name
                Token::Dollar => {
                    self.advance();
                    let name = self.parse_member_name()?;
                    unsafe {
                        let dollar_sym = Rf_install(c"$".as_ptr());
                        let nil = R_NilValue();
                        let name_cell = self.cons(name, nil)?;
                        let args = self.cons(expr, name_cell)?;
                        let call = self.cons(dollar_sym, args)?;
                        if !call.is_null() {
                            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                        }
                        expr = call;
                    }
                }
                // Namespace lookup: pkg::name or pkg:::name
                Token::DoubleColon | Token::TripleColon => {
                    let op_token = self.advance();
                    let name = self.parse_member_name()?;
                    unsafe {
                        let op = match op_token {
                            Token::TripleColon => Rf_install(c":::".as_ptr()),
                            _ => Rf_install(c"::".as_ptr()),
                        };
                        expr = self.lang3(op, expr, name)?;
                    }
                }
                // At: x@name
                Token::At => {
                    self.advance();
                    let name = self.parse_member_name()?;
                    unsafe {
                        let at_sym = Rf_install(c"@".as_ptr());
                        let nil = R_NilValue();
                        let name_cell = self.cons(name, nil)?;
                        let args = self.cons(expr, name_cell)?;
                        let call = self.cons(at_sym, args)?;
                        if !call.is_null() {
                            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                        }
                        expr = call;
                    }
                }
                _ => return Ok(expr),
            }
        }
    }

    /// Parse a member name after $ or @ — can be identifier or backtick name.
    pub(crate) fn parse_member_name(&mut self) -> Result<SEXP, ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                self.install_symbol(&name)
            }
            Token::Str(name) => {
                // Allow "name" after $ for compatibility
                let name = name.clone();
                self.advance();
                self.install_symbol(&name)
            }
            _ => Err(self.unexpected_at(self.pos)),
        }
    }

    /// Parse the slot list of `[` / `[[` subscripts: comma-separated
    /// expressions where empty slots (leading/trailing/doubled commas, or
    /// nothing before the closing bracket) become R_MissingArg, mirroring
    /// gram.y's xxsub0. A `name = value` slot is tagged like a call argument
    /// (subset.rs ExtractDropArg relies on tags for `drop=` / `exact=`).
    pub(crate) fn parse_subscript_slots(
        &mut self,
        close: &Token,
    ) -> Result<Vec<(Option<String>, SEXP)>, ParseError> {
        let mut slots = Vec::new();
        self.skip_newlines();
        if self.peek() == close {
            // `x[]` and `x[[]]` each carry one explicitly missing index in
            // GNU's call AST. Keep that node: replacement dispatch observes
            // its presence via nargs(), and read-side `[[` must report a
            // missing subscript instead of treating the call as `x`.
            slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
            return Ok(slots);
        }
        loop {
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                // Empty slot before a comma
                slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                self.advance();
                self.skip_newlines();
                if self.peek() == close {
                    // Trailing comma: empty final slot
                    slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
                continue;
            }
            if self.peek() == close {
                break;
            }
            let (name, val) = self.parse_arg()?;
            slots.push((name, val));
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek() == close {
                    slots.push((None, unsafe { crate::sexp::globals::R_MissingArg() }));
                    break;
                }
            } else {
                break;
            }
        }
        Ok(slots)
    }
}
