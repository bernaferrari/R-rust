use super::*;

impl<'arena, 'session> Parser<'arena, 'session> {
    // -----------------------------------------------------------------------
    // Control structures
    // -----------------------------------------------------------------------

    /// Whether an `if` whose body ended at `body_end` was already popped
    /// before a newline-crossing `else` (gram.y: `case ']'/')'/RBRACE:
    /// `while (*contextp == 'i') ifpop()`).
    ///
    /// The `if` is completed when, scanning back from `body_end`, a
    /// `]`/`)`/`}` closes the group the `if` body belonged to — i.e. the
    /// opener depth at the body's last token is deeper than (or the opener
    /// differs from) the opener at `body_end`. Closes strictly inside the
    /// body (a call's `)` before the body's final token) do not pop it.
    pub(crate) fn if_closed_before(&self, body_end: usize) -> bool {
        if body_end == 0 || body_end > self.tokens.len() {
            return false;
        }
        // Innermost unclosed opener just before the newline vs at the last
        // body token: if the body ended by closing its own group (the
        // opener changed between body-last and body-end), the `if` popped.
        let before = body_end.saturating_sub(1);
        let opener_at_end = self.group_opener.get(body_end).copied().flatten();
        let opener_before = self.group_opener.get(before).copied().flatten();
        opener_at_end != opener_before
    }

    /// if (cond) expr [else expr]
    pub(crate) fn parse_if(&mut self) -> Result<SEXP, ParseError> {
        // Token index of this `if`: the EatLines gate below probes the
        // group depth HERE (gram.y pushes 'i' at IF-time when inside a
        // group; a bare `[[` later in the body must not change the answer).
        let if_pos = self.pos;
        self.advance(); // consume 'if'
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        // Skip newlines after )
        self.skip_newlines();
        let body = self.parse_expr()?;

        // Check for else. Upstream (gram.y token(): `case IF: IfPush()`
        // pushes 'i' when inside `(`/`[`/`{`; `case ']'/')'/RBRACE:
        // `while (*contextp == 'i') ifpop()` pops it) reports ELSE after a
        // newline only when the `if` is still on the context stack: at top
        // level the `if` was never pushed, so a newline-terminated body
        // rejects `else`; inside a function body or `{` block the `if`
        // sits under an unclosed group, so whisker's `if (escape)\n
        // gsub(...)\n else ...` attaches. Gate on the group depth at the
        // `if` keyword itself: `None` (top level) rejects a
        // newline-crossing `else`, `Some(_)` (inside a group) accepts it.
        let body_end = self.pos;
        let if_opener = self.group_opener.get(if_pos).copied().flatten();
        self.skip_newlines();
        if self.peek() == &Token::KwElse {
            // A newline ends `else` only at top level. Inside `{` or `(`,
            // GNU source() still attaches it (`if (cond)\n expr\n else expr`).
            if self.pos > body_end && if_opener.is_none() {
                return Err(self.unexpected_at(self.pos));
            }
            self.advance();
            self.skip_newlines();
            let alt = self.parse_expr()?;
            unsafe {
                let if_sym = Rf_install(c"if".as_ptr());
                let nil = R_NilValue();
                let alt_cell = self.cons(alt, nil)?;
                let body_cell = self.cons(body, alt_cell)?;
                let cond_cell = self.cons(cond, body_cell)?;
                let call = self.cons(if_sym, cond_cell)?;
                if !call.is_null() {
                    crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                }
                Ok(call)
            }
        } else {
            // Newlines after a complete if-body are statement terminators,
            // not postfix call openers: `if (c) f()\n(x)` is two expressions
            // (gram.y), not `f()(x)`.
            self.pos = body_end;
            unsafe {
                let if_sym = Rf_install(c"if".as_ptr());
                self.lang3(if_sym, cond, body)
            }
        }
    }

    /// for (var in seq) body
    pub(crate) fn parse_for(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'for'
        self.skip_newlines();
        self.expect(&Token::LParen)?;

        // var must be an identifier
        let var_name = match self.advance() {
            Token::Ident(name) => name,
            _ => return Err(self.unexpected_at(self.pos - 1)),
        };
        let var = self.install_symbol(&var_name)?;

        self.expect(&Token::KwIn)?;
        let seq = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let for_sym = Rf_install(c"for".as_ptr());
            let nil = R_NilValue();
            let body_cell = self.cons(body, nil)?;
            let seq_cell = self.cons(seq, body_cell)?;
            let var_cell = self.cons(var, seq_cell)?;
            let call = self.cons(for_sym, var_cell)?;
            if !call.is_null() {
                crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
            }
            Ok(call)
        }
    }

    /// while (cond) body
    pub(crate) fn parse_while(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'while'
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let while_sym = Rf_install(c"while".as_ptr());
            self.lang3(while_sym, cond, body)
        }
    }

    /// repeat body
    pub(crate) fn parse_repeat(&mut self) -> Result<SEXP, ParseError> {
        self.advance(); // consume 'repeat'
        self.skip_newlines();
        let body = self.parse_expr()?;

        unsafe {
            let repeat_sym = Rf_install(c"repeat".as_ptr());
            self.lang2(repeat_sym, body)
        }
    }

    /// function(args) body — also handles R 4.1+ `\(...)` lambda syntax.
    pub(crate) fn parse_function(&mut self) -> Result<SEXP, ParseError> {
        let tok_start = self.spans.get(self.pos).map(|&(a, _)| a).unwrap_or(0);
        match self.peek() {
            Token::KwFunction | Token::KwLambda => {
                self.advance();
            }
            _ => {}
        }
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        let formals = self.parse_formals()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;
        let tok_end = if self.pos > 0 {
            self.spans
                .get(self.pos - 1)
                .map(|&(_, b)| b)
                .unwrap_or(tok_start)
        } else {
            tok_start
        };

        unsafe {
            let src: String = self.source.iter().collect();
            let lloc = crate::mainutils::srcref::srcref_lloc(&src, tok_start, tok_end);
            let srcref = self.arena.alloc_vector(SEXPTYPE::INTSXP, 8);
            if srcref.is_null() {
                return Err(self.allocation_error());
            }
            let p = INTEGER(srcref);
            for (i, v) in lloc.iter().enumerate() {
                *p.add(i) = *v;
            }
            let cls = self.scalar_string("srcref")?;
            let class_cell = self.cons(cls, R_NilValue())?;
            SETTAG(class_cell, crate::sexp::attrib_core::R_ClassSymbol());
            SET_ATTRIB(srcref, class_cell);
            let fn_sym = Rf_install(c"function".as_ptr());
            let nil = R_NilValue();
            // GNU gram.y: lang4(function, formals, body, R_NilValue) plus
            // a `srcref` attribute. Putting the integer vector in the 4th
            // language slot made inner `function()` AST cells differ under
            // `identical(..., ignore.srcref=TRUE)`.
            let fourth = if self.keep_srcrefs { srcref } else { nil };
            let srcref_cell = self.cons(fourth, nil)?;
            let body_cell = self.cons(body, srcref_cell)?;
            let formals_cell = self.cons(formals, body_cell)?;
            let call = self.cons(fn_sym, formals_cell)?;
            if !call.is_null() {
                crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                if self.keep_srcrefs {
                    let attr_cell = self.cons(srcref, nil)?;
                    SETTAG(attr_cell, Rf_install(c"srcref".as_ptr()));
                    SET_ATTRIB(call, attr_cell);
                }
                if self.keep_srcrefs
                    && !body.is_null()
                    && TYPEOF(body) == SEXPTYPE::LANGSXP
                    && CAR(body) != Rf_install(c"{".as_ptr())
                {
                    let body_attr = self.cons(srcref, nil)?;
                    SETTAG(body_attr, Rf_install(c"srcref".as_ptr()));
                    SET_ATTRIB(body, body_attr);
                }
            }
            Ok(call)
        }
    }

    /// Parse formal arguments: name [= default], name [= default], ...
    /// Returns a pairlist of (name default) pairs.
    pub(crate) fn parse_formals(&mut self) -> Result<SEXP, ParseError> {
        let nil = unsafe { R_NilValue() };
        let mut pairs: Vec<(String, SEXP)> = Vec::new();

        self.skip_newlines();
        if self.peek() == &Token::RParen {
            return Ok(nil);
        }

        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::DotDotDot => {
                    self.advance();
                    pairs.push(("...".to_string(), unsafe { R_NilValue() }));
                }
                Token::Ident(name) => {
                    let name = name.clone();
                    if pairs.iter().any(|(seen, _)| seen == &name) {
                        return Err(self.token_position_error(
                            self.pos,
                            &format!("repeated formal argument '{name}'"),
                        ));
                    }
                    self.advance();
                    // GNU accepts a line break between a formal's name
                    // and its '=' (e.g. base demo recursion.R: `limit\n= 10`).
                    self.skip_newlines();
                    let default = if self.peek() == &Token::Assign {
                        self.advance();
                        self.parse_expr()?
                    } else {
                        unsafe { crate::sexp::globals::R_MissingArg() }
                    };
                    pairs.push((name, default));
                }
                _ => return Err(self.unexpected_at(self.pos)),
            }

            self.skip_newlines();
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        // Build pairlist in reverse (R expects first arg first)
        unsafe {
            let mut list = nil;
            for (name, default) in pairs.into_iter().rev() {
                let cell = self.cons(default, list)?;
                let sym = self.install_symbol(&name)?;
                crate::sexp::accessors::SETTAG(cell, sym);
                list = cell;
            }
            Ok(list)
        }
    }
}
