use super::*;

impl<'arena, 'session> Parser<'arena, 'session> {
    /// Kept so callers can request file-parse mode. Newline-`else` follows
    /// the group opener either way.
    pub fn set_strict_newline_else(&mut self, _strict: bool) {}

    pub fn new(
        input: &str,
        arena: &'arena mut RArena,
        domain: impl Into<NodeDomain<'session>>,
    ) -> Self {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        let mut spans = Vec::new();
        let mut token_literal_warnings = Vec::new();
        let mut have_pipebind = false;
        loop {
            let tok = lexer.next_token();
            let end = lexer.pos;
            let start = lexer.last_token_start;
            let is_eof = tok == Token::Eof;
            if tok == Token::PipeBind {
                have_pipebind = true;
            }
            for message in take_pending_literal_warnings() {
                token_literal_warnings.push((tokens.len(), message));
            }
            tokens.push(tok);
            spans.push((start, end));
            if is_eof {
                break;
            }
        }
        let mut inside_group = Vec::with_capacity(tokens.len());
        let mut inside_nest = Vec::with_capacity(tokens.len());
        let mut group_opener: Vec<Option<usize>> = Vec::with_capacity(tokens.len());
        let mut depth = 0usize;
        let mut nest_stack: Vec<bool> = Vec::new();
        let mut opener_stack: Vec<usize> = Vec::new();
        for (idx, tok) in tokens.iter().enumerate() {
            inside_group.push(depth > 0);
            inside_nest.push(nest_stack.last().copied().unwrap_or(false));
            group_opener.push(opener_stack.last().copied());
            match tok {
                Token::LParen | Token::LBracket => {
                    opener_stack.push(idx);
                    depth += 1;
                    nest_stack.push(true);
                }
                Token::LBrace => {
                    opener_stack.push(idx);
                    depth += 1;
                    nest_stack.push(false);
                }
                Token::LDoubleBracket => {
                    opener_stack.push(idx);
                    opener_stack.push(idx);
                    depth += 2;
                    nest_stack.push(true);
                    nest_stack.push(true);
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    opener_stack.pop();
                    depth = depth.saturating_sub(1);
                    nest_stack.pop();
                }
                _ => {}
            }
        }
        Parser {
            domain: domain.into(),
            tokens,
            spans,
            source: input.chars().collect(),
            pos: 0,
            arena,
            placeholder: std::ptr::null_mut(),
            have_pipebind,
            inside_group,
            inside_nest,
            group_opener,

            token_literal_warnings,
            keep_srcrefs: false,
        }
    }

    /// Reject a foreign arena or symbol bank before publishing graph edges.
    /// Symbol interning is callback-free and touches only the persistent bank;
    /// parsing allocates exclusively through the separately lent arena.
    pub(crate) fn validate_domain(&self) -> Result<(), ParseError> {
        if !self.domain.belongs_to(&self.arena.heap_identity()) {
            return Err(ParseError("parser arena belongs to another runtime".into()));
        }
        self.domain
            .wrap(self.domain.nil().as_raw())
            .map_err(|error| ParseError(error.to_string()))?;
        let active_heap = crate::sexp::instance::current_instance_ptr()
            .map(|owner| unsafe { (*owner).heap_identity.clone() });
        if !active_heap
            .as_ref()
            .is_some_and(|heap| self.domain.belongs_to(heap))
        {
            return Err(ParseError(
                "parser symbol bank belongs to another runtime".into(),
            ));
        }
        Ok(())
    }

    /// The shared placeholder node for `_`, allocated on first use.
    pub(crate) fn placeholder_node(&mut self) -> Result<SEXP, ParseError> {
        if self.placeholder.is_null() {
            self.placeholder = self.scalar_string("_")?;
        }
        Ok(self.placeholder)
    }

    pub(crate) fn install_symbol(&self, name: &str) -> Result<SEXP, ParseError> {
        let c_name =
            CString::new(name).map_err(|_| ParseError(format!("symbol contains NUL: {name:?}")))?;
        let symbol = unsafe { Rf_install(c_name.as_ptr()) };
        if symbol.is_null() {
            Err(self.allocation_error())
        } else {
            Ok(symbol)
        }
    }

    pub(crate) fn allocation_error(&self) -> ParseError {
        self.token_position_error(
            self.pos.saturating_sub(1),
            "parser allocation failed: arena resource limit exceeded",
        )
    }

    pub(crate) fn cons(&mut self, car: SEXP, cdr: SEXP) -> Result<SEXP, ParseError> {
        let cell = unsafe {
            /* SAFETY: parser owns this arena lend; parsed children remain in the same arena. */
            self.arena.cons(car, cdr, std::ptr::null_mut())
        };
        if cell.is_null() {
            Err(self.allocation_error())
        } else {
            Ok(cell)
        }
    }

    pub(crate) fn lang2(&mut self, car: SEXP, arg: SEXP) -> Result<SEXP, ParseError> {
        let nil = unsafe { R_NilValue() };
        let arg_cell = self.cons(arg, nil)?;
        let call = self.cons(car, arg_cell)?;
        unsafe {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        Ok(call)
    }

    pub(crate) fn lang3(&mut self, car: SEXP, arg1: SEXP, arg2: SEXP) -> Result<SEXP, ParseError> {
        let nil = unsafe { R_NilValue() };
        let arg2_cell = self.cons(arg2, nil)?;
        let arg1_cell = self.cons(arg1, arg2_cell)?;
        let call = self.cons(car, arg1_cell)?;
        unsafe {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        Ok(call)
    }

    pub(crate) fn scalar_real(&mut self, value: f64) -> Result<SEXP, ParseError> {
        scalar_real_in(self.arena, value)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_complex(&mut self, imaginary: f64) -> Result<SEXP, ParseError> {
        scalar_complex_in(self.arena, 0.0, imaginary)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_complex_parts(
        &mut self,
        real: f64,
        imaginary: f64,
    ) -> Result<SEXP, ParseError> {
        scalar_complex_in(self.arena, real, imaginary)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_integer(&mut self, value: i32) -> Result<SEXP, ParseError> {
        scalar_integer_in(self.arena, value)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_logical(&mut self, value: i32) -> Result<SEXP, ParseError> {
        scalar_logical_in(self.arena, value)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_string(&mut self, value: &str) -> Result<SEXP, ParseError> {
        scalar_string_in(self.arena, value)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_bytes(&mut self, bytes: &[u8]) -> Result<SEXP, ParseError> {
        scalar_bytes_in(self.arena, bytes)
            .map(|s| s.as_raw())
            .ok_or_else(|| self.allocation_error())
    }

    pub(crate) fn scalar_na_string(&mut self) -> Result<SEXP, ParseError> {
        let strings = self.arena.alloc_vector(SEXPTYPE::STRSXP, 1);
        if strings.is_null() {
            return Err(self.allocation_error());
        }
        if self
            .arena
            .set_reference_element(strings, 0, unsafe { R_NaString() })
            .is_none()
        {
            return Err(self.allocation_error());
        }
        Ok(strings)
    }

    pub(crate) fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    pub(crate) fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    pub(crate) fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        let tok = self.advance();
        if &tok == expected {
            Ok(())
        } else {
            // Upstream renders bison "expecting" errors as plain
            // "unexpected X" (gram.y edits the expecting clause away).
            Err(self.unexpected_at(self.pos - 1))
        }
    }

    /// Build the upstream-shaped parse error for the token at `index`:
    /// `unexpected ')' in "<source context>"`, with no context when the
    /// input ran out. The context is the source through the end of the
    /// offending token, windowed to the last `PARSE_CONTEXT_WINDOW` chars
    /// and reduced to its final two lines (source.c `parseError`).
    pub(crate) fn unexpected_at(&self, index: usize) -> ParseError {
        let tok = self.tokens.get(index).cloned().unwrap_or(Token::Eof);
        let head = format!("unexpected {}", token_display(&tok));
        if matches!(tok, Token::Eof) {
            return ParseError(head);
        }
        let (start, end_span) = self
            .spans
            .get(index)
            .copied()
            .unwrap_or((0, self.source.len()));
        let end = end_span;
        let loc = if matches!(tok, Token::Invalid) {
            let (ln, col) = self.line_col(start);
            format!(" (<input>:{ln}:{})", col + 1)
        } else {
            String::new()
        };
        let start_w = end.saturating_sub(PARSE_CONTEXT_WINDOW);
        let window: String = self.source[start_w..end].iter().collect();
        let mut lines: Vec<&str> = window.split('\n').collect();
        if matches!(lines.last(), Some(&"")) {
            lines.pop();
        }
        match lines.as_slice() {
            [] | [""] => ParseError(format!("{head}{loc}")),
            [line] => ParseError(format!("{head} in \"{line}\"{loc}")),
            lines => ParseError(format!(
                "{head} in:\n\"{}\n{}\"{loc}",
                lines[lines.len() - 2],
                lines[lines.len() - 1]
            )),
        }
    }

    /// 1-based line and 0-based column of a char offset in the source.
    /// Upstream counts columns in first-bytes (like chars) and snaps tabs
    /// to 8-column stops; plain char counting matches it off tab runs.
    pub(crate) fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let mut line = 1usize;
        let mut col = 0usize;
        let mut i = 0;
        while i < offset {
            match self.source[i] {
                '\r' => {
                    line += 1;
                    col = 0;
                    i += 1;
                    if i < offset && self.source.get(i) == Some(&'\n') {
                        i += 1;
                    }
                }
                '\n' => {
                    line += 1;
                    col = 0;
                    i += 1;
                }
                _ => {
                    col += 1;
                    i += 1;
                }
            }
        }
        (line, col)
    }

    /// Render `<msg> (<input>:line:col)` for a token index. Upstream
    /// captures the location right after SkipSpace consumed the token's
    /// first character (gram.y token()), so the reported column is one
    /// past the token's first character.
    pub(crate) fn token_position_error(&self, index: usize, msg: &str) -> ParseError {
        let (start, _) = self
            .spans
            .get(index)
            .copied()
            .unwrap_or((self.source.len(), self.source.len()));
        let (line, col) = self.line_col(start);
        ParseError(format!("{msg} (<input>:{line}:{})", col + 1))
    }

    /// Position for the post-parse per-expression checks (gram.y's
    /// R_Parse1 handler): the parser's lookahead is reported there. A
    /// consumed newline resets the column to 0 (the Status-3 line
    /// adjustment then restores the expression's last line), and EOF
    /// behaves the same way since upstream parses a virtual trailing
    /// newline; any other terminator reports its own position with the
    /// same +1 quirk as `token_position_error`.
    pub(crate) fn pipebind_position_error(&self, msg: &str) -> ParseError {
        let index = self.pos;
        let (start, _) = self
            .spans
            .get(index)
            .copied()
            .unwrap_or((self.source.len(), self.source.len()));
        let (line, _) = self.line_col(start);
        match self.tokens.get(index) {
            Some(Token::Newline) | Some(Token::Eof) | None => {
                ParseError(format!("{msg} (<input>:{line}:0)"))
            }
            _ => {
                let (_, col) = self.line_col(start);
                ParseError(format!("{msg} (<input>:{line}:{})", col + 1))
            }
        }
    }

    /// Skip newlines (but not semicolons).
    pub(crate) fn skip_newlines(&mut self) {
        while self.peek() == &Token::Newline {
            self.advance();
        }
    }

    /// Skip semicolons and newlines.
    pub(crate) fn skip_terminators(&mut self) {
        while self.peek() == &Token::Semicolon || self.peek() == &Token::Newline {
            self.advance();
        }
    }

    pub(crate) fn take_token_warnings(&mut self, start: usize, end: usize) -> Vec<String> {
        let mut out = Vec::new();
        self.token_literal_warnings.retain(|(index, message)| {
            if *index >= start && *index < end {
                out.push(message.clone());
                false
            } else {
                true
            }
        });
        out
    }

    pub fn parse_top_level_expressions(&mut self) -> Result<Vec<Sexp<'session>>, ParseError> {
        self.validate_domain()?;
        begin_parsed_expr_warnings();
        let mut exprs = Vec::new();
        loop {
            self.skip_terminators();
            if self.peek() == &Token::Eof {
                break;
            }
            let start = self.pos;
            let expr = self.parse_expr()?;
            if self.expr_contains_placeholder(expr) {
                return Err(ParseError("invalid use of pipe placeholder".to_string()));
            }
            if self.have_pipebind && expr_contains_pipebind(expr) {
                return Err(self.pipebind_position_error("invalid use of pipe bind symbol"));
            }
            exprs.push(
                self.domain
                    .wrap(expr)
                    .map_err(|error| ParseError(error.to_string()))?,
            );
            record_parsed_expr_warning_msgs(self.take_token_warnings(start, self.pos));
            self.skip_terminators();
        }

        Ok(exprs)
    }

    /// Parse top-level expressions together with their (start, end) BYTE
    /// spans in the source, for srcref attribution (keep.source=TRUE).
    /// `pos` is the lexer's byte cursor: snapshot it before/after each
    /// top-level expression, skipping leading terminators so the span
    /// starts at the expression's first token.
    pub(crate) fn input_len(&self) -> usize {
        self.spans.last().map(|&(_, b)| b).unwrap_or(0)
    }

    /// When set, `function` expressions and their single-call bodies carry a
    /// `srcref` attribute. GNU `Rscript` leaves `keep.source` false, and
    /// `identical(body(f), substitute(...))` is how `setGeneric` decides a
    /// skeleton is a standard generic.
    pub fn set_keep_srcrefs(&mut self, keep: bool) {
        self.keep_srcrefs = keep;
    }

    pub fn parse_top_level_with_spans(
        &mut self,
    ) -> Result<Vec<(Sexp<'session>, usize, usize)>, ParseError> {
        self.validate_domain()?;
        begin_parsed_expr_warnings();
        let mut spans = Vec::new();
        loop {
            self.skip_terminators();
            if self.peek() == &Token::Eof {
                break;
            }
            let start = self.pos;
            let tok_start = self
                .spans
                .get(self.pos)
                .map(|&(a, _)| a)
                .unwrap_or(self.input_len());
            let expr = self.parse_expr()?;
            if self.expr_contains_placeholder(expr) {
                return Err(ParseError("invalid use of pipe placeholder".to_string()));
            }
            if self.have_pipebind && expr_contains_pipebind(expr) {
                return Err(self.pipebind_position_error("invalid use of pipe bind symbol"));
            }
            let tok_end = if self.pos > 0 {
                self.spans
                    .get(self.pos - 1)
                    .map(|&(_, b)| b)
                    .unwrap_or(tok_start)
            } else {
                tok_start
            };
            spans.push((
                self.domain
                    .wrap(expr)
                    .map_err(|error| ParseError(error.to_string()))?,
                tok_start,
                tok_end,
            ));
            record_parsed_expr_warning_msgs(self.take_token_warnings(start, self.pos));
            self.skip_terminators();
        }
        Ok(spans)
    }

    pub fn parse_program(&mut self) -> Result<Sexp<'session>, ParseError> {
        let exprs = self.parse_top_level_expressions()?;
        if exprs.is_empty() {
            return Ok(self.domain.nil());
        }
        if exprs.len() == 1 {
            return Ok(exprs.into_iter().next().expect("one parsed expression"));
        }

        // Retain every completed expression until the new brace call owns
        // their graph and its own automatic allocation lease is installed.
        let brace_sym = self.install_symbol("{")?;
        let nil = unsafe { R_NilValue() };
        let mut list = nil;
        for expression in exprs.iter().rev() {
            list = self.cons(expression.clone().as_raw(), list)?;
        }
        let call = self.cons(brace_sym, list)?;
        unsafe {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        self.domain
            .wrap(call)
            .map_err(|error| ParseError(error.to_string()))
    }
}
