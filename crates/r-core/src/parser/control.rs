use super::*;

impl<'h, 'i> Parser<'h, 'i> {
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
    pub fn if_closed_before(&self, body_end: usize) -> bool {
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
    pub fn parse_if(&mut self) -> Result<ValueId, ParseError> {
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
            {
                let if_sym = self.install_symbol("if");
                let nil = ValueId::NIL;
                let alt_cell = self.cons(alt, nil)?;
                let body_cell = self.cons(body, alt_cell)?;
                let cond_cell = self.cons(cond, body_cell)?;
                self.lang_cons(if_sym, cond_cell)
            }
        } else {
            // Newlines after a complete if-body are statement terminators,
            // not postfix call openers: `if (c) f()\n(x)` is two expressions
            // (gram.y), not `f()(x)`.
            self.pos = body_end;
            {
                let if_sym = self.install_symbol("if");
                self.lang3(if_sym, cond, body)
            }
        }
    }

    /// for (var in seq) body
    pub fn parse_for(&mut self) -> Result<ValueId, ParseError> {
        self.advance(); // consume 'for'
        self.skip_newlines();
        self.expect(&Token::LParen)?;

        // var must be an identifier
        let var_name = match self.advance() {
            Token::Ident(name) => name,
            _ => return Err(self.unexpected_at(self.pos - 1)),
        };
        let var = self.install_symbol(&var_name);

        self.expect(&Token::KwIn)?;
        let seq = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        {
            let for_sym = self.install_symbol("for");
            let nil = ValueId::NIL;
            let body_cell = self.cons(body, nil)?;
            let seq_cell = self.cons(seq, body_cell)?;
            let var_cell = self.cons(var, seq_cell)?;
            self.lang_cons(for_sym, var_cell)
        }
    }

    /// while (cond) body
    pub fn parse_while(&mut self) -> Result<ValueId, ParseError> {
        self.advance(); // consume 'while'
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        let body = self.parse_expr()?;

        {
            let while_sym = self.install_symbol("while");
            self.lang3(while_sym, cond, body)
        }
    }

    /// repeat body
    pub fn parse_repeat(&mut self) -> Result<ValueId, ParseError> {
        self.advance(); // consume 'repeat'
        self.skip_newlines();
        let body = self.parse_expr()?;

        {
            let repeat_sym = self.install_symbol("repeat");
            self.lang2(repeat_sym, body)
        }
    }

    /// function(args) body — also handles R 4.1+ `\(...)` lambda syntax.
    pub fn parse_function(&mut self) -> Result<ValueId, ParseError> {
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

        {
            let fn_sym = self.install_symbol("function");
            let nil = ValueId::NIL;
            // GNU gram.y: lang4(function, formals, body, R_NilValue) — the
            // fourth slot carries the srcref when keep.source is on, NULL
            // otherwise. The native parser never attaches srcrefs, so the
            // fourth slot is always NIL here.
            let srcref_cell = self.cons(nil, nil)?;
            let body_cell = self.cons(body, srcref_cell)?;
            let formals_cell = self.cons(formals, body_cell)?;
            self.lang_cons(fn_sym, formals_cell)
        }
    }

    /// Parse formal arguments: name [= default], name [= default], ...
    /// Returns a pairlist of (name default) pairs.
    pub fn parse_formals(&mut self) -> Result<ValueId, ParseError> {
        let nil = ValueId::NIL;
        let mut pairs: Vec<(String, ValueId)> = Vec::new();

        self.skip_newlines();
        if self.peek() == &Token::RParen {
            return Ok(nil);
        }

        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::DotDotDot => {
                    self.advance();
                    pairs.push(("...".to_string(), ValueId::NIL));
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
                        ValueId::MISSING
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

        let mut list = nil;
        for (name, default) in pairs.into_iter().rev() {
            let cell = self.cons(default, list)?;
            let sym = self.install_symbol(&name);
            self.set_tag(cell, sym);
            list = cell;
        }
        Ok(list)
    }
}
