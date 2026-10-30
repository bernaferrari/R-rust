use super::*;

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    Number(f64),
    Complex(f64),
    Int(i32),
    Str(String),
    /// Octal/hex escapes that are not valid UTF-8 (`"\\xf4..."`).
    StrBytes(Vec<u8>),
    Ident(String),
    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Percent(String), // %% or %/% or %in% etc. — the full %...% operator text
    // Comparison
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    // Logical
    And,
    And2,
    Or,
    Or2,
    Not,
    // Pipe
    Pipe,
    // Pipe bind (`=>`, gated by the _R_USE_PIPEBIND_ envvar)
    PipeBind,
    // Pipe placeholder (`_`)
    Placeholder,
    // Assignment
    Assign,      // =
    LeftAssign,  // <-
    RightAssign, // ->
    LeftSuper,   // <<-
    // Help operator: `?topic` (unary) and `topic ? help` (binary)
    Question,
    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LDoubleBracket,
    Comma,
    Semicolon,
    Newline,
    // Special
    Tilde,
    Colon,
    DoubleColon,
    TripleColon,
    Dollar,
    At,
    DotDotDot,
    // Keywords
    KwIf,
    KwElse,
    KwFor,
    KwIn,
    KwWhile,
    KwRepeat,
    KwFunction,
    KwLambda,
    KwBreak,
    KwNext,
    // Eof
    Eof,
    // Malformed numeric lexeme ("0x", "0x1p", "1e") — upstream's
    // lexer-level ERROR, rendered as "unexpected input".
    Invalid,
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

pub(crate) struct Lexer {
    chars: Vec<char>,
    pub(crate) pos: usize,
    /// Position where the most recently returned token starts, i.e. after
    /// leading whitespace and comments — gram.y captures yylloc in
    /// token() right after SkipSpace, so this (not the pre-skip position)
    /// is what upstream location reporting uses.
    pub(crate) last_token_start: usize,
}

impl Lexer {
    pub(crate) fn new(input: &str) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos: 0,
            last_token_start: 0,
        }
    }

    pub(crate) fn peek_char(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    pub(crate) fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    pub(crate) fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    pub(crate) fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            // GNU gram.y SkipSpace: space, tab, form-feed. Newlines stay tokens.
            if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\u{0C}' {
                self.advance();
            } else {
                break;
            }
        }
    }

    pub(crate) fn skip_comment(&mut self) {
        if self.peek_char() == Some('#') {
            while let Some(ch) = self.peek_char() {
                if ch == '\n' || ch == '\r' {
                    break;
                }
                self.advance();
            }
        }
    }
    pub(crate) fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        self.skip_comment();
        self.last_token_start = self.pos;

        let ch = match self.peek_char() {
            Some(c) => c,
            None => return Token::Eof,
        };

        if ch == '\n' {
            self.advance();
            return Token::Newline;
        }
        // A CR terminates a comment (unlike other CR whitespace consumed by
        // skip_whitespace); preserve it as the statement boundary here.
        if ch == '\r' {
            self.advance();
            if self.peek_char() == Some('\n') {
                self.advance();
            }
            return Token::Newline;
        }

        if (ch == 'r' || ch == 'R') && matches!(self.peek_char_at(1), Some('"') | Some('\'')) {
            self.advance();
            return self.read_raw_string();
        }

        if ch == '"' || ch == '\'' {
            return self.read_string();
        }

        if ch == '\\' && self.peek_char_at(1) == Some('(') {
            self.advance();
            return Token::KwLambda;
        }

        // Backtick names
        if ch == '`' {
            return self.read_backtick_name();
        }

        // Numbers: digit or .digit
        if ch.is_ascii_digit()
            || (ch == '.' && self.peek_char_at(1).map_or(false, |c| c.is_ascii_digit()))
        {
            return self.read_number();
        }

        // Identifiers and keywords
        if crate::mainutils::rlocale::identifier_alpha(ch) || ch == '.' || ch == '_' {
            return self.read_ident();
        }

        self.advance();

        match ch {
            '+' => Token::Plus,
            '-' => {
                if self.peek_char() == Some('>') {
                    self.advance();
                    Token::RightAssign
                } else {
                    Token::Minus
                }
            }
            '*' => {
                if self.peek_char() == Some('*') {
                    self.advance();
                    Token::Caret
                } else {
                    Token::Star
                }
            }
            '/' => Token::Slash,
            '^' => Token::Caret,
            '%' => {
                // Custom infix operators: %...%
                // Also handles %% and %/%
                if self.peek_char() == Some('/') && self.peek_char_at(1) == Some('%') {
                    // %/% integer division
                    self.advance(); // skip /
                    self.advance(); // skip %
                    Token::Percent("%/%".to_string())
                } else if self.peek_char() == Some('%') {
                    self.advance();
                    Token::Percent("%%".to_string())
                } else {
                    // Read custom %...% operator
                    let mut op = String::from("%");
                    while let Some(c) = self.peek_char() {
                        if c == '%' {
                            self.advance();
                            op.push('%');
                            break;
                        }
                        op.push(c);
                        self.advance();
                    }
                    Token::Percent(op)
                }
            }
            '<' => {
                if self.peek_char() == Some('-') {
                    self.advance();
                    Token::LeftAssign
                } else if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Le
                } else if self.peek_char() == Some('<') {
                    self.advance();
                    if self.peek_char() == Some('-') {
                        self.advance();
                        Token::LeftSuper
                    } else {
                        Token::Lt // fallback, shouldn't happen
                    }
                } else {
                    Token::Lt
                }
            }
            '>' => {
                if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Ge
                } else {
                    Token::Gt
                }
            }
            '=' => {
                if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Eq
                } else if self.peek_char() == Some('>') {
                    // gram.y: '=' + '>' → PIPEBIND ("=>"), the pipe-bind
                    // symbol; the feature gate is applied at reduce time.
                    self.advance();
                    Token::PipeBind
                } else {
                    Token::Assign
                }
            }
            '!' => {
                if self.peek_char() == Some('=') {
                    self.advance();
                    Token::Ne
                } else {
                    Token::Not
                }
            }
            '&' => {
                if self.peek_char() == Some('&') {
                    self.advance();
                    Token::And2
                } else {
                    Token::And
                }
            }
            '|' => {
                if self.peek_char() == Some('|') {
                    self.advance();
                    Token::Or2
                } else if self.peek_char() == Some('>') {
                    self.advance();
                    Token::Pipe
                } else {
                    Token::Or
                }
            }
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => {
                if self.peek_char() == Some('[') {
                    self.advance();
                    Token::LDoubleBracket
                } else {
                    Token::LBracket
                }
            }
            // A `]` closes one subscript level; `]]` closes two. The lexer
            // must NOT greedily fuse adjacent closes: in `v[c(1,2)[1]]`
            // the inner `[1]` consumes the first `]` and the outer `[...]`
            // consumes the second. Always emit a single close here; the
            // `[[` parse rule consumes two `]` positionally.
            ']' => Token::RBracket,
            ',' => Token::Comma,
            ';' => Token::Semicolon,
            '?' => Token::Question,
            '~' => Token::Tilde,
            ':' => {
                if self.peek_char() == Some(':') {
                    self.advance();
                    if self.peek_char() == Some(':') {
                        self.advance();
                        Token::TripleColon
                    } else {
                        Token::DoubleColon
                    }
                } else {
                    Token::Colon
                }
            }
            '$' => Token::Dollar,
            '@' => Token::At,
            '.' => {
                // Check for ...
                if self.peek_char() == Some('.') && self.peek_char_at(1) == Some('.') {
                    self.advance();
                    self.advance();
                    Token::DotDotDot
                } else {
                    Token::Ident(".".to_string())
                }
            }
            // Never reinterpret an unrecognized source character as EOF:
            // doing so silently accepts the valid prefix of a malformed
            // script and lets its side effects run.
            _ => Token::Invalid,
        }
    }

    pub(crate) fn read_number(&mut self) -> Token {
        let mut s = String::new();
        let mut has_dot = false;
        let mut has_e = false;

        // Hex literals: 0x... (gram.y `NumericValue`). The mantissa takes
        // hex digits and at most one '.', needs at least one character, and
        // may carry a [pP][+-]?[0-9]+ binary exponent (fractions do not
        // require one, and "0x.8p1" / "0x1." are accepted).
        if self.peek_char() == Some('0')
            && (self.peek_char_at(1) == Some('x') || self.peek_char_at(1) == Some('X'))
        {
            let Some(zero) = self.advance() else {
                return Token::Eof;
            };
            let Some(prefix) = self.advance() else {
                return Token::Eof;
            };
            s.push(zero);
            s.push(prefix);
            let mut nd = 0usize;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_hexdigit() {
                    s.push(ch);
                    self.advance();
                    nd += 1;
                } else if ch == '.' && !has_dot {
                    has_dot = true;
                    s.push(ch);
                    self.advance();
                    nd += 1;
                } else {
                    break;
                }
            }
            if nd == 0 {
                // "0x" with no mantissa: upstream lexical error.
                return Token::Invalid;
            }
            if matches!(self.peek_char(), Some('p') | Some('P')) {
                has_e = true;
                s.push(self.advance().unwrap());
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    s.push(self.advance().unwrap());
                }
                let mut ed = 0usize;
                loop {
                    match self.peek_char() {
                        Some(d) if d.is_ascii_digit() => {
                            s.push(d);
                            self.advance();
                            ed += 1;
                        }
                        _ => break,
                    }
                }
                if ed == 0 {
                    // "0x1p" without exponent digits: upstream lexical error.
                    return Token::Invalid;
                }
            }
            // Use the bounded GNU numeric parser for accepted hexadecimal
            // syntax, including integers longer than u64. The lexer above
            // remains responsible for the source-language token boundaries.
            let Some(v) = crate::mainutils::coerce::parse_double_str(&s) else {
                return Token::Invalid;
            };
            if self.peek_char() == Some('i') {
                self.advance();
                return Token::Complex(v);
            }
            if self.peek_char() == Some('L') {
                self.advance();
                return Self::integer_or_float(v, &s, has_dot, has_e);
            }
            return Token::Number(v);
        }

        // Decimal literals. `exp_digits` counts digits after the exponent
        // marker so "1e" / "1e+" can be rejected like upstream.
        let mut exp_digits: Option<usize> = None;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
                if let Some(n) = exp_digits.as_mut() {
                    *n += 1;
                }
            } else if ch == '.' && !has_dot && !has_e {
                has_dot = true;
                s.push(ch);
                self.advance();
            } else if (ch == 'e' || ch == 'E') && !has_e {
                has_e = true;
                has_dot = true;
                s.push(ch);
                self.advance();
                if (self.peek_char() == Some('+') || self.peek_char() == Some('-'))
                    && let Some(sign) = self.advance()
                {
                    s.push(sign);
                }
                exp_digits = Some(0);
            } else {
                // 'L' and everything else ends the literal; the L suffix is
                // handled after the loop (left unconsumed here).
                break;
            }
        }
        if exp_digits == Some(0) {
            // "1e" / "1e+": upstream lexical error.
            return Token::Invalid;
        }

        // NumericValue in GNU gram.y uses R_atof for source literals too.
        // Its long-mantissa rounding and underflow differ from C strtod and
        // Rust's correctly rounded parser; reuse our bounded GNU seam.
        let Some(v) = crate::mainutils::coerce::parse_double_str(&s) else {
            return Token::Invalid;
        };
        if self.peek_char() == Some('i') {
            self.advance();
            if self.peek_char() == Some('L') {
                self.advance();
                return Token::Invalid;
            }
            return Token::Complex(v);
        }
        if self.peek_char() == Some('L') {
            self.advance();
            return Self::integer_or_float(v, &s, has_dot, has_e);
        }
        // Bare decimal literals are double in R and there are no octal
        // source literals ("010" is ten); only the L suffix makes an integer.
        Token::Number(v)
    }

    /// Trunk's L-suffix rule (gram.y `NumericValue`): the literal becomes an
    /// integer only when its value is integral and fits in `int`; otherwise
    /// the numeric value wins, with one of upstream's literal warnings.
    pub(crate) fn integer_or_float(v: f64, lexeme: &str, has_dot: bool, has_e: bool) -> Token {
        let fits = v.is_finite() && v == v.trunc() && i32::try_from(v as i64).is_ok();
        let lexeme = format!("{lexeme}L");
        if fits {
            if has_dot && !has_e {
                warn_literal(&format!(
                    "integer literal {lexeme} contains unnecessary decimal point"
                ));
            }
            return Token::Int(v as i32);
        }
        if has_dot && !has_e {
            warn_literal(&format!(
                "integer literal {lexeme} contains decimal; using numeric value"
            ));
        } else {
            warn_literal(&format!(
                "non-integer value {lexeme} qualified with L; using numeric value"
            ));
        }
        Token::Number(v)
    }

    pub(crate) fn read_raw_string(&mut self) -> Token {
        let quote = match self.advance() {
            Some('"') => '"',
            Some('\'') => '\'',
            _ => return Token::Invalid,
        };

        // Raw constants use matching (), [] or {} delimiters and may put the
        // same number of dashes on both sides. Keep accepting the historical
        // single-quote spelling, while applying the same strict termination
        // rule to it.
        let mut dashes = 0usize;
        while self.peek_char_at(dashes) == Some('-') {
            dashes += 1;
        }
        let Some(open) = self.peek_char_at(dashes) else {
            return Token::Invalid;
        };
        let close = match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            _ => return Token::Invalid,
        };
        for _ in 0..=dashes {
            self.advance();
        }
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(c) if c == close => {
                    let matching_dashes =
                        (0..dashes).all(|offset| self.peek_char_at(offset) == Some('-'));
                    if matching_dashes && self.peek_char_at(dashes) == Some(quote) {
                        for _ in 0..dashes {
                            self.advance();
                        }
                        self.advance();
                        return Token::Str(s);
                    }
                    s.push(c);
                }
                Some(c) => s.push(c),
                None => return Token::Invalid,
            }
        }
    }

    pub(crate) fn take_hex_digits(&mut self, max: usize) -> Option<u32> {
        let mut val = 0u32;
        let mut n = 0usize;
        while n < max {
            match self.peek_char() {
                Some(c) if c.is_ascii_hexdigit() => {
                    self.advance();
                    val = val * 16 + c.to_digit(16).unwrap();
                    n += 1;
                }
                _ => break,
            }
        }
        if n == 0 { None } else { Some(val) }
    }

    /// GNU gram.y `StringValue`: \\a \\b \\f \\n \\r \\t \\v \\\\ \\' \\" \\`
    /// \\  \\<newline>, octal, \\xHH, \\u / \\U. Mix of Unicode and octal/hex
    /// is an error. Nul is an error. The reported column is the character
    /// just consumed, not the opening quote.
    pub(crate) fn bad_escape(&mut self) -> Token {
        if self.pos > 0 {
            self.last_token_start = self.pos - 1;
        }
        Token::Invalid
    }

    pub(crate) fn read_string(&mut self) -> Token {
        let quote = self.advance().unwrap_or('"');
        let mut bytes = Vec::new();
        let mut oct_or_hex = false;
        let mut use_wcs = false;
        loop {
            let Some(c) = self.advance() else {
                return Token::Invalid;
            };
            if c == quote {
                break;
            }
            if c != '\\' {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                continue;
            }
            let Some(esc) = self.advance() else {
                return Token::Invalid;
            };
            match esc {
                'a' => bytes.push(0x07),
                'b' => bytes.push(0x08),
                'f' => bytes.push(0x0c),
                'n' => bytes.push(b'\n'),
                'r' => bytes.push(b'\r'),
                't' => bytes.push(b'\t'),
                'v' => bytes.push(0x0b),
                '\\' | '"' | '\'' | '`' | ' ' | '\n' => {
                    let mut buf = [0u8; 4];
                    bytes.extend_from_slice(esc.encode_utf8(&mut buf).as_bytes());
                }
                '0'..='7' => {
                    if use_wcs {
                        return self.bad_escape();
                    }
                    let mut octal = esc as u32 - '0' as u32;
                    if let Some(d) = self.peek_char() {
                        if ('0'..='7').contains(&d) {
                            self.advance();
                            octal = 8 * octal + d as u32 - '0' as u32;
                            if let Some(e) = self.peek_char() {
                                if ('0'..='7').contains(&e) {
                                    self.advance();
                                    octal = 8 * octal + e as u32 - '0' as u32;
                                }
                            }
                        }
                    }
                    if octal == 0 || octal > 0xff {
                        return self.bad_escape();
                    }
                    bytes.push(octal as u8);
                    oct_or_hex = true;
                }
                'x' => {
                    if use_wcs {
                        return self.bad_escape();
                    }
                    let Some(val) = self.take_hex_digits(2) else {
                        return self.bad_escape();
                    };
                    if val == 0 {
                        return self.bad_escape();
                    }
                    bytes.push(val as u8);
                    oct_or_hex = true;
                }
                'u' | 'U' => {
                    if oct_or_hex {
                        return self.bad_escape();
                    }
                    let max = if esc == 'u' { 4 } else { 8 };
                    let delim = self.peek_char() == Some('{');
                    if delim {
                        self.advance();
                    }
                    let Some(val) = self.take_hex_digits(max) else {
                        return self.bad_escape();
                    };
                    if delim {
                        if self.peek_char() != Some('}') {
                            return self.bad_escape();
                        }
                        self.advance();
                    }
                    if val == 0 {
                        return self.bad_escape();
                    }
                    let Some(ch) = char::from_u32(val) else {
                        return self.bad_escape();
                    };
                    let mut buf = [0u8; 4];
                    bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    use_wcs = true;
                }
                _ => return self.bad_escape(),
            }
        }
        match String::from_utf8(bytes.clone()) {
            Ok(s) => Token::Str(s),
            Err(_) => Token::StrBytes(bytes),
        }
    }

    pub(crate) fn read_backtick_name(&mut self) -> Token {
        self.advance(); // skip `
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\\') => match self.advance() {
                    Some(c) => s.push(c),
                    None => return Token::Invalid,
                },
                Some('`') => break,
                Some(c) => s.push(c),
                None => return Token::Invalid,
            }
        }
        Token::Ident(s)
    }

    pub(crate) fn read_ident(&mut self) -> Token {
        let mut s = String::new();
        while let Some(ch) = self.peek_char() {
            if crate::mainutils::rlocale::identifier_alnum(ch) || ch == '.' || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if (s == "r" || s == "R") && matches!(self.peek_char(), Some('"') | Some('\'')) {
            return self.read_raw_string();
        }

        // A standalone `_` is the pipe placeholder (R 4.2+ `|>`); it is
        // only valid inside the RHS of a pipe and is rejected elsewhere by
        // the post-parse scan.
        if s == "_" {
            return Token::Placeholder;
        }
        if s.starts_with('_') {
            return Token::Invalid;
        }
        // Check keywords
        match s.as_str() {
            "if" => Token::KwIf,
            "else" => Token::KwElse,
            "for" => Token::KwFor,
            "in" => Token::KwIn,
            "while" => Token::KwWhile,
            "repeat" => Token::KwRepeat,
            "function" => Token::KwFunction,
            "break" => Token::KwBreak,
            "next" => Token::KwNext,
            _ => Token::Ident(s),
        }
    }
}
