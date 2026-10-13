use super::*;

// ---------------------------------------------------------------------------
// Parse error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The message is already upstream-shaped ("unexpected ')' in ..."
        // or "unexpected end of input"); top-level renderers wrap it as
        // `Error: <message>` exactly like Rscript.
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse-error context window, mirroring upstream's `PARSE_CONTEXT_SIZE`:
/// how many trailing source characters are considered for the
/// `in "<context>"` suffix.
pub(crate) const PARSE_CONTEXT_WINDOW: usize = 256;

thread_local! {
    static PENDING_LITERAL_WARNINGS: std::cell::RefCell<Vec<String>> =
        std::cell::RefCell::new(Vec::new());
    static PARSED_EXPR_LITERAL_WARNINGS: std::cell::RefCell<Vec<Vec<String>>> =
        std::cell::RefCell::new(Vec::new());
}

/// Emit one of the literal-suffix warnings from gram.y's `NumericValue`
/// (e.g. "non-integer value 0x1p1024L qualified with L; using numeric
/// value"). Queued while the arena is lent; flush after `with_arena`.
pub(crate) fn warn_literal(message: &str) {
    PENDING_LITERAL_WARNINGS.with(|w| w.borrow_mut().push(message.to_string()));
}

pub(crate) fn take_pending_literal_warnings() -> Vec<String> {
    PENDING_LITERAL_WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

pub(crate) fn begin_parsed_expr_warnings() {
    PARSED_EXPR_LITERAL_WARNINGS.with(|w| w.borrow_mut().clear());
}

pub(crate) fn record_parsed_expr_warning_msgs(msgs: Vec<String>) {
    PARSED_EXPR_LITERAL_WARNINGS.with(|w| w.borrow_mut().push(msgs));
}

/// Fire the parse-time literal warnings attached to top-level expression `index`.
pub fn flush_parsed_expr_warnings(index: usize) {
    let msgs = PARSED_EXPR_LITERAL_WARNINGS.with(|w| {
        w.borrow_mut()
            .get_mut(index)
            .map(std::mem::take)
            .unwrap_or_default()
    });
    for message in msgs {
        if let Ok(c) = CString::new(message) {
            unsafe { crate::main::errors::Rf_warning(c.as_ptr()) };
        }
    }
}

/// Fire leftover parse-time literal warnings after the arena lend is released.
pub fn flush_literal_warnings() {
    let msgs = take_pending_literal_warnings();
    for message in msgs {
        if let Ok(c) = CString::new(message) {
            unsafe { crate::main::errors::Rf_warning(c.as_ptr()) };
        }
    }
}

/// How a token appears in an upstream "unexpected X" parse error, per
/// gram.y's `yytname_translations` table (constants and symbols in prose,
/// operators and keywords as quoted source text).
pub(crate) fn token_display(tok: &Token) -> String {
    match tok {
        Token::Number(_) | Token::Complex(_) | Token::Int(_) => "numeric constant".to_string(),
        Token::Str(_) | Token::StrBytes(_) => "string constant".to_string(),
        Token::Ident(_) => "symbol".to_string(),
        Token::LeftAssign => "assignment".to_string(),
        Token::Newline => "end of line".to_string(),
        Token::Eof => "end of input".to_string(),
        Token::Invalid => "input".to_string(),
        Token::Percent(op) => format!("'{op}'"),
        Token::Plus => "'+'".to_string(),
        Token::Minus => "'-'".to_string(),
        Token::Star => "'*'".to_string(),
        Token::Slash => "'/'".to_string(),
        Token::Caret => "'^'".to_string(),
        Token::Lt => "'<'".to_string(),
        Token::Gt => "'>'".to_string(),
        Token::Le => "'<='".to_string(),
        Token::Ge => "'>='".to_string(),
        Token::Eq => "'=='".to_string(),
        Token::Ne => "'!='".to_string(),
        Token::And => "'&'".to_string(),
        Token::And2 => "'&&'".to_string(),
        Token::Or => "'|'".to_string(),
        Token::Or2 => "'||'".to_string(),
        Token::Not => "'!'".to_string(),
        Token::Question => "'?'".to_string(),
        Token::Pipe => "'|>'".to_string(),
        Token::PipeBind => "'=>'".to_string(),
        Token::Placeholder => "'_'".to_string(),
        Token::Assign => "'='".to_string(),
        Token::RightAssign => "'->'".to_string(),
        Token::LeftSuper => "'<<-'".to_string(),
        Token::LParen => "'('".to_string(),
        Token::RParen => "')'".to_string(),
        Token::LBrace => "'{'".to_string(),
        Token::RBrace => "'}'".to_string(),
        Token::RBracket => "']'".to_string(),
        Token::LBracket => "'['".to_string(),
        Token::LDoubleBracket => "'[['".to_string(),
        Token::Comma => "','".to_string(),
        Token::Semicolon => "';'".to_string(),
        Token::Tilde => "'~'".to_string(),
        Token::Colon => "':'".to_string(),
        Token::DoubleColon => "'::'".to_string(),
        Token::TripleColon => "':::'".to_string(),
        Token::Dollar => "'$'".to_string(),
        Token::At => "'@'".to_string(),
        Token::DotDotDot => "'...'".to_string(),
        Token::KwIf => "'if'".to_string(),
        Token::KwElse => "'else'".to_string(),
        Token::KwFor => "'for'".to_string(),
        Token::KwIn => "'in'".to_string(),
        Token::KwWhile => "'while'".to_string(),
        Token::KwRepeat => "'repeat'".to_string(),
        Token::KwFunction => "'function'".to_string(),
        Token::KwLambda => "'\\('".to_string(),
        Token::KwBreak => "'break'".to_string(),
        Token::KwNext => "'next'".to_string(),
    }
}

/// The print name of a symbol as an owned Rust string.
pub(crate) fn symbol_name_string(sym: SEXP) -> String {
    unsafe {
        let chars = CHAR(PRINTNAME(sym));
        if chars.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// Upstream's `StringTrue`: the accepted spellings of a true envvar value.
pub(crate) fn string_true(value: &str) -> bool {
    matches!(value, "T" | "True" | "TRUE" | "true")
}

/// gram.y `xxpipebind`'s feature gate: `_R_USE_PIPEBIND_` must hold a true
/// value for `=>` to parse. Upstream caches the lookup in a function-local
/// static that short-circuits once it has seen a true value; the flag
/// below reproduces that sticky-true behavior process-wide.
pub(crate) fn pipebind_enabled() -> bool {
    use std::sync::atomic::{AtomicI32, Ordering};
    static USE_PIPEBIND: AtomicI32 = AtomicI32::new(0);
    if USE_PIPEBIND.load(Ordering::Relaxed) == 1 {
        return true;
    }
    let enabled = std::env::var("_R_USE_PIPEBIND_")
        .map(|v| string_true(&v))
        .unwrap_or(false);
    if enabled {
        USE_PIPEBIND.store(1, Ordering::Relaxed);
    }
    enabled
}

/// gram.y `checkForPipeBind`: whether a `=>` call survived anywhere in the
/// expression tree (recursing through language object CARs only).
pub(crate) fn expr_contains_pipebind(expr: SEXP) -> bool {
    if expr.is_null() {
        return false;
    }
    unsafe {
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return false;
        }
        let pipebind_sym = Rf_install(c"=>".as_ptr());
        let nil = R_NilValue();
        let mut cur = expr;
        while cur != nil {
            let car = CAR(cur);
            if car == pipebind_sym || expr_contains_pipebind(car) {
                return true;
            }
            cur = CDR(cur);
        }
        false
    }
}

/// Functions upstream marks IS_SPECIAL_SYMBOL (names.c `Spec_name`); gram.y's
/// `check_rhs()` screens these out of pipe RHS calls (`9 |> `/`(3)` is an
/// error, not division).
pub(crate) fn is_special_rhs_function(name: &str) -> bool {
    matches!(
        name,
        "if" | "while"
            | "repeat"
            | "for"
            | "break"
            | "next"
            | "return"
            | "function"
            | "("
            | "{"
            | "+"
            | "-"
            | "*"
            | "/"
            | "^"
            | "%%"
            | "%/%"
            | "%*%"
            | ":"
            | "::"
            | ":::"
            | "?"
            | "|>"
            | "~"
            | "@"
            | "=>"
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "&"
            | "|"
            | "&&"
            | "||"
            | "!"
            | "<-"
            | "<<-"
            | "="
            | "$"
            | "["
            | "[["
            | "$<-"
            | "[<-"
            | "[[<-"
    )
}
