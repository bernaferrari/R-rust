//! Bounded conversion of interpreter expression trees into owned mathematical labels.
use crate::mainutils::essentials::{base_error, elt_to_string};
use crate::sexp::{
    accessors::*,
    ffi::{SEXP, SEXPTYPE},
    globals::R_NilValue,
};
use r_graphics_engine::{DrawTarget, PlotParameters, Point, math::MathExpr};

#[derive(Clone)]
pub(crate) enum Label {
    Text(String),
    Math(MathExpr),
}
impl Label {
    pub(crate) fn dimensions(
        &self,
        target: &dyn DrawTarget,
        params: &PlotParameters,
    ) -> (f32, f32) {
        match self {
            Self::Text(text) => {
                let m = target.measure_text(text, params);
                (m.width, m.ascent + m.descent)
            }
            Self::Math(expr) => {
                let m = expr.layout(target, params);
                (m.width, m.ascent + m.descent)
            }
        }
    }
    pub(crate) fn draw(
        &self,
        target: &mut dyn DrawTarget,
        position: Point,
        params: &PlotParameters,
    ) {
        match self {
            Self::Text(text) => target.draw_text(text, position, params),
            Self::Math(expr) => expr.layout(target, params).draw(target, position, params),
        }
    }
}
pub(crate) unsafe fn labels(value: SEXP) -> Vec<Label> {
    unsafe {
        if value == R_NilValue() {
            return vec![];
        }
        match SEXPTYPE(TYPEOF(value)) {
            SEXPTYPE::EXPRSXP => (0..XLENGTH(value))
                .map(|i| {
                    let mut budget = 4096;
                    Label::Math(decode(VECTOR_ELT(value, i), 0, &mut budget))
                })
                .collect(),
            SEXPTYPE::LANGSXP | SEXPTYPE::SYMSXP => {
                let mut budget = 4096;
                vec![Label::Math(decode(value, 0, &mut budget))]
            }
            _ => (0..XLENGTH(value))
                .map(|i| Label::Text(elt_to_string(value, i)))
                .collect(),
        }
    }
}
fn greek(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "omicron" => "ο",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "infinity" => "∞",
        "partialdiff" => "∂",
        "nabla" => "∇",
        "degree" => "°",
        "cdot" => "·",
        "ldots" => "…",
        "cdots" => "⋯",
        "vartheta" => "ϑ",
        "varphi" => "ϕ",
        "varsigma" => "ς",
        "aleph" => "ℵ",
        "emptyset" => "∅",
        "exclam" => "!",
        "universal" => "∀",
        "existential" => "∃",
        "suchthat" => "∋",
        "congruent" => "≡",
        "lessequal" => "≤",
        "greaterequal" => "≥",
        "plusminus" => "±",
        "multiply" => "×",
        "divide" => "÷",
        "notequal" => "≠",
        "equivalence" => "≡",
        "approxequal" => "≈",
        "proportional" => "∝",
        "element" => "∈",
        "notelement" => "∉",
        "intersection" => "∩",
        "union" => "∪",
        "propersuperset" => "⊃",
        "reflexsuperset" => "⊇",
        "notsubset" => "⊄",
        "propersubset" => "⊂",
        "reflexsubset" => "⊆",
        "angle" => "∠",
        "logicaland" => "∧",
        "logicalor" => "∨",
        "therefore" => "∴",
        "perpendicular" => "⊥",
        "bullet" | "dotmath" => "⋅",
        "lozenge" => "◊",
        "diamond" => "⋄",
        "club" => "♣",
        "heart" => "♥",
        "spade" => "♠",
        "arrowleft" => "←",
        "arrowright" => "→",
        "arrowup" => "↑",
        "arrowdown" => "↓",
        "arrowboth" => "↔",
        "arrowdblleft" => "⇐",
        "arrowdblright" => "⇒",
        "arrowdblup" => "⇑",
        "arrowdbldown" => "⇓",
        "arrowdblboth" => "⇔",
        "ellipsis" => "…",
        "minute" => "′",
        "second" => "″",
        _ => return None,
    })
}
unsafe fn decode(value: SEXP, depth: usize, budget: &mut usize) -> MathExpr {
    unsafe {
        if depth > 64 || *budget == 0 {
            base_error("plotmath expression exceeds nesting or size limit");
        }
        *budget -= 1;
        if TYPEOF(value) != SEXPTYPE::LANGSXP {
            return match SEXPTYPE(TYPEOF(value)) {
                SEXPTYPE::SYMSXP => {
                    let name = elt_to_string(value, 0);
                    if let Some(symbol) = greek(&name) {
                        MathExpr::Upright(symbol.into())
                    } else {
                        MathExpr::Variable(name)
                    }
                }
                SEXPTYPE::STRSXP
                | SEXPTYPE::CHARSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::LGLSXP
                    if XLENGTH(value) == 1 =>
                {
                    let text = elt_to_string(value, 0);
                    if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::REALSXP {
                        MathExpr::Upright(text)
                    } else {
                        MathExpr::Text(text)
                    }
                }
                _ => base_error("invalid plotmath atom"),
            };
        }
        let op = CAR(value);
        if TYPEOF(op) != SEXPTYPE::SYMSXP {
            base_error("invalid plotmath operator");
        }
        let name = elt_to_string(op, 0);
        let mut args = vec![];
        let mut tail = CDR(value);
        while tail != R_NilValue() {
            if args.len() > 4096 || TYPEOF(tail) != SEXPTYPE::LISTSXP {
                base_error("invalid plotmath arguments");
            }
            args.push(decode(CAR(tail), depth + 1, budget));
            tail = CDR(tail);
        }
        let need = |n| {
            if args.len() != n {
                base_error(format!("plotmath '{name}' requires {n} arguments"));
            }
        };
        match name.as_str() {
            "frac" | "over" | "atop" => {
                need(2);
                let b = args.pop().unwrap();
                let a = args.pop().unwrap();
                if name == "atop" {
                    MathExpr::Atop(Box::new(a), Box::new(b))
                } else {
                    MathExpr::Fraction(Box::new(a), Box::new(b))
                }
            }
            "^" => {
                need(2);
                let sup = Box::new(args.pop().unwrap());
                let base = args.pop().unwrap();
                match base {
                    MathExpr::Scripts { base, sub, .. } => MathExpr::Scripts {
                        base,
                        sub,
                        sup: Some(sup),
                    },
                    base => MathExpr::Scripts {
                        base: Box::new(base),
                        sub: None,
                        sup: Some(sup),
                    },
                }
            }
            "[" => {
                need(2);
                let sub = Box::new(args.pop().unwrap());
                let base = Box::new(args.pop().unwrap());
                MathExpr::Scripts {
                    base,
                    sub: Some(sub),
                    sup: None,
                }
            }
            "sum" | "prod" | "integral" | "union" | "intersect" | "lim" | "liminf" | "limsup"
            | "inf" | "sup" | "min" | "max" => {
                if args.is_empty() || args.len() > 3 {
                    base_error("plotmath sum/product/integral requires 1 to 3 arguments");
                }
                let body = args.remove(0);
                let sub = if args.is_empty() {
                    None
                } else {
                    Some(Box::new(args.remove(0)))
                };
                let sup = if args.is_empty() {
                    None
                } else {
                    Some(Box::new(args.remove(0)))
                };
                let symbol = match name.as_str() {
                    "sum" => "∑",
                    "prod" => "∏",
                    "integral" => "∫",
                    "union" => "∪",
                    "intersect" => "∩",
                    other => other,
                };
                MathExpr::DisplayOperator {
                    symbol: symbol.into(),
                    body: Box::new(body),
                    sub,
                    sup,
                }
            }
            "bgroup" => {
                need(3);
                let right = args.pop().unwrap();
                let body = args.pop().unwrap();
                let left = args.pop().unwrap();
                let delim = |v: &MathExpr| match v {
                    MathExpr::Text(s) | MathExpr::Upright(s) | MathExpr::Variable(s)
                        if matches!(
                            s.as_str(),
                            "" | "." | "(" | ")" | "[" | "]" | "{" | "}" | "|" | "||"
                        ) =>
                    {
                        s.clone()
                    }
                    _ => base_error("plotmath bgroup delimiters must be strings or symbols"),
                };
                MathExpr::BGroup {
                    left: delim(&left),
                    body: Box::new(body),
                    right: delim(&right),
                }
            }
            "group" => {
                need(3);
                let right = args.pop().unwrap();
                let body = args.pop().unwrap();
                let left = args.pop().unwrap();
                if !matches!(left, MathExpr::Text(_) | MathExpr::Upright(_))
                    || !matches!(right, MathExpr::Text(_) | MathExpr::Upright(_))
                {
                    base_error("plotmath group delimiters must be strings or symbols");
                }
                MathExpr::Row(vec![left, body, right])
            }
            "hat" | "tilde" | "dot" | "ring" | "bar" => {
                need(1);
                let accent = match name.as_str() {
                    "hat" => "ˆ",
                    "tilde" => "˜",
                    "dot" => "˙",
                    "ring" => "˚",
                    _ => "¯",
                };
                MathExpr::Accent(Box::new(args.pop().unwrap()), accent.into())
            }
            "widehat" | "widetilde" => {
                need(1);
                MathExpr::WideAccent(
                    Box::new(args.pop().unwrap()),
                    if name == "widehat" { "hat" } else { "tilde" }.into(),
                )
            }
            "underline" => {
                need(1);
                MathExpr::Underline(Box::new(args.pop().unwrap()))
            }
            "sqrt" => {
                need(1);
                MathExpr::Radical(Box::new(args.pop().unwrap()))
            }
            "phantom" => {
                need(1);
                MathExpr::Phantom(Box::new(args.pop().unwrap()))
            }
            "plain" | "bold" | "italic" | "math" | "bolditalic" => {
                need(1);
                use r_graphics_engine::FontFace;
                let face = match name.as_str() {
                    "bold" => FontFace::Bold,
                    "italic" => FontFace::Italic,
                    "bolditalic" => FontFace::BoldItalic,
                    _ => FontFace::Plain,
                };
                MathExpr::Style(Box::new(args.pop().unwrap()), face)
            }
            "*" | "paste" => MathExpr::Row(args),
            "~" => {
                let mut row = vec![];
                for arg in args {
                    if !row.is_empty() {
                        row.push(MathExpr::Space(0.3));
                    }
                    row.push(arg);
                }
                MathExpr::Row(row)
            }
            "(" | "{" => {
                need(1);
                if name == "{" {
                    args.pop().unwrap()
                } else {
                    MathExpr::Row(vec![
                        MathExpr::Text("(".into()),
                        args.pop().unwrap(),
                        MathExpr::Text(")".into()),
                    ])
                }
            }
            "+" | "-" | "/" | ":" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "%=~%" | "%==%"
            | "%~~%" | "%prop%" | "%~%" | "%<->%" | "%<-%" | "%up%" | "%->%" | "%down%"
            | "%<=>%" | "%<=%" | "%dblup%" | "%=>%" | "%dbldown%" | "%supset%" | "%supseteq%"
            | "%notsubset%" | "%subset%" | "%subseteq%" | "%in%" | "%notin%" | "%+-%" | "%*%"
            | "%/%" | "%intersection%" | "%union%" | "%.%" => {
                if args.is_empty() || args.len() > 2 {
                    base_error("invalid plotmath operator arity");
                }
                let symbol = match name.as_str() {
                    "==" => "=",
                    "!=" => "≠",
                    "<=" => "≤",
                    ">=" => "≥",
                    "%+-%" => "±",
                    "%*%" => "×",
                    "%/%" => "÷",
                    "%in%" => "∈",
                    "%notin%" => "∉",
                    "%=~%" => "≅",
                    "%==%" => "≡",
                    "%~~%" => "≈",
                    "%prop%" => "∝",
                    "%~%" => "∼",
                    "%<->%" => "↔",
                    "%<-%" => "←",
                    "%up%" => "↑",
                    "%->%" => "→",
                    "%down%" => "↓",
                    "%<=>%" => "⇔",
                    "%<=%" => "⇐",
                    "%dblup%" => "⇑",
                    "%=>%" => "⇒",
                    "%dbldown%" => "⇓",
                    "%supset%" => "⊃",
                    "%supseteq%" => "⊇",
                    "%notsubset%" => "⊄",
                    "%subset%" => "⊂",
                    "%subseteq%" => "⊆",
                    "%intersection%" => "∩",
                    "%union%" => "∪",
                    "%.%" => "⋅",
                    _ => &name,
                };
                if args.len() == 1 {
                    MathExpr::Row(vec![
                        MathExpr::Text(symbol.into()),
                        MathExpr::Space(1. / 6.),
                        args.pop().unwrap(),
                    ])
                } else {
                    let gap = if name == "/" {
                        0.
                    } else if matches!(
                        name.as_str(),
                        "==" | "!="
                            | "<"
                            | ">"
                            | "<="
                            | ">="
                            | "%=~%"
                            | "%==%"
                            | "%~~%"
                            | "%prop%"
                            | "%~%"
                            | "%<->%"
                            | "%<-%"
                            | "%up%"
                            | "%->%"
                            | "%down%"
                            | "%<=>%"
                            | "%<=%"
                            | "%dblup%"
                            | "%=>%"
                            | "%dbldown%"
                            | "%supset%"
                            | "%supseteq%"
                            | "%notsubset%"
                            | "%subset%"
                            | "%subseteq%"
                            | "%in%"
                            | "%notin%"
                    ) {
                        5. / 18.
                    } else {
                        2. / 9.
                    };
                    MathExpr::Row(vec![
                        args.remove(0),
                        MathExpr::Space(gap),
                        MathExpr::Text(symbol.into()),
                        MathExpr::Space(gap),
                        args.remove(0),
                    ])
                }
            }
            _ => {
                let mut row = vec![
                    MathExpr::Text(name),
                    MathExpr::Scale(Box::new(MathExpr::Text("(".into())), 1.25),
                ];
                for (i, arg) in args.into_iter().enumerate() {
                    if i > 0 {
                        row.push(MathExpr::Text(", ".into()));
                    }
                    row.push(arg);
                }
                row.push(MathExpr::Scale(Box::new(MathExpr::Text(")".into())), 1.25));
                MathExpr::Row(row)
            }
        }
    }
}

#[cfg(test)]
mod oracle_tests {
    use super::*;

    #[test]
    fn decoded_expressions_match_gnu_r_same_font_metrics() {
        let mut session = crate::sexp::session::RSession::new();
        let corpus = [
            ("alpha", "alpha"),
            ("fraction", "frac(alpha[1]^2, sqrt(beta))"),
            ("radical", "sqrt(x)"),
            ("delimiters", "bgroup(\"(\", alpha[1]^2, \")\")"),
            ("sum", "sum(i == 1, n, i^2)"),
            ("integral", "integral(f(x) * dx)"),
        ];
        let expected: Vec<_> = include_str!("../../../../scripts/plotmath_font_oracle.expected")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .collect();
        assert_eq!(expected.len(), corpus.len() * 3);
        for expected in expected {
            let columns: Vec<_> = expected.split_whitespace().collect();
            let name = columns[0];
            let expression = corpus.iter().find(|(key, _)| *key == name).unwrap().1;
            let size: f32 = columns[1].parse().unwrap();
            let width: f32 = columns[2].parse().unwrap();
            let height: f32 = columns[3].parse().unwrap();
            let (w, h) = session.eval_code_with_output_capture_then(
                &format!("expression({expression})"),
                |value, _, _| {
                    let value = value.unwrap();
                    // Evaluation and decoding share the active session and the
                    // result remains rooted until the owned label is built.
                    let label = unsafe { labels(value.as_raw()) }.remove(0);
                    label.dimensions(
                        &r_graphics_engine::Scene::new(300, 200),
                        &PlotParameters {
                            font_size: size,
                            ..Default::default()
                        },
                    )
                },
            );
            assert!(
                (w - width * 72.).abs() < 0.00002,
                "{name} width: {w} != {}",
                width * 72.
            );
            assert!(
                (h - height * 72.).abs() < 0.00002,
                "{name} height: {h} != {}",
                height * 72.
            );
        }
    }
}
