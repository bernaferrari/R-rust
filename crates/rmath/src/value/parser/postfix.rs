use super::*;

impl<'h, 'i> Parser<'h, 'i> {
    // -----------------------------------------------------------------------
    // Postfix operations (left-associative)
    // -----------------------------------------------------------------------

    /// Parse postfix operations: function calls, subscript, member access.
    /// These are all left-associative and chain together.
    pub(crate) fn parse_postfix(&mut self) -> Result<ValueId, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                // Function call: f(args)
                Token::LParen => {
                    self.advance();
                    let args = self.parse_arglist()?;
                    self.expect(&Token::RParen)?;

                    let nil = ValueId::NIL;
                    let mut arg_list = nil;
                    for (name, val) in args.into_iter().rev() {
                        let cell = self.cons(val, arg_list)?;
                        if let Some(n) = name {
                            let sym = self.install_symbol(&n);
                            self.set_tag(cell, sym);
                        }
                        arg_list = cell;
                    }
                    expr = self.lang_cons(expr, arg_list)?;
                }
                // Subscript: x[i], x[i, j]; empty slots (leading/trailing/
                // doubled commas) become missing arguments, like gram.y xxsub0.
                Token::LBracket => {
                    self.advance();
                    let slots = self.parse_subscript_slots(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;

                    {
                        let bracket_sym = self.install_symbol("[");
                        let nil = ValueId::NIL;
                        let mut arg_list = nil;
                        for (name, val) in slots.into_iter().rev() {
                            let cell = self.cons(val, arg_list)?;
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n);
                                self.set_tag(cell, sym);
                            }
                            arg_list = cell;
                        }
                        arg_list = self.cons(expr, arg_list)?;
                        expr = self.lang_cons(bracket_sym, arg_list)?;
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

                    {
                        let dbracket_sym = self.install_symbol("[[");
                        let nil = ValueId::NIL;
                        let mut arg_list = nil;
                        for (name, val) in slots.into_iter().rev() {
                            let cell = self.cons(val, arg_list)?;
                            if let Some(n) = name {
                                let sym = self.install_symbol(&n);
                                self.set_tag(cell, sym);
                            }
                            arg_list = cell;
                        }
                        arg_list = self.cons(expr, arg_list)?;
                        expr = self.lang_cons(dbracket_sym, arg_list)?;
                    }
                }
                // Dollar: x$name
                Token::Dollar => {
                    self.advance();
                    let name = self.parse_member_name()?;
                    {
                        let dollar_sym = self.install_symbol("$");
                        let nil = ValueId::NIL;
                        let name_cell = self.cons(name, nil)?;
                        let args = self.cons(expr, name_cell)?;
                        expr = self.lang_cons(dollar_sym, args)?;
                    }
                }
                // Namespace lookup: pkg::name or pkg:::name
                Token::DoubleColon | Token::TripleColon => {
                    let op_token = self.advance();
                    let name = self.parse_member_name()?;
                    {
                        let op = match op_token {
                            Token::TripleColon => self.install_symbol(":::"),
                            _ => self.install_symbol("::"),
                        };
                        expr = self.lang3(op, expr, name)?;
                    }
                }
                // At: x@name
                Token::At => {
                    self.advance();
                    let name = self.parse_member_name()?;
                    {
                        let at_sym = self.install_symbol("@");
                        let nil = ValueId::NIL;
                        let name_cell = self.cons(name, nil)?;
                        let args = self.cons(expr, name_cell)?;
                        expr = self.lang_cons(at_sym, args)?;
                    }
                }
                _ => return Ok(expr),
            }
        }
    }

    /// Parse a member name after $ or @ — can be identifier or backtick name.
    pub(crate) fn parse_member_name(&mut self) -> Result<ValueId, ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(self.install_symbol(&name))
            }
            Token::Str(name) => {
                // Allow "name" after $ for compatibility
                let name = name.clone();
                self.advance();
                Ok(self.install_symbol(&name))
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
    ) -> Result<Vec<(Option<String>, ValueId)>, ParseError> {
        let mut slots = Vec::new();
        self.skip_newlines();
        if self.peek() == close {
            // `x[]` and `x[[]]` each carry one explicitly missing index in
            // GNU's call AST. Keep that node: replacement dispatch observes
            // its presence via nargs(), and read-side `[[` must report a
            // missing subscript instead of treating the call as `x`.
            slots.push((None, ValueId::MISSING));
            return Ok(slots);
        }
        loop {
            self.skip_newlines();
            if self.peek() == &Token::Comma {
                // Empty slot before a comma
                slots.push((None, ValueId::MISSING));
                self.advance();
                self.skip_newlines();
                if self.peek() == close {
                    // Trailing comma: empty final slot
                    slots.push((None, ValueId::MISSING));
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
                    slots.push((None, ValueId::MISSING));
                    break;
                }
            } else {
                break;
            }
        }
        Ok(slots)
    }
}
