use super::*;

impl<'arena, 'session> Parser<'arena, 'session> {
    // -----------------------------------------------------------------------
    // Expression hierarchy (precedence climbing)
    // -----------------------------------------------------------------------

    pub(crate) fn parse_expr(&mut self) -> Result<SEXP, ParseError> {
        self.parse_help()
    }

    /// Help operator, gram.y's lowest-precedence `%left '?'` tier:
    /// `?topic` (unary) and `topic ? help` (binary, left-associative).
    pub(crate) fn parse_help(&mut self) -> Result<SEXP, ParseError> {
        let mut left = if self.peek() == &Token::Question {
            self.advance();
            self.skip_newlines();
            let operand = self.parse_help_operand()?;
            unsafe {
                let op = Rf_install(c"?".as_ptr());
                self.lang2(op, operand)?
            }
        } else {
            self.parse_assignment()?
        };
        while self.peek() == &Token::Question {
            self.advance();
            self.skip_newlines();
            let right = self.parse_help_rhs()?;
            unsafe {
                let op = Rf_install(c"?".as_ptr());
                left = self.lang3(op, left, right)?;
            }
        }
        Ok(left)
    }

    /// Prefix operand: a chain of leading `?`s (`??x` is `?`(`?`(x))),
    /// otherwise an assignment-level expression. A following binary `?`
    /// binds looser, per the reduce in gram.y.
    pub(crate) fn parse_help_operand(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Question {
            self.advance();
            self.skip_newlines();
            let operand = self.parse_help_operand()?;
            unsafe {
                let op = Rf_install(c"?".as_ptr());
                return self.lang2(op, operand);
            }
        }
        self.parse_assignment()
    }

    /// Binary `?` right-hand side: assignment level, optionally prefixed
    /// by unary `?`s (`a ? ?b` is `` ?`(a, `?`(b)) ``).
    pub(crate) fn parse_help_rhs(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Question {
            return self.parse_help_operand();
        }
        self.parse_assignment()
    }

    /// assignment: tilde (('=' | '<-' | '->' | '<<-') assignment)?
    pub(crate) fn parse_assignment(&mut self) -> Result<SEXP, ParseError> {
        let left = self.parse_tilde()?;

        match self.peek() {
            Token::LeftAssign | Token::Assign | Token::LeftSuper => {
                let op = self.advance().clone();
                self.skip_newlines();
                let right = self.parse_assignment()?;
                unsafe {
                    let op_sym = match op {
                        Token::Assign => Rf_install(c"=".as_ptr()),
                        Token::LeftSuper => Rf_install(c"<<-".as_ptr()),
                        _ => Rf_install(c"<-".as_ptr()),
                    };
                    self.lang3(op_sym, left, right)
                }
            }
            Token::RightAssign => {
                // Chained right assignment: `1 -> x -> y` assigns both x and
                // y, folding left-associatively into nested `<-` calls.
                let mut value = left;
                while self.peek() == &Token::RightAssign {
                    self.advance();
                    self.skip_newlines();
                    let target = self.parse_tilde()?;
                    // x -> y is equivalent to y <- x
                    unsafe {
                        let op_sym = Rf_install(c"<-".as_ptr());
                        value = self.lang3(op_sym, target, value)?;
                    }
                }
                Ok(value)
            }
            _ => Ok(left),
        }
    }

    /// tilde: or ('~' or)*
    pub(crate) fn parse_tilde(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Tilde {
            self.advance();
            self.skip_newlines();
            let right = self.parse_or()?;
            unsafe {
                let op = Rf_install(c"~".as_ptr());
                return self.lang2(op, right);
            }
        }

        let mut left = self.parse_or()?;
        loop {
            // EatLines (gram.y): a newline before a binary operator
            // continues the expression only inside a group (`(`/`[`/`{`).
            // At top level the newline terminates the expression.
            if self.peek() == &Token::Newline
                && !self.inside_nest.get(self.pos).copied().unwrap_or(false)
            {
                break;
            }
            self.skip_newlines();
            if self.peek() == &Token::Tilde {
                self.advance();
                self.skip_newlines();
                let right = self.parse_or()?;
                unsafe {
                    let op = Rf_install(c"~".as_ptr());
                    left = self.lang3(op, left, right)?;
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(crate) fn parse_or(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_and()?;
        loop {
            // EatLines (gram.y): a newline before a binary operator
            // continues the expression only inside a group (`(`/`[`/`{`).
            // At top level the newline terminates the expression.
            if self.peek() == &Token::Newline
                && !self.inside_nest.get(self.pos).copied().unwrap_or(false)
            {
                return Ok(left);
            }
            self.skip_newlines();
            match self.peek() {
                Token::Or2 => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_and()?;
                    unsafe {
                        let op = Rf_install(c"||".as_ptr());
                        left = self.lang3(op, left, right)?;
                    }
                }
                Token::Or => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_and()?;
                    unsafe {
                        let op = Rf_install(c"|".as_ptr());
                        left = self.lang3(op, left, right)?;
                    }
                }
                _ => return Ok(left),
            }
        }
    }

    pub(crate) fn parse_and(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_not()?;
        loop {
            // EatLines (gram.y): a newline before a binary operator
            // continues the expression only inside a group (`(`/`[`/`{`).
            // At top level the newline terminates the expression.
            if self.peek() == &Token::Newline
                && !self.inside_nest.get(self.pos).copied().unwrap_or(false)
            {
                return Ok(left);
            }
            self.skip_newlines();
            match self.peek() {
                Token::And2 => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_not()?;
                    unsafe {
                        let op = Rf_install(c"&&".as_ptr());
                        left = self.lang3(op, left, right)?;
                    }
                }
                Token::And => {
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_not()?;
                    unsafe {
                        let op = Rf_install(c"&".as_ptr());
                        left = self.lang3(op, left, right)?;
                    }
                }
                _ => return Ok(left),
            }
        }
    }

    pub(crate) fn parse_not(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Not {
            self.advance();
            let operand = self.parse_comparison()?;
            unsafe {
                let op = Rf_install(c"!".as_ptr());
                self.lang2(op, operand)
            }
        } else {
            self.parse_comparison()
        }
    }

    /// Comparison operators. gram.y declares these %nonassoc, so after one
    /// comparison a second operator at this level is a syntax error:
    /// `1 < 2 < 3` fails to parse while `(1 < 2) < 3` is fine.
    pub(crate) fn parse_comparison(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_addition()?;
        let op_name = match self.peek() {
            Token::Lt => "<",
            Token::Gt => ">",
            Token::Le => "<=",
            Token::Ge => ">=",
            Token::Eq => "==",
            Token::Ne => "!=",
            _ => return Ok(left),
        };
        self.advance();
        self.skip_newlines();
        let right = self.parse_addition()?;
        let op = self.install_symbol(op_name)?;
        left = self.lang3(op, left, right)?;
        if matches!(
            self.peek(),
            Token::Lt | Token::Gt | Token::Le | Token::Ge | Token::Eq | Token::Ne
        ) {
            // Non-associative chained comparison; the offending token is
            // the second comparison operator at the current position.
            return Err(self.unexpected_at(self.pos));
        }
        Ok(left)
    }

    pub(crate) fn parse_addition(&mut self) -> Result<SEXP, ParseError> {
        if self.peek() == &Token::Tilde {
            self.advance();
            self.skip_newlines();
            let body = self.parse_addition()?;
            unsafe {
                let op = Rf_install(c"~".as_ptr());
                return self.lang2(op, body);
            }
        }
        let mut left = self.parse_multiplication()?;
        loop {
            // EatLines (gram.y): see the group gate in parse_tilde below.
            if self.peek() == &Token::Newline
                && !self.inside_nest.get(self.pos).copied().unwrap_or(false)
            {
                return Ok(left);
            }
            self.skip_newlines();
            let op_name = match self.peek() {
                Token::Plus => "+",
                Token::Minus => "-",
                _ => return Ok(left),
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_multiplication()?;
            let op = self.install_symbol(op_name)?;
            left = self.lang3(op, left, right)?;
        }
    }

    pub(crate) fn parse_multiplication(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_special()?;
        loop {
            // EatLines (gram.y): a newline before a binary operator
            // continues the expression only inside a group (`(`/`[`/`{`).
            // At top level the newline terminates the expression.
            if self.peek() == &Token::Newline
                && !self.inside_nest.get(self.pos).copied().unwrap_or(false)
            {
                return Ok(left);
            }
            self.skip_newlines();
            let op_name = match self.peek() {
                Token::Star => "*".to_string(),
                Token::Slash => "/".to_string(),
                _ => return Ok(left),
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_special()?;
            let op = self.install_symbol(&op_name)?;
            left = self.lang3(op, left, right)?;
        }
    }
    /// Percent-delimited special operators (`%%`, `%/%`, `%in%`, `%*%`,
    /// user-defined `%foo%`) and the native pipe `|>`. gram.y declares
    /// SPECIAL and PIPE at the same %left tier — tighter than `*` / `/`
    /// and `+` / `-`, looser than `:` — so `2 |> f() * 10` parses as
    /// `(2 |> f()) * 10` and `x |> f() |> g()` as `g(f(x))`, while
    /// `1:3 %% 2` still groups the colon first.
    pub(crate) fn parse_special(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_pipebind()?;
        loop {
            // EatLines (gram.y): a newline before a binary operator
            // continues the expression only inside a group (`(`/`[`/`{`).
            // At top level the newline terminates the expression.
            if self.peek() == &Token::Newline
                && !self.inside_nest.get(self.pos).copied().unwrap_or(false)
            {
                return Ok(left);
            }
            self.skip_newlines();
            match self.peek() {
                Token::Percent(name) => {
                    let op_name = name.clone();
                    self.advance();
                    self.skip_newlines();
                    let right = self.parse_pipebind()?;
                    let op = self.install_symbol(&op_name)?;
                    left = self.lang3(op, left, right)?;
                }
                Token::Pipe => {
                    self.advance();
                    self.skip_newlines();
                    let rhs_start = self.pos;
                    let right = self.parse_pipebind()?;
                    left = self.build_pipe(left, right, rhs_start)?;
                }
                _ => return Ok(left),
            }
        }
    }

    /// The `=>` pipe-bind operator: gram.y's `expr PIPEBIND expr`, a
    /// %left tier between SPECIAL/PIPE and `:` — tighter than `|>`, so
    /// `x |> y => log(y)` groups as `x |> (y => log(y))`, and looser than
    /// `+`, so the bind's RHS stops at the first additive operator.
    /// Left-associative: `a => b => c` nests leftward.
    pub(crate) fn parse_pipebind(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_colon()?;
        while self.peek() == &Token::PipeBind {
            let op_index = self.pos;
            self.advance();
            self.skip_newlines();
            let right = self.parse_colon()?;
            left = self.build_pipebind(left, right, op_index)?;
        }
        Ok(left)
    }

    /// Build the call for `lhs => rhs`, porting gram.y's `xxpipebind()`:
    /// a plain `=>(lhs, rhs)` binary call, gated by the
    /// `_R_USE_PIPEBIND_` envvar. Only the `|>` rewrite in `build_pipe`
    /// ever consumes such a call; anything left over fails the post-parse
    /// "invalid use of pipe bind symbol" scan.
    pub(crate) fn build_pipebind(
        &mut self,
        lhs: SEXP,
        rhs: SEXP,
        op_index: usize,
    ) -> Result<SEXP, ParseError> {
        if !pipebind_enabled() {
            // Reported at the `=>` token (gram.y passes &@2). Upstream's
            // location points one past the token's first character.
            return Err(self.token_position_error(
                op_index,
                "'=>' is disabled; set '_R_USE_PIPEBIND_' envvar to a true value to enable it",
            ));
        }
        let op = self.install_symbol("=>")?;
        self.lang3(op, lhs, rhs)
    }

    /// Build the call for `lhs |> rhs`, porting gram.y's `xxpipe()`.
    ///
    /// The RHS must be a function call; a bare symbol is rejected. The LHS
    /// is inserted as the first argument unless a `_` placeholder names the
    /// insertion point:
    /// - a top-level argument `tag = _` is replaced in place — the
    ///   placeholder must be named and may only appear once, and
    /// - an extractor chain like `_[i]`, `_$col`, `_[[i]]`, `_@slot` has
    ///   its placeholder head replaced (R 4.5+).
    ///
    /// A pipe-bind RHS (`x |> y => expr`) rewrites instead into
    /// `(function(y) expr)(x)`; `rhs_start` is the token index where the
    /// pipe's RHS began, for the "RHS variable must be a symbol" location.
    pub(crate) fn build_pipe(
        &mut self,
        lhs: SEXP,
        rhs: SEXP,
        rhs_start: usize,
    ) -> Result<SEXP, ParseError> {
        if unsafe { TYPEOF(rhs) } != SEXPTYPE::LANGSXP {
            return Err(self.token_position_error(
                rhs_start,
                "The pipe operator requires a function call as RHS",
            ));
        }

        // xxpipe's pipe-bind branch, checked before everything else: rewrite
        // `lhs |> var => expr` into `(function(var) expr)(lhs)`.
        let pipebind_sym = self.install_symbol("=>")?;
        if unsafe { CAR(rhs) } == pipebind_sym {
            let var = unsafe { CADR(rhs) };
            let body = unsafe { crate::sexp::accessors::CADDR(rhs) };
            if var.is_null() || unsafe { TYPEOF(var) } != SEXPTYPE::SYMSXP {
                return Err(self.token_position_error(rhs_start, "RHS variable must be a symbol"));
            }
            let nil = unsafe { R_NilValue() };
            // alist = list1(R_MissingArg); SET_TAG(alist, var)
            let formal_cell = self.cons(unsafe { crate::sexp::globals::R_MissingArg() }, nil)?;
            unsafe {
                crate::sexp::accessors::SETTAG(formal_cell, var);
            }
            // fun = lang4(function, alist, expr, R_NilValue)
            let body_cell = self.cons(body, nil)?;
            let formals_cell = self.cons(formal_cell, body_cell)?;
            let fun_sym = self.install_symbol("function")?;
            let fun = self.cons(fun_sym, formals_cell)?;
            if !fun.is_null() {
                unsafe {
                    crate::sexp::accessors::SET_TYPEOF(fun, SEXPTYPE::LANGSXP.as_c_int());
                }
            }
            // lang2(fun, lhs): call the fresh closure on the pipe LHS.
            return self.lang2(fun, lhs);
        }

        let placeholder = self.placeholder;
        unsafe {
            let fun = CAR(rhs);

            // A placeholder in the function position is never substitutable.
            if self.expr_contains_placeholder(fun) {
                return Err(self.token_position_error(
                    rhs_start,
                    "pipe placeholder cannot be used in the RHS function",
                ));
            }

            // Extractor chains: `x |> _[2]`, `d |> _$col`, ...
            if let Some(phcell) = self.find_extractor_placeholder_cell(rhs) {
                let nil = R_NilValue();
                let mut rest = CDR(CDR(rhs));
                while rest != nil {
                    if self.expr_contains_placeholder(CAR(rest)) {
                        return Err(self.token_position_error(
                            rhs_start,
                            "pipe placeholder may only appear once",
                        ));
                    }
                    rest = CDR(rest);
                }
                SETCAR(phcell, lhs);
                return Ok(rhs);
            }

            // A top-level placeholder argument marks the insertion point.
            let nil = R_NilValue();
            let mut cell = CDR(rhs);
            while cell != nil {
                if CAR(cell) == placeholder {
                    let tag = TAG(cell);
                    if tag.is_null() || tag == nil {
                        return Err(self.token_position_error(
                            rhs_start,
                            "pipe placeholder can only be used as a named argument",
                        ));
                    }
                    let mut rest = CDR(cell);
                    while rest != nil {
                        if CAR(rest) == placeholder {
                            return Err(self.token_position_error(
                                rhs_start,
                                "pipe placeholder may only appear once",
                            ));
                        }
                        rest = CDR(rest);
                    }
                    SETCAR(cell, lhs);
                    return Ok(rhs);
                }
                cell = CDR(cell);
            }

            // Screen out syntactically special functions like `/` or `$`.
            if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                let name = symbol_name_string(fun);
                if is_special_rhs_function(&name) {
                    return Err(self.token_position_error(
                        rhs_start,
                        &format!("function '{name}' not supported in RHS call of a pipe"),
                    ));
                }
            }

            // Default: prepend the LHS as the first argument.
            let args = self.cons(lhs, CDR(rhs))?;
            let call = self.cons(fun, args)?;
            if !call.is_null() {
                crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
            }
            Ok(call)
        }
    }

    /// Find the argument cell whose CAR is the placeholder at the head of
    /// an extractor chain (`[`, `[[`, `$`, `@`), mirroring gram.y's
    /// `findExtractorChainPHCell()`. Returns the cell to overwrite with
    /// the pipe LHS.
    pub(crate) fn find_extractor_placeholder_cell(&self, expr: SEXP) -> Option<SEXP> {
        unsafe {
            let fun = CAR(expr);
            let bracket = Rf_install(c"[".as_ptr());
            let bracket2 = Rf_install(c"[[".as_ptr());
            let dollar = Rf_install(c"$".as_ptr());
            let at = Rf_install(c"@".as_ptr());
            if fun != bracket && fun != bracket2 && fun != dollar && fun != at {
                return None;
            }
            let arg1 = CADR(expr);
            if arg1 == self.placeholder {
                Some(CDR(expr))
            } else if TYPEOF(arg1) == SEXPTYPE::LANGSXP {
                self.find_extractor_placeholder_cell(arg1)
            } else {
                None
            }
        }
    }

    /// Whether `expr` contains the pipe placeholder node anywhere,
    /// mirroring gram.y's `checkForPlaceholder()`. Used for the RHS
    /// function slot and for the final per-expression scan.
    pub(crate) fn expr_contains_placeholder(&self, expr: SEXP) -> bool {
        if expr.is_null() || self.placeholder.is_null() {
            return false;
        }
        if expr == self.placeholder {
            return true;
        }
        unsafe {
            let t = TYPEOF(expr);
            if t != SEXPTYPE::LANGSXP && t != SEXPTYPE::LISTSXP {
                return false;
            }
            let nil = R_NilValue();
            let mut cur = expr;
            while cur != nil {
                if self.expr_contains_placeholder(CAR(cur)) {
                    return true;
                }
                cur = CDR(cur);
            }
            false
        }
    }

    /// Power operator: tightest binary operator, right-associative, and
    /// tighter than unary sign, so `2^3^2 == 2^(3^2)` and the exponent may
    /// carry a unary sign (`2^-1`).
    pub(crate) fn parse_power(&mut self) -> Result<SEXP, ParseError> {
        let base = self.parse_postfix()?;
        if self.peek() == &Token::Caret {
            self.advance();
            self.skip_newlines();
            let exp = self.parse_unary()?;
            unsafe {
                let op = Rf_install(c"^".as_ptr());
                self.lang3(op, base, exp)
            }
        } else {
            Ok(base)
        }
    }

    /// Colon operator: x:y (used for sequences like 1:10). Looser than unary
    /// sign but tighter than `*` / `/`, so `-2:3 == (-2):3` while
    /// `2*1:3 == 2*(1:3)`.
    pub(crate) fn parse_colon(&mut self) -> Result<SEXP, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            // EatLines: a newline before a binary operator continues the
            // expression — but only when the operator actually follows.
            // Probe past the newline WITHOUT consuming it: if no colon
            // follows, the newline terminates (an `if` body ending here
            // must still see its newline for the else gate below).
            let before_nl = self.pos;
            self.skip_newlines();
            if self.peek() != &Token::Colon {
                self.pos = before_nl;
                break;
            }
            self.advance();
            self.skip_newlines();
            let right = self.parse_unary()?;
            unsafe {
                let op = Rf_install(c":".as_ptr());
                left = self.lang3(op, left, right)?;
            }
        }
        Ok(left)
    }

    /// Parse a sign chain without changing power precedence.  A second sign
    /// is another unary expression (`--2`), while a normal operand goes
    /// through power first (`-x^2`).
    pub(crate) fn parse_unary_operand(&mut self) -> Result<SEXP, ParseError> {
        if matches!(self.peek(), Token::Minus | Token::Plus | Token::Not) {
            self.parse_unary()
        } else {
            self.parse_power()
        }
    }

    pub(crate) fn parse_unary(&mut self) -> Result<SEXP, ParseError> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                // R's power operator binds tighter than a unary sign:
                // `-x^2` is `-(x^2)`.  parse_power also delegates the
                // exponent back to parse_unary, so `2^-2` remains valid.
                let operand = self.parse_unary_operand()?;
                unsafe {
                    let op = Rf_install(c"-".as_ptr());
                    self.lang2(op, operand)
                }
            }
            Token::Plus => {
                self.advance();
                let operand = self.parse_unary_operand()?;
                unsafe {
                    // stock R keeps the call: `+"a"` errors at runtime
                    // ("invalid argument to unary operator")
                    let op = Rf_install(c"+".as_ptr());
                    self.lang2(op, operand)
                }
            }
            Token::Not => {
                self.advance();
                let operand = self.parse_unary_operand()?;
                unsafe {
                    let op = Rf_install(c"!".as_ptr());
                    self.lang2(op, operand)
                }
            }
            _ => self.parse_power(),
        }
    }
}
