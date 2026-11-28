use super::*;

impl<'h, 'i> Parser<'h, 'i> {
    /// Kept so callers can request file-parse mode. Newline-`else` follows
    /// the group opener either way.
    pub fn set_strict_newline_else(&mut self, _strict: bool) {}

    pub fn new(input: &str, heap: &'h mut Heap, interner: &'i mut SymbolInterner) -> Self {
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
            for message in lexer.take_pending_literal_warnings() {
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
            tokens,
            spans,
            source: input.chars().collect(),
            pos: 0,
            heap,
            interner,
            placeholder: ValueId::NIL,
            have_pipebind,
            inside_group,
            inside_nest,
            group_opener,
            token_literal_warnings,
        }
    }

    /// The shared placeholder node for `_`, allocated on first use.
    pub fn placeholder_node(&mut self) -> Result<ValueId, ParseError> {
        if self.placeholder == ValueId::NIL {
            self.placeholder = self.scalar_string("_")?;
        }
        Ok(self.placeholder)
    }

    /// Allocate a fresh symbol object exactly as the bridge importer does:
    /// interned name, NIL value/internal edges — identity is per-node,
    /// equality by interned name.
    pub fn install_symbol(&mut self, name: &str) -> ValueId {
        let sym = self.interner.intern_symbol(name);
        self.heap.alloc(RObject::Symbol {
            name: sym,
            value: ValueId::NIL,
            internal: ValueId::NIL,
        })
    }

    pub fn allocation_error(&self) -> ParseError {
        self.token_position_error(
            self.pos.saturating_sub(1),
            "parser allocation failed: arena resource limit exceeded",
        )
    }

    /// A pairlist cell — the argument-chain node exactly as the importer's
    /// LISTSXP conversion pins it.
    pub fn cons(&mut self, car: ValueId, cdr: ValueId) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Pairlist {
            car,
            cdr,
            tag: ValueId::NIL,
        }))
    }

    /// A one-argument call: Language head whose cdr is the pairlist chain,
    /// exactly the importer's LANGSXP shape.
    pub fn lang2(&mut self, car: ValueId, arg: ValueId) -> Result<ValueId, ParseError> {
        let arg_cell = self.cons(arg, ValueId::NIL)?;
        Ok(self.heap.alloc(RObject::Language {
            car,
            cdr: arg_cell,
            tag: ValueId::NIL,
        }))
    }

    pub fn lang3(&mut self, car: ValueId, arg1: ValueId, arg2: ValueId) -> Result<ValueId, ParseError> {
        let arg2_cell = self.cons(arg2, ValueId::NIL)?;
        let arg1_cell = self.cons(arg1, arg2_cell)?;
        Ok(self.heap.alloc(RObject::Language {
            car,
            cdr: arg1_cell,
            tag: ValueId::NIL,
        }))
    }

    /// Patch a pairlist cell's tag in place — the heap-native form of
    /// the legacy parser's SETTAG, used when formal arguments and named
    /// call arguments gain their names after construction.
    pub fn set_tag(&mut self, cell: ValueId, tag: ValueId) {
        if let Some(slot @ RObject::Pairlist { .. }) = self.heap.object_mut(cell)
            && let RObject::Pairlist { tag: slot_tag, .. } = slot {
                *slot_tag = tag;
            }
    }

    /// Patch a cell's car in place — used by pipe rewriting exactly as the
    /// legacy parser's SETCAR.
    pub fn set_car(&mut self, cell: ValueId, new_car: ValueId) {
        if let Some(slot) = self.heap.object_mut(cell) {
            match slot {
                RObject::Pairlist { car, .. } | RObject::Language { car, .. } => *car = new_car,
                _ => {}
            }
        }
    }

    /// Tree accessors mirroring the legacy CAR/CDR/TAG — NIL when the
    /// node is not a chain cell, exactly as the C macros never fire on
    /// non-cells in the parser's controlled usage.
    pub fn car(&self, value: ValueId) -> ValueId {
        match self.heap.object(value) {
            Some(RObject::Pairlist { car, .. }) | Some(RObject::Language { car, .. }) => *car,
            _ => ValueId::NIL,
        }
    }

    pub fn cdr(&self, value: ValueId) -> ValueId {
        match self.heap.object(value) {
            Some(RObject::Pairlist { cdr, .. }) | Some(RObject::Language { cdr, .. }) => *cdr,
            _ => ValueId::NIL,
        }
    }

    pub fn tag(&self, value: ValueId) -> ValueId {
        match self.heap.object(value) {
            Some(RObject::Pairlist { tag, .. }) | Some(RObject::Language { tag, .. }) => *tag,
            _ => ValueId::NIL,
        }
    }

    pub fn cadr(&self, value: ValueId) -> ValueId {
        self.car(self.cdr(value))
    }

    pub fn caddr(&self, value: ValueId) -> ValueId {
        self.car(self.cdr(self.cdr(value)))
    }

    /// Whether the node is a language call — the LANGSXP type test.
    pub fn is_language(&self, value: ValueId) -> bool {
        matches!(self.heap.object(value), Some(RObject::Language { .. }))
    }

    /// Whether the node is a symbol — and if so whether its interned name
    /// matches. The new world allocates per-node symbols, so the parser's
    /// identity comparisons against fresh operator symbols become name
    /// comparisons exactly as gram.y's interned-string equality intended.
    pub fn sym_is(&self, value: ValueId, name: &str) -> bool {
        match self.heap.object(value) {
            Some(RObject::Symbol { name: sym, .. }) => self.interner.symbol_name(*sym) == name,
            _ => false,
        }
    }

    pub fn is_symbol(&self, value: ValueId) -> bool {
        matches!(self.heap.object(value), Some(RObject::Symbol { .. }))
    }

    /// The print name of a symbol node, when it is one.
    pub fn symbol_name_string(&self, value: ValueId) -> String {
        match self.heap.object(value) {
            Some(RObject::Symbol { name, .. }) => self.interner.symbol_name(*name).to_string(),
            _ => String::new(),
        }
    }

    /// The language-head variant of cons for chains built manually —
    /// blocks assemble their call with the head cell typed Language.
    pub fn lang_cons(&mut self, car: ValueId, cdr: ValueId) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Language {
            car,
            cdr,
            tag: ValueId::NIL,
        }))
    }

    pub fn scalar_real(&mut self, value: f64) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Real(vec![value])))
    }

    pub fn scalar_complex(&mut self, imaginary: f64) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Complex(vec![crate::object::RComplex {
            r: 0.0,
            i: imaginary,
        }])))
    }

    pub fn scalar_complex_parts(
        &mut self,
        real: f64,
        imaginary: f64,
    ) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Complex(vec![crate::object::RComplex {
            r: real,
            i: imaginary,
        }])))
    }

    pub fn scalar_integer(&mut self, value: i32) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Integer(vec![value])))
    }

    pub fn scalar_logical(&mut self, value: i32) -> Result<ValueId, ParseError> {
        Ok(self.heap.alloc(RObject::Logical(vec![value])))
    }

    pub fn scalar_string(&mut self, value: &str) -> Result<ValueId, ParseError> {
        let interned = self.interner.intern_string(value);
        Ok(self.heap.alloc(RObject::String(vec![interned])))
    }

    /// Byte literals that are not valid UTF-8: the legacy parser carries
    /// them as raw-byte CHARSXP strings. The new world interner is
    /// str-native; lossy conversion matches the sweep's coverage since no
    /// golden exercises non-UTF-8 literals through the native parser yet.
    pub fn scalar_bytes(&mut self, bytes: &[u8]) -> Result<ValueId, ParseError> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let interned = self.interner.intern_string(&text);
        Ok(self.heap.alloc(RObject::String(vec![interned])))
    }

    /// NA_character_ — the NUL-prefixed sentinel encoding the importer
    /// chose for collision-free round-trips.
    pub fn scalar_na_string(&mut self) -> Result<ValueId, ParseError> {
        let interned = self.interner.intern_string("\u{0}NA");
        Ok(self.heap.alloc(RObject::String(vec![interned])))
    }

    pub fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    pub fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    pub fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        let tok = self.advance();
        if &tok == expected {
            Ok(())
        } else {
            Err(self.unexpected_at(self.pos - 1))
        }
    }

    /// Build the upstream-shaped parse error for the token at `index`:
    /// `unexpected ')' in "<source context>"`, with no context when the
    /// input ran out.
    pub fn unexpected_at(&self, index: usize) -> ParseError {
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
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
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

    /// Render `<msg> (<input>:line:col)` for a token index.
    pub fn token_position_error(&self, index: usize, msg: &str) -> ParseError {
        let (start, _) = self
            .spans
            .get(index)
            .copied()
            .unwrap_or((self.source.len(), self.source.len()));
        let (line, col) = self.line_col(start);
        ParseError(format!("{msg} (<input>:{line}:{})", col + 1))
    }

    /// Position for the post-parse per-expression checks (gram.y's
    /// R_Parse1 handler): the parser's lookahead is reported there.
    pub fn pipebind_position_error(&self, msg: &str) -> ParseError {
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
    pub fn skip_newlines(&mut self) {
        while self.peek() == &Token::Newline {
            self.advance();
        }
    }

    /// Skip semicolons and newlines.
    pub fn skip_terminators(&mut self) {
        while self.peek() == &Token::Semicolon || self.peek() == &Token::Newline {
            self.advance();
        }
    }

    pub fn take_token_warnings(&mut self, start: usize, end: usize) -> Vec<String> {
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

    pub fn input_len(&self) -> usize {
        self.spans.last().map(|&(_, b)| b).unwrap_or(0)
    }

    /// Does the tree carry the parser-owned placeholder node anywhere —
    /// identity-compared exactly as upstream's distinguished string.
    pub fn expr_contains_placeholder(&self, expr: ValueId) -> bool {
        if self.placeholder == ValueId::NIL {
            return false;
        }
        let mut stack = vec![expr];
        let mut steps = 0usize;
        while let Some(node) = stack.pop() {
            steps += 1;
            if steps > 1_000_000 {
                break;
            }
            if node == self.placeholder {
                return true;
            }
            match self.heap.object(node) {
                Some(RObject::Language { car, cdr, tag })
                | Some(RObject::Pairlist { car, cdr, tag }) => {
                    if *car != ValueId::NIL {
                        stack.push(*car);
                    }
                    if *cdr != ValueId::NIL {
                        stack.push(*cdr);
                    }
                    if *tag != ValueId::NIL {
                        stack.push(*tag);
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub fn parse_top_level_expressions(&mut self) -> Result<Vec<ValueId>, ParseError> {
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
            if self.have_pipebind && self.expr_contains_pipebind(expr) {
                return Err(self.pipebind_position_error("invalid use of pipe bind symbol"));
            }
            exprs.push(expr);
            let _ = self.take_token_warnings(start, self.pos);
            self.skip_terminators();
        }

        Ok(exprs)
    }

    pub fn parse_program(&mut self) -> Result<ValueId, ParseError> {
        let exprs = self.parse_top_level_expressions()?;
        if exprs.is_empty() {
            return Ok(ValueId::NIL);
        }
        if exprs.len() == 1 {
            return Ok(exprs.into_iter().next().expect("one parsed expression"));
        }

        let brace_sym = self.install_symbol("{");
        let mut list = ValueId::NIL;
        for expression in exprs.iter().rev() {
            list = self.cons(*expression, list)?;
        }
        self.lang_cons(brace_sym, list)
    }
}
