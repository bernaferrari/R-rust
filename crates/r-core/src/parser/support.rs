use super::*;

pub const PARSE_CONTEXT_WINDOW: usize = 256;

pub fn token_display(tok: &Token) -> String {
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

/// Upstream's `StringTrue`: the accepted spellings of a true envvar value.
pub fn string_true(value: &str) -> bool {
    matches!(value, "T" | "True" | "TRUE" | "true")
}

/// gram.y `xxpipebind`'s feature gate: `_R_USE_PIPEBIND_` must hold a true
/// value for `=>` to parse.
pub fn pipebind_enabled() -> bool {
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

impl<'h, 'i> Parser<'h, 'i> {
    /// gram.y `checkForPipeBind`: whether a `=>` call survived anywhere in
    /// the expression tree (recursing through language object CARs only).
    /// Symbols compare by interned name here — the new world allocates
    /// per-node symbol objects rather than singletons.
    pub fn expr_contains_pipebind(&self, expr: ValueId) -> bool {
        let mut stack = vec![expr];
        let mut steps = 0usize;
        while let Some(node) = stack.pop() {
            steps += 1;
            if steps > 1_000_000 {
                break;
            }
            let (car, cdr) = match self.heap.object(node) {
                Some(RObject::Language { car, cdr, .. }) => (*car, *cdr),
                _ => continue,
            };
            if let Some(RObject::Symbol { name, .. }) = self.heap.object(car) {
                if self.interner.symbol_name(*name) == "=>" {
                    return true;
                }
            }
            stack.push(car);
            // Walk the argument chain: each Pairlist cell's car recurses.
            let mut cur = cdr;
            for _ in 0..100_000 {
                if cur == ValueId::NIL {
                    break;
                }
                match self.heap.object(cur) {
                    Some(RObject::Pairlist { car, cdr: next, .. }) => {
                        stack.push(*car);
                        cur = *next;
                    }
                    _ => break,
                }
            }
        }
        false
    }
}

/// Functions upstream marks IS_SPECIAL_SYMBOL (names.c `Spec_name`); gram.y's
/// `check_rhs()` screens these out of pipe RHS calls.
pub fn is_special_rhs_function(name: &str) -> bool {
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
