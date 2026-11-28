//! GNU print formatting — the exact text shape `print`/`cat` produce.
//!
//! The conformance corpus is print-driven: cases prove semantics through
//! captured output, so parity is parity of this formatting. The rules
//! implemented here mirror R's `format()` machinery for the migrated types:
//! seven significant digits for reals, scientific notation outside the
//! `%g` exponent window, integral reals without a decimal point, element
//! separators and the `[1]` positional prefix.

use crate::prim::pairlist_parts;
use crate::heap::Heap;
use crate::id::ValueId;
use crate::intern::SymbolInterner;
use crate::object::{RObject, NA_INTEGER, NA_REAL_BITS};

/// Format a real scalar exactly as R's 7-significant-digit default.
/// NA is the dedicated bit pattern; NaN (a different payload) prints as
/// NaN exactly as GNU distinguishes them.
/// GNU's scientific() digit analysis exactly as format.c: the exponent
/// and significant-digit count of one finite nonzero value under the
/// default digits=7 — alpha rounding through the exact power-of-ten
/// scaling, trailing zeros trimming the significant count.
fn gnu_scientific(x: f64) -> (bool, i32, i32, bool) {
    gnu_scientific_d(x, 7)
}

/// The digit-parameterized core of gnu_scientific — coercion paths run
/// the identical analysis at DBL_DIG exactly as coerce.c arranges.
fn gnu_scientific_d(x: f64, digits: i32) -> (bool, i32, i32, bool) {
    // Returns (neg, kpower, nsig, roundingwidens).
    if x == 0.0 {
        return (false, 0, 1, false);
    }
    let neg = x < 0.0;
    let r = x.abs();
    let DIGITS: i32 = digits;
    const TBL: [f64; 23] = [
        1e00, 1e01, 1e02, 1e03, 1e04, 1e05, 1e06, 1e07, 1e08, 1e09,
        1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19,
        1e20, 1e21, 1e22,
    ];
    let mut kp = r.log10().floor() as i32 - DIGITS + 1;
    let mut r_prec = r;
    if kp.abs() <= 22 {
        if kp >= 0 {
            r_prec /= TBL[kp as usize];
        } else {
            r_prec *= TBL[(-kp) as usize];
        }
    } else if kp <= -308 {
        // GNU's denormal-aware path exactly: scaling up by 1e+303 first
        // keeps every intermediate inside the normal range — direct
        // 10^kp evaluation would bottom out in subnormal imprecision.
        r_prec = (r_prec * 1e303) / 10f64.powi(kp + 303);
    } else {
        r_prec /= 10f64.powi(kp);
    }
    if r_prec < TBL[(DIGITS - 1) as usize] {
        r_prec *= 10.0;
        kp -= 1;
    }
    let mut alpha = r_prec.round();
    // nearbyint rounds half-to-even exactly as GNU — f64::round is
    // half-away, so correct the exact-half boundary cases.
    if (r_prec - r_prec.floor() - 0.5).abs() < 1e-9 && (r_prec.floor() as i64) % 2 == 1 {
        // exactly at .5 with odd floor — half-to-even rounds down, not up
        if r_prec.fract() == 0.5 {
            alpha = r_prec.floor();
        }
    }
    let mut nsig = DIGITS;
    for _ in 1..=DIGITS {
        alpha /= 10.0;
        if alpha == alpha.floor() {
            nsig -= 1;
        } else {
            break;
        }
    }
    let mut roundingwidens = false;
    if nsig == 0 && DIGITS > 0 {
        nsig = 1;
        kp += 1;
        roundingwidens = true;
    }
    let kpower = kp + DIGITS - 1;
    (neg, kpower, nsig, roundingwidens)
}

/// GNU's formatReal width decision exactly as format.c: the uniform
/// (width, decimals, scientific-flag) triple for printing a whole real
/// vector — fixed notation whenever its width does not exceed the
/// scientific width plus scipen, trailing decimals uniform across every
/// element. nsmall is zero in the print paths the corpus exercises.
pub fn format_real_params(vals: &[f64]) -> (usize, usize, i32) {
    format_real_params_d(vals, 7)
}

/// formatReal at an explicit digit count exactly as the coercion paths
/// require — DBL_DIG pinning inside StringFromReal.
pub fn format_real_params_d(vals: &[f64], digits: i32) -> (usize, usize, i32) {
    let mut naflag = false;
    let mut nanflag = false;
    let mut posinf = false;
    let mut neginf = false;
    let mut neg = 0i32;
    let mut mnl = i32::MAX;
    let mut mxl = i32::MIN;
    let mut rgt = i32::MIN;
    let mut mxsl = i32::MIN;
    let mut mxns = i32::MIN;
    let mut any_finite = false;
    for &x in vals {
        if !x.is_finite() {
            if x.is_nan() {
                if x.to_bits() == crate::object::NA_REAL_BITS {
                    naflag = true;
                } else {
                    nanflag = true;
                }
            } else if x > 0.0 {
                posinf = true;
            } else {
                neginf = true;
            }
        } else {
            any_finite = true;
            let (n, kpower, nsig, rw) = gnu_scientific_d(x, digits);
            let neg_i = if n { 1 } else { 0 };
            let mut left = kpower + 1;
            if rw {
                left -= 1;
            }
            let sleft = neg_i + if left <= 0 { 1 } else { left };
            let right = nsig - left;
            if n {
                neg = 1;
            }
            rgt = rgt.max(right);
            mxl = mxl.max(left);
            mnl = mnl.min(left);
            mxsl = mxsl.max(sleft);
            mxns = mxns.max(nsig);
        }
    }
    if !any_finite {
        // All non-finite: width answers the widest sentinel alone.
        let mut w = 0i32;
        if naflag && w < 2 {
            w = 2;
        }
        if nanflag && w < 3 {
            w = 3;
        }
        if posinf && w < 3 {
            w = 3;
        }
        if neginf && w < 4 {
            w = 4;
        }
        return (w.max(0) as usize, 0, 0);
    }
    if rgt < 0 {
        rgt = 0;
    }
    if mxl < 0 {
        mxsl = 1 + neg;
    }
    let mut w_f = mxsl + rgt + if rgt != 0 { 1 } else { 0 };
    let mut e = if mxl > 100 || mnl <= -99 { 2 } else { 1 };
    let mut d = mxns - 1;
    let mut w = neg + if d > 0 { 1 } else { 0 } + d + 4 + e;
    if w_f <= w + 0 {
        // scipen = 0 exactly as the default print options pin.
        e = 0;
        w = w_f;
        d = rgt;
    }
    let _ = &mut w_f;
    let _ = &mut e;
    if naflag && w < 2 {
        w = 2;
    }
    if nanflag && w < 3 {
        w = 3;
    }
    if posinf && w < 3 {
        w = 3;
    }
    if neginf && w < 4 {
        w = 4;
    }
    (w.max(0) as usize, d.max(0) as usize, e)
}

/// GNU's EncodeReal0 exactly as printutils.c: fixed notation carrying
/// precisely d decimals — trailing zeros KEPT, the uniformity being the
/// point — or scientific notation with the C printf %#e shape: d decimal
/// digits, a signed exponent of at least two digits. Everything right-
/// justifies inside the width.
pub fn encode_real(x: f64, w: usize, d: usize, e: i32) -> String {
    let body = if x.is_nan() {
        if x.to_bits() == crate::object::NA_REAL_BITS {
            "NA".to_string()
        } else {
            "NaN".to_string()
        }
    } else if x.is_infinite() {
        if x > 0.0 { "Inf".to_string() } else { "-Inf".to_string() }
    } else if e != 0 {
        // C's %#w.de shape: mantissa with exactly d decimals — zeros
        // kept — then e, the sign, and at least two exponent digits.
        // Rust's {:.*e} rounding is correctly rounded even in subnormal
        // territory where manual scaling loses precision; the exponent
        // reshapes to printf form after.
        let x = if x == 0.0 { 0.0 } else { x };
        let raw = format!("{:.*e}", d, x);
        let (mantissa_part, exp_part) = raw.split_once('e').unwrap_or((raw.as_str(), "0"));
        let exp_value: i32 = exp_part.parse().unwrap_or(0);
        format!("{}e{}{:02}", mantissa_part, if exp_value < 0 { "-" } else { "+" }, exp_value.abs())
    } else {
        let x = if x == 0.0 { 0.0 } else { x };
        format!("{:.*}", d, x)
    };
    if body.len() >= w {
        body
    } else {
        format!("{}{}", " ".repeat(w - body.len()), body)
    }
}

/// GNU's coercion rendering exactly as StringFromReal: the scalar
/// formats through formatReal at the given digit count — 15 for
/// as.character — trailing zeros trimmed through the significant-digit
/// analysis rather than the fixed width.
pub fn format_real_scalar_d(v: f64, digits: i32) -> String {
    if v.is_nan() {
        return if v.to_bits() == crate::object::NA_REAL_BITS {
            "NA".to_string()
        } else {
            "NaN".to_string()
        };
    }
    if v.is_infinite() {
        return if v > 0.0 { "Inf".to_string() } else { "-Inf".to_string() };
    }
    let (w, d, e) = format_real_params_d(&[v], digits);
    encode_real(v, w, d, e).trim_start().to_string()
}

pub fn format_real(v: f64) -> String {
    if v.is_nan() {
        return if v.to_bits() == NA_REAL_BITS {
            "NA".to_string()
        } else {
            "NaN".to_string()
        };
    }
    if v.is_infinite() {
        return if v > 0.0 { "Inf".to_string() } else { "-Inf".to_string() };
    }
    if v == 0.0 {
        return "0".to_string();
    }
    // GNU format.c's formatReal exactly: fixed notation whenever its
    // width does not exceed scientific width plus scipen — a width
    // comparison, not an exponent threshold. digits=7 and scipen=0
    // are the pinned defaults.
    let (_neg, kpower, nsig) = scientific_parts(v);
    let left = kpower + 1;
    let right = nsig - left;
    let rgt = right.max(0);
    let _d = nsig - 1;
    if needs_scientific(v) {
        // Scientific exactly as GNU: nsig significant digits in the
        // mantissa, two-digit minimum exponent.
        let mantissa = v / 10f64.powi(kpower);
        // Rounding to nsig significant digits can carry into the next
        // power exactly as GNU — the exponent shifts with it.
        let mut mantissa_text = trim_fixed(mantissa, (nsig - 1).max(0) as usize);
        let mut kpower = kpower;
        if mantissa_text.parse::<f64>().unwrap_or(0.0).abs() >= 10.0 {
            mantissa_text = trim_fixed(mantissa / 10.0, (nsig - 1).max(0) as usize);
            kpower += 1;
        }
        let sign = if kpower < 0 { "-" } else { "+" };
        format!("{}e{}{:02}", mantissa_text, sign, kpower.abs())
    } else {
        trim_fixed(v, rgt as usize)
    }
}

/// GNU format.c's scientific(): (neg, kpower, nsig) with nsig the
/// lesser of digits=7 and the shortest round-trip significant count.
fn scientific_parts(v: f64) -> (i32, i32, i32) {
    let neg = if v < 0.0 { 1 } else { 0 };
    // GNU rounds to digits significant figures FIRST — carry-over
    // changes both the exponent and the significant count exactly as
    // 999999999999 rounding into 1e12 demonstrates.
    let abs = v.abs();
    let rough_exp = abs.log10().floor() as i32;
    let rounded: f64 = format!("{:.*e}", 6, abs)
        .parse()
        .unwrap_or(abs);
    let _ = rough_exp;
    let s = format!("{:e}", rounded);
    let (mantissa, exponent) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    let kpower: i32 = exponent.parse().unwrap_or(0);
    let nsig_actual = mantissa.chars().filter(|c| c.is_ascii_digit()).count() as i32;
    let nsig = 7.min(nsig_actual).max(1);
    (neg, kpower, nsig)
}

/// Fixed-notation formatting with trailing zeros and a trailing decimal
/// point trimmed — GNU drops both: 2.0 prints as "2", 0.10 as "0.1".
fn trim_fixed(v: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, v);
    if text.contains('.') {
        let trimmed = text.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        text
    }
}

/// The decimals a fixed-notation element actually uses after trailing-zero
/// trimming — the input to GNU's uniform-decimal computation.
/// One complex element's display text — real part, explicit imaginary
/// sign, magnitude, trailing i exactly as GNU's complex printing; an NA
/// in either part renders the whole element NA.
pub fn format_complex(z: crate::object::RComplex) -> String {
    if z.r.to_bits() == crate::object::NA_REAL_BITS
        || z.i.to_bits() == crate::object::NA_REAL_BITS
    {
        return "NA".to_string();
    }
    let sign = if z.i.is_sign_negative() && !z.i.is_nan() { "-" } else { "+" };
    format!("{}{}{}{}", format_real(z.r), sign, format_real(z.i.abs()), "i")
}

/// GNU's fixed-vs-scientific decision exactly as formatReal: fixed
/// whenever its width does not exceed scientific width (scipen = 0).
fn needs_scientific(v: f64) -> bool {
    if !v.is_finite() || v == 0.0 {
        return false;
    }
    let (neg, kpower, nsig) = scientific_parts(v);
    let left = kpower + 1;
    let sleft = neg + if left <= 0 { 1 } else { left };
    let right = nsig - left;
    let rgt = right.max(0);
    let w_f = sleft + rgt + i32::from(rgt != 0);
    let e = if left > 100 || left <= -99 { 2 } else { 1 };
    let d = nsig - 1;
    let w_e = neg + i32::from(d > 0) + d + 4 + e;
    w_f > w_e
}

fn decimals_used(v: f64) -> Option<usize> {
    if v.is_nan() || v.is_infinite() || v == 0.0 {
        return Some(0);
    }
    if needs_scientific(v) {
        return None;
    }
    // Fixed-notation decimals exactly as formatReal's rgt — trailing
    // zeros trimmed after rendering at the computed precision.
    let (_, kpower, nsig) = scientific_parts(v);
    let left = kpower + 1;
    let rgt = (nsig - left).max(0) as usize;
    let text = format!("{:.*}", rgt, v);
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    Some(trimmed.split('.').nth(1).map(|s| s.len()).unwrap_or(0))
}

/// Fixed-notation formatting at an exact decimal count — no trimming,
/// since uniformity is the point.
fn fixed_exact(v: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, v)
}

/// One element's display text — the per-type shapes GNU's print uses.
fn format_element(_heap: &Heap, interner: &SymbolInterner, value: &RObject, index: usize) -> String {
    match value {
        RObject::Real(values) => format_real(values[index]),
        RObject::Integer(values) | RObject::Logical(values) => {
            let v = values[index];
            if v == NA_INTEGER {
                "NA".to_string()
            } else if matches!(value, RObject::Logical(_)) {
                if v != 0 {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            } else {
                v.to_string()
            }
        }
        RObject::Complex(values) => format_complex(values[index]),
        RObject::Raw(values) => format!("{:02x}", values[index]),
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                "NA".to_string()
            } else {
                // GNU escapes embedded quotes and backslashes inside the
                // surrounding quotes exactly as print.c does.
                let escaped = text
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\t', "\\t")
                    .replace('\r', "\\r");
                format!("\"{}\"", escaped)
            }
        }
        _ => "?".to_string(),
    }
}

/// Format a vector as print's output text — `[1]` prefix, space-separated
/// elements, one line. Multi-line width wrapping is outside the slice;
/// conformance vectors are short enough that the single-line shape covers
/// them.
/// table class membership exactly as the factor helper.
fn is_table(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match crate::attrib::get_attrib(heap, interner, value, "class") {
        Some(class_id) => crate::attrib::string_vec_pub(heap, interner, class_id)
            .iter()
            .any(|c| c == "table"),
        None => false,
    }
}


/// Render the attribute chain as GNU's attr(,"name") blocks — insertion
/// order, each value formatted inline exactly as print.default does.
fn format_attributes(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let attrib = heap.get(value).map(|slot| slot.attrib).unwrap_or(crate::id::ValueId::NIL);
    let mut out = String::new();
    let mut cursor = attrib;
    for _ in 0..1000 {
        if cursor == crate::id::ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                let name = match heap.object(*tag) {
                    Some(RObject::Symbol { name, .. }) => deparse_name(interner.symbol_name(*name)),
                    _ => String::new(),
                };
                if !name.is_empty() && name != "names" {
                    let rendered = format_value(heap, interner, *car);
                    out.push_str(&format!("attr(,\"{}\")\n{}\n", name, rendered));
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    out.trim_end().to_string()
}

pub fn format_value(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let object = match heap.object(value) {
        Some(o) => o,
        None => return "NULL".to_string(),
    };
    // Language objects and bare symbols print deparsed — the source
    // reconstruction exactly as GNU's print.language, no [1] prefix.
    if matches!(object, RObject::Language { .. } | RObject::Symbol { .. }) {
        return deparse(heap, interner, value);
    }
    // Expression vectors print inside the expression() wrapper with each
    // element deparsed comma-separated exactly as GNU's print.expression.
    if let RObject::Expression(items) = object {
        if items.is_empty() {
            return "expression()".to_string();
        }
        let parts: Vec<String> = items
            .iter()
            .map(|item| deparse(heap, interner, *item))
            .collect();
        return format!("expression({})", parts.join(", "));
    }
    // factor printing: classed integer codes display as their level labels
    // unquoted, with the Levels line trailing exactly as print.factor.
    if matches!(object, RObject::Integer(_)) && is_factor(heap, interner, value) {
        return format_factor(heap, interner, value);
    }
    // Condition printing exactly as print.condition: the angle-bracket
    // rendering `<class in <call>: message>` — the call clause dropping
    // when NULL exactly as GNU — the class answering the first class.
    if let RObject::List(_) = object {
        let classes: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "class") {
            Some(id) => crate::attrib::string_vec_pub(heap, interner, id),
            None => Vec::new(),
        };
        if classes.iter().any(|c| c == "condition") {
            let field = |name: &str| -> Option<ValueId> {
                let names_id = crate::attrib::get_attrib(heap, interner, value, "names")?;
                let names = crate::attrib::string_vec_pub(heap, interner, names_id);
                let index = names.iter().position(|n| n == name)?;
                match heap.object(value) {
                    Some(RObject::List(items)) => items.get(index).copied(),
                    _ => None,
                }
            };
            let message = field("message")
                .and_then(|id| match heap.object(id) {
                    Some(RObject::String(vals)) if !vals.is_empty() => {
                        Some(interner.string_text(vals[0]).to_string())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            let call_clause = match field("call") {
                Some(id) if id != ValueId::NIL && !matches!(heap.object(id), Some(RObject::Nil)) => {
                    format!(" in {}", deparse(heap, interner, id))
                }
                _ => String::new(),
            };
            let class = classes.first().cloned().unwrap_or_else(|| "condition".to_string());
            return format!("<{}{}: {}>", class, call_clause, message);
        }
    }
    // summaryDefault printing: GNU's two-row table — statistic names as
    // the header row, formatted values below, every column right-justified
    // to a uniform table-wide width with single-space separators.
    if matches!(object, RObject::Real(_) | RObject::Integer(_) | RObject::String(_))
        && is_summary_default(heap, interner, value)
    {
        let headers: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "names") {
            Some(n) => crate::attrib::string_vec_pub(heap, interner, n),
            None => Vec::new(),
        };
        // Numeric cells format as a uniform-decimal group exactly as GNU's
        // zapsmall-printed statistics; a trailing NAs count stays integer,
        // and character-valued cells (the logical mode row) pass through.
        let cells: Vec<String> = match object {
            RObject::Real(vals) => {
                let stat_len = if headers.last().map(|h| h == "NAs").unwrap_or(false) {
                    vals.len().saturating_sub(1)
                } else {
                    vals.len()
                };
                let stats = &vals[..stat_len.min(vals.len())];
                // Decimals needed: the maximum minimal decimal count across
                // finite statistics, trailing zeros trimmed per value.
                let decimals_needed = |v: f64, digits: usize| -> usize {
                    for d in 0..digits {
                        if (v * 10f64.powi(d as i32)).fract().abs() < 1e-9 {
                            return d;
                        }
                    }
                    digits
                };
                let max_decimals = stats
                    .iter()
                    .filter(|v| v.is_finite())
                    .map(|v| decimals_needed(*v, 4))
                    .max()
                    .unwrap_or(0);
                let mut out: Vec<String> = stats
                    .iter()
                    .map(|v| {
                        if v.is_nan() {
                            if v.to_bits() == crate::object::NA_REAL_BITS {
                                "NA".to_string()
                            } else {
                                "NaN".to_string()
                            }
                        } else if v.is_infinite() {
                            if *v > 0.0 { "Inf".to_string() } else { "-Inf".to_string() }
                        } else {
                            format!("{:.*}", max_decimals, v)
                        }
                    })
                    .collect();
                for v in &vals[stat_len.min(vals.len())..] {
                    out.push(if v.is_nan() { "NA".to_string() } else { format!("{}", *v as i64) });
                }
                out
            }
            RObject::Integer(vals) => vals
                .iter()
                .map(|v| {
                    if *v == crate::object::NA_INTEGER {
                        "NA".to_string()
                    } else {
                        v.to_string()
                    }
                })
                .collect(),
            RObject::String(vals) => vals.iter().map(|v| interner.string_text(*v).to_string()).collect(),
            _ => Vec::new(),
        };
        let width = headers
            .iter()
            .chain(cells.iter())
            .map(|s| s.len())
            .max()
            .unwrap_or(0);
        let header_row: Vec<String> = headers.iter().map(|h| format!("{:>width$}", h, width = width)).collect();
        let value_row: Vec<String> = cells.iter().map(|c| format!("{:>width$}", c, width = width)).collect();
        return format!("{}\n{}", header_row.join(" "), value_row.join(" "));
    }
    // table printing: the category header row above the count row, columns
    // right-aligned to the wider of the two exactly as print.table.
    if matches!(object, RObject::Integer(_)) && is_table(heap, interner, value) {
        let counts: Vec<i32> = match object {
            RObject::Integer(vals) => vals.clone(),
            _ => Vec::new(),
        };
        let categories: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "dimnames")
            .and_then(|dn| match heap.object(dn) {
                Some(RObject::List(items)) => items.first().copied(),
                _ => None,
            })
            .map(|cats| crate::attrib::string_vec_pub(heap, interner, cats))
        {
            Some(c) if !c.is_empty() => c,
            _ => match crate::attrib::get_attrib(heap, interner, value, "names") {
                Some(n) => crate::attrib::string_vec_pub(heap, interner, n),
                None => Vec::new(),
            },
        };
        // GNU's named-vector discipline exactly as printvector.c: one
        // uniform width across the whole table — the wider of the
        // formatted counts and the category labels — every cell right-
        // justified inside it and followed by the single-space gap. The
        // dimnames dimension name prints as the leading title line, an
        // empty name yielding the blank line the goldens pin.
        let mut width = 0usize;
        for count in counts.iter() {
            width = width.max(count.to_string().len());
        }
        for cat in categories.iter() {
            width = width.max(cat.len());
        }
        let mut header = String::new();
        let mut body = String::new();
        for (index, count) in counts.iter().enumerate() {
            let cat = categories.get(index).cloned().unwrap_or_default();
            header.push_str(&format!("{:>width$} ", cat, width = width));
            body.push_str(&format!("{:>width$} ", count, width = width));
        }
        // The title line exactly as printNamedVector — names(dimnames)[1]
        // printing before the layout whenever present, the table's empty
        // dimension name drawing the blank line.
        let title = match crate::attrib::get_attrib(heap, interner, value, "dimnames") {
            Some(dn) => crate::attrib::get_attrib(heap, interner, dn, "names")
                .and_then(|n| match heap.object(n) {
                    Some(RObject::String(vals)) => vals.first().map(|v| interner.string_text(*v).to_string()),
                    _ => None,
                }),
            None => None,
        };
        let title_line = title.map(|t| format!("{}\n", t)).unwrap_or_default();
        return format!("{}{}\n{}", title_line, header.trim_end(), body.trim_end());
    }
    // POSIXct printing: classed epoch seconds display as quoted datetimes
    // carrying the tzone suffix exactly as print.POSIXct.
    if matches!(object, RObject::Real(_) | RObject::Integer(_))
        && crate::attrib::is_posixct_classed(heap, interner, value)
    {
        let secs: Vec<f64> = match object {
            RObject::Real(vals) => vals.clone(),
            RObject::Integer(vals) => vals
                .iter()
                .map(|&v| {
                    if v == crate::object::NA_INTEGER {
                        f64::from_bits(crate::object::NA_REAL_BITS)
                    } else {
                        v as f64
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        if secs.is_empty() {
            return "POSIXct of length 0".to_string();
        }
        let tz = crate::attrib::get_attrib(heap, interner, value, "tzone")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) if !vals.is_empty() => {
                    Some(interner.string_text(vals[0]).to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "UTC".to_string());
        // GNU drops the time component only when every finite element is
        // exactly midnight — a single non-midnight renders all times
        // exactly as format.POSIXct's collective omission rule.
        let all_midnight = secs
            .iter()
            .filter(|s| s.is_finite())
            .all(|&s| (s as i64).rem_euclid(86400) == 0);
        let elements: Vec<String> = secs
            .iter()
            .map(|&s| {
                if s.is_nan() {
                    "NA".to_string()
                } else {
                    let full = crate::civil::format_iso_datetime(s as i64);
                    let rendered = if all_midnight && full.ends_with(" 00:00:00") {
                        full[..10].to_string()
                    } else {
                        full
                    };
                    format!("\"{} {}\"", rendered, tz)
                }
            })
            .collect();
        let width = elements.iter().map(|e| e.len()).max().unwrap_or(0);
        let mut out = String::from("[1]");
        for element in &elements {
            out.push(' ');
            out.push_str(&" ".repeat(width.saturating_sub(element.len())));
            out.push_str(element);
        }
        return out;
    }
    // Date printing: classed day counts display as quoted ISO strings
    // exactly as print.Date — a character-vector rendering of the civil
    // dates.
    if matches!(object, RObject::Real(_) | RObject::Integer(_))
        && crate::attrib::is_date_classed(heap, interner, value)
    {
        let days: Vec<f64> = match object {
            RObject::Real(vals) => vals.clone(),
            RObject::Integer(vals) => vals
                .iter()
                .map(|&v| {
                    if v == crate::object::NA_INTEGER {
                        f64::from_bits(crate::object::NA_REAL_BITS)
                    } else {
                        v as f64
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let elements: Vec<String> = days
            .iter()
            .map(|&d| {
                if d.is_nan() {
                    "NA".to_string()
                } else {
                    format!("\"{}\"", crate::civil::format_iso(d as i64))
                }
            })
            .collect();
        if elements.is_empty() {
            return "Date of length 0".to_string();
        }
        let width = elements.iter().map(|e| e.len()).max().unwrap_or(0);
        let mut out = String::from("[1]");
        for element in &elements {
            out.push(' ');
            out.push_str(&" ".repeat(width.saturating_sub(element.len())));
            out.push_str(element);
        }
        return out;
    }
    // difftime printing: the unclassed numeric prints inside the "Time
    // difference of N units" phrasing exactly as print.difftime.
    if matches!(object, RObject::Real(_) | RObject::Integer(_)) && is_difftime(heap, interner, value) {
        let nums: Vec<f64> = match object {
            RObject::Real(vals) => vals.clone(),
            RObject::Integer(vals) => vals.iter().map(|&v| v as f64).collect(),
            _ => Vec::new(),
        };
        let first = nums.first().copied().unwrap_or(0.0);
        let unit = crate::attrib::get_attrib(heap, interner, value, "units")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) if !vals.is_empty() => {
                    Some(interner.string_text(vals[0]).to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "days".to_string());
        let amount = if first.fract() == 0.0 && !first.is_nan() {
            format!("{}", first as i64)
        } else {
            format_real(first)
        };
        return format!("Time difference of {amount} {unit}");
    }
    // data.frame printing: classed lists lay out as tables — column names
    // right-justified over per-column widths, row names on the left,
    // character cells unquoted exactly as print.data.frame.
    if matches!(object, RObject::List(_)) && is_data_frame(heap, interner, value) {
        return format_data_frame(heap, interner, value);
    }
    let length = match object {
        RObject::Real(v) => v.len(),
        RObject::Integer(v) => v.len(),
        RObject::Logical(v) => v.len(),
        RObject::String(v) => v.len(),
        RObject::Complex(v) => v.len(),
        RObject::Raw(v) => v.len(),
        RObject::Nil => return "NULL".to_string(),
        RObject::List(items) if items.is_empty() && !is_data_frame(heap, interner, value) => {
            // GNU prints the bare empty list as list() exactly — no
            // recursion layout applies to zero elements.
            return "list()".to_string();
        }
        RObject::List(items) => {
            // List printing in GNU is a recursive [[i]] layout — the slice
            // prints the flat element count shape for scalarness checks.
            // Named elements print under `$name` headers; unnamed ones keep
            // the positional [[i]] form — GNU's mixed-header rule.
            let names = crate::attrib::get_attrib(heap, interner, value, "names");
            let names_vec: Option<Vec<String>> = names.and_then(|id| match heap.object(id) {
                Some(RObject::String(values)) => Some(
                    values
                        .iter()
                        .map(|v| interner.string_text(*v).to_string())
                        .collect(),
                ),
                _ => None,
            });
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                let name = names_vec
                    .as_ref()
                    .and_then(|v| v.get(i))
                    .filter(|n| !n.is_empty() && *n != "\0NA");
                match name {
                    Some(n) => out.push_str(&format!("${n}\n")),
                    None => out.push_str(&format!("[[{}]]\n", i + 1)),
                }
                out.push_str(&format_value(heap, interner, *item));
                // GNU separates list elements with a blank line.
                out.push_str("\n\n");
            }
            return out.trim_end().to_string();
        }
        RObject::Closure { .. } => return "<function>".to_string(),
        RObject::Environment { .. } => {
            // Named environments print their name exactly as GNU's
            // <environment: R_GlobalEnv> — the name rides the attribute
            // chain in the new world.
            let name = crate::attrib::get_attrib(heap, interner, value, "name")
                .and_then(|id| match heap.object(id) {
                    Some(RObject::String(values)) if !values.is_empty() => {
                        Some(interner.string_text(values[0]).to_string())
                    }
                    _ => None,
                });
            return match name {
                Some(n) if !n.is_empty() => format!("<environment: {n}>"),
                _ => "<environment>".to_string(),
            };
        }
        _ => return "<object>".to_string(),
    };
    if length == 0 {
        // GNU prefixes "named" when the empty vector carries a names
        // attribute exactly as print.default's empty-shape rendering.
        let named = crate::attrib::get_attrib(heap, interner, value, "names").is_some();
        let prefix = if named { "named " } else { "" };
        let base = match object {
            RObject::Real(_) => format!("{prefix}numeric(0)"),
            RObject::Integer(_) => format!("{prefix}integer(0)"),
            RObject::Logical(_) => format!("{prefix}logical(0)"),
            RObject::String(_) => format!("{prefix}character(0)"),
            RObject::Complex(_) => format!("{prefix}complex(0)"),
            RObject::Raw(_) => format!("{prefix}raw(0)"),
            RObject::List(_) => return "list()".to_string(),
            _ => "NULL".to_string(),
        };
        // Attributes print after the empty shape exactly as GNU's
        // attr-bearing vectors — the chain walks in insertion order.
        let attrs = format_attributes(heap, interner, value);
        if attrs.is_empty() {
            return base;
        }
        return format!("{base}\n{attrs}");
    }
    // Matrix layout: a two-length integer dim attribute lays the vector out
    // column-major under [,j] column headers and [i,] row labels exactly as
    // GNU's print.matrix does.
    if let Some(dim) = dim_attribute(heap, interner, value)
        && dim.len() == 2 && dim[0] > 0 && dim[1] > 0 && dim[0] * dim[1] == length as i32 {
            let (row_names, col_names) = dimnames_of(heap, interner, value);
            return format_matrix_inner(
                heap,
                interner,
                object,
                dim[0] as usize,
                dim[1] as usize,
                row_names,
                col_names,
            );
        }
    // GNU right-justifies every element to the widest element text so the
    // columnar reading holds — NA padding is the visible consequence.
    // Real vectors share one decimal count across all elements — GNU's
    // format.c uniformity rule: the maximum decimals any fixed-window
    // element needs applies to all, so 100.5 drags 200 into 200.0.
    let uniform_decimals: Option<usize> = match object {
        RObject::Real(values) => {
            let mut max_dec = 0usize;
            let mut all_fixed = true;
            for v in values.iter() {
                if v.is_nan() || v.is_infinite() {
                    continue;
                }
                match decimals_used(*v) {
                    Some(d) => max_dec = max_dec.max(d),
                    None => {
                        all_fixed = false;
                        break;
                    }
                }
            }
            if all_fixed && max_dec > 0 {
                Some(max_dec)
            } else {
                None
            }
        }
        _ => None,
    };
    // GNU's digits option exactly as print.c: options(digits) pins the
    // significant digits of every real and complex print, clamped into
    // the 1..22 range, defaulting 7.
    let print_digits: i32 = heap
        .options
        .iter()
        .find(|(name, _)| name == "digits")
        .and_then(|(_, id)| match heap.object(*id) {
            Some(RObject::Real(vals)) => vals.first().copied(),
            Some(RObject::Integer(vals)) => vals.first().map(|v| *v as f64),
            _ => None,
        })
        .map(|v| (v as i32).clamp(1, 22))
        .unwrap_or(7);
    let elements: Vec<String> = match object {
        // GNU's printvector discipline exactly: real vectors format
        // through one uniform (width, decimals, scientific) triple
        // computed across the whole vector — fixed notation keeping its
        // trailing zeros, the width comparison deciding scientific.
        RObject::Real(values) => {
            let (w, d, e) = format_real_params_d(values, print_digits);
            values.iter().map(|&v| encode_real(v, w, d, e).trim_start().to_string()).collect()
        }
        // GNU's formatComplex exactly: two independent formatReal passes —
        // the real parts as one vector, the imaginary magnitudes as
        // another — the sign printing explicitly between them.
        RObject::Complex(values) => {
            let finite: Vec<&crate::object::RComplex> = values
                .iter()
                .filter(|z| z.r.to_bits() != crate::object::NA_REAL_BITS && z.i.to_bits() != crate::object::NA_REAL_BITS)
                .collect();
            let re_parts: Vec<f64> = finite.iter().map(|z| z.r).collect();
            let im_parts: Vec<f64> = finite.iter().map(|z| z.i.abs()).collect();
            let (wr, dr, er) = format_real_params_d(&re_parts, print_digits);
            let (wi, di, ei) = format_real_params_d(&im_parts, print_digits);
            values
                .iter()
                .map(|z| {
                    if z.r.to_bits() == crate::object::NA_REAL_BITS || z.i.to_bits() == crate::object::NA_REAL_BITS {
                        "NA".to_string()
                    } else {
                        let re_text = encode_real(z.r, wr, dr, er);
                        let im_text = encode_real(z.i.abs(), wi, di, ei);
                        // GNU's sign decision exactly as printvector.c:
                        // tmp.i >= 0 in IEEE — negative zero taking the
                        // plus exactly as EncodeReal0's zero normalization
                        // arranges.
                        let sign = if z.i < 0.0 { "-" } else { "+" };
                        format!("{}{}{}i", re_text, sign, im_text)
                    }
                })
                .collect()
        }
        _ => (0..length)
            .map(|i| {
                if let (Some(dec), RObject::Real(values)) = (uniform_decimals, object) {
                    let v = values[i];
                    if v.is_nan() || v.is_infinite() {
                        format_real(v)
                    } else {
                        fixed_exact(v, dec)
                    }
                } else {
                    format_element(heap, interner, object, i)
                }
            })
            .collect(),
    };
    let width = if matches!(object, RObject::String(_)) {
        // GNU does not column-align character vectors — quoting delimits.
        0
    } else {
        elements.iter().map(|e| e.len()).max().unwrap_or(0)
    };
    // GNU named-vector layout: the names print as a header row over the
    // values, each column right-justified to the wider of name and value,
    // with no [1] tags — the two-row reading is the tell.
    let names_id = crate::attrib::get_attrib(heap, interner, value, "names");
    let names_vec: Option<Vec<String>> = names_id.and_then(|id| match heap.object(id) {
        Some(RObject::String(values)) if values.len() == length => Some(
            values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
        ),
        _ => None,
    });
    let mut out = String::new();
    match names_vec {
        Some(names) => {
            // GNU's printNamedVector: one uniform width across every name
            // and value, each cell right-justified and followed by a
            // trailing space — line ends keep the final blank exactly as
            // printvector.c emits it.
            let uniform = elements
                .iter()
                .chain(names.iter())
                .map(|s| s.len())
                .max()
                .unwrap_or(0);
            for name in names.iter() {
                out.push_str(&" ".repeat(uniform - name.len()));
                out.push_str(name);
                out.push(' ');
            }
            out.push('\n');
            for element in elements.iter() {
                out.push_str(&" ".repeat(uniform - element.len()));
                out.push_str(element);
                out.push(' ');
            }
        }
        None => {
            out.push_str("[1]");
            if matches!(object, RObject::String(_)) {
                // GNU character vectors column-align LEFT-justified to the
                // uniform width — the padding trails each element, the
                // line itself keeping the final blank exactly as
                // printvector.c emits it.
                let uniform = elements.iter().map(|e| e.len()).max().unwrap_or(0);
                for element in &elements {
                    out.push(' ');
                    out.push_str(element);
                    out.push_str(&" ".repeat(uniform.saturating_sub(element.len())));
                }
            } else {
                for element in &elements {
                    out.push(' ');
                    out.push_str(&" ".repeat(width.saturating_sub(element.len())));
                    out.push_str(element);
                }
            }
        }
    }
    // GNU attribute blocks: every attribute the layout itself did not
    // consume prints as attr(,"name") followed by its own formatted body.
    out.push_str(&format_attribute_blocks(heap, interner, value));
    out
}

/// The trailing attr(,) blocks of a printed object — the attribute chain in
/// order, skipping the attributes the value layout consumes structurally
/// (names/dim/dimnames, and class/levels on factors).
fn format_attribute_blocks(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let chain = heap.get(value).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    if chain == ValueId::NIL {
        return String::new();
    }
    let factor = is_factor(heap, interner, value);
    let mut out = String::new();
    let mut cursor = chain;
    while let Some((car, cdr, tag)) = pairlist_parts(heap, cursor) {
        let name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
            _ => String::new(),
        };
        let consumed = matches!(name.as_str(), "names" | "dim" | "dimnames")
            || (factor && matches!(name.as_str(), "class" | "levels"));
        if !consumed && !name.is_empty() {
            out.push_str(&format!("\nattr(,\"{name}\")\n"));
            out.push_str(&format_value(heap, interner, car));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    out
}

/// The dim attribute of an object as integer extents, when present.
fn dim_attribute(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<Vec<i32>> {
    let dim_id = crate::attrib::get_attrib(heap, interner, value, "dim")?;
    match heap.object(dim_id) {
        Some(RObject::Integer(values)) => Some(values.clone()),
        Some(RObject::Real(values)) => Some(values.iter().map(|v| *v as i32).collect()),
        _ => None,
    }
}


#[allow(clippy::too_many_arguments)]
fn format_matrix_inner(
    heap: &Heap,
    interner: &SymbolInterner,
    object: &RObject,
    nrow: usize,
    ncol: usize,
    row_names: Option<Vec<String>>,
    col_names: Option<Vec<String>>,
) -> String {
    let element = |i: usize, j: usize| format_element(heap, interner, object, i + j * nrow);
    // Character matrix columns left-align exactly as GNU's printMatrix —
    // numeric columns right-align.
    let left_align = matches!(object, RObject::String(_));
    let headers: Vec<String> = match &col_names {
        Some(names) => names.clone(),
        None => (0..ncol).map(|j| format!("[,{}]", j + 1)).collect(),
    };
    let widths: Vec<usize> = (0..ncol)
        .map(|j| {
            let mut width = headers[j].len();
            for i in 0..nrow {
                width = width.max(element(i, j).len());
            }
            width
        })
        .collect();
    let row_labels: Vec<String> = match &row_names {
        Some(names) => names.clone(),
        None => (0..nrow).map(|i| format!("[{},]", i + 1)).collect(),
    };
    let label_width = row_labels.iter().map(|l| l.len()).max().unwrap_or(0);
    // Positional headers carry a leading space column the bare dimnames
    // headers do not — GNU's alignment detail.
    let positional = row_names.is_none() && col_names.is_none();
    let mut out = String::new();
    out.push_str(&" ".repeat(label_width + 1));
    for (j, header) in headers.iter().enumerate() {
        if j > 0 {
            out.push(' ');
        }
        out.push_str(&" ".repeat(widths[j] - header.len()));
        out.push_str(header);
    }
    let _ = positional;
    #[allow(clippy::needless_range_loop)] // multi-array index arithmetic is the algorithm
    for i in 0..nrow {
        out.push('\n');
        out.push_str(&row_labels[i]);
        out.push_str(&" ".repeat(label_width - row_labels[i].len() + 1));
        #[allow(clippy::needless_range_loop)] // multi-array index arithmetic is the algorithm
        for j in 0..ncol {
            if j > 0 {
                out.push(' ');
            }
            let text = element(i, j);
            if left_align {
                out.push_str(&text);
                out.push_str(&" ".repeat(widths[j] - text.len()));
            } else {
                out.push_str(&" ".repeat(widths[j] - text.len()));
                out.push_str(&text);
            }
        }
    }
    out
}

/// The cat-coercion of a scalar: unquoted, NA drops exactly as GNU (NA
/// contributes nothing to output).
pub fn format_cat_element(_heap: &Heap, interner: &SymbolInterner, value: &RObject, index: usize) -> String {
    match value {
        RObject::Real(values) => format_real(values[index]),
        RObject::Integer(values) | RObject::Logical(values) => {
            let v = values[index];
            if v == NA_INTEGER {
                "NA".to_string()
            } else if matches!(value, RObject::Logical(_)) {
                if v != 0 {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            } else {
                v.to_string()
            }
        }
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                "NA".to_string()
            } else {
                text.to_string()
            }
        }
        RObject::Complex(values) => {
            let z = values[index];
            // GNU's sign discipline exactly: the separator answers the
            // imaginary rendering's own leading minus — negative zero
            // never drawing one, just as format_real renders it bare.
            let im = format_real(z.i);
            let sign = if im.starts_with('-') { "" } else { "+" };
            format!("{}{}{}i", format_real(z.r), sign, im)
        }
        RObject::Raw(values) => format!("{:02x}", values[index]),
        _ => "?".to_string(),
    }
}

/// Extract dimnames as unquoted row/column name vectors when present.
fn dimnames_of(
    heap: &Heap,
    interner: &SymbolInterner,
    value: ValueId,
) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let id = match crate::attrib::get_attrib(heap, interner, value, "dimnames") {
        Some(v) => v,
        None => return (None, None),
    };
    let items = match heap.object(id) {
        Some(RObject::List(items)) => items.clone(),
        _ => return (None, None),
    };
    let extract = |idx: usize| -> Option<Vec<String>> {
        let item = items.get(idx)?;
        match heap.object(*item) {
            // Empty vectors ride as absent exactly as GNU's NULL dimnames
            // entries — positional labels take their place.
            Some(RObject::String(values)) if !values.is_empty() => Some(
                values
                    .iter()
                    .map(|v| interner.string_text(*v).to_string())
                    .collect(),
            ),
            _ => None,
        }
    };
    (extract(0), extract(1))
}

/// The class attribute holds data.frame — the object shape test.
fn is_data_frame(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match crate::attrib::get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "data.frame"),
            _ => false,
        },
        None => false,
    }
}

/// Unquoted cell text for one column element — print.data.frame's shape.
fn format_cell(heap: &Heap, interner: &SymbolInterner, column: &RObject, index: usize) -> String {
    match column {
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                // Frame cells render character NA bracketed exactly as
                // GNU's format.char — distinct from the bare NA of
                // matrix and vector printing.
                "<NA>".to_string()
            } else {
                text.to_string()
            }
        }
        _ => format_element(heap, interner, column, index),
    }
}

/// Tabular layout: headers right-justified over per-column widths, row
/// names leftmost — column-major cell extraction over the list shape.
fn format_data_frame(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let items = match heap.object(value) {
        Some(RObject::List(items)) => items.clone(),
        _ => return "<data.frame>".to_string(),
    };
    let col_names: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "names") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let row_names: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "row.names") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            Some(RObject::Integer(values)) => values.iter().map(|v| v.to_string()).collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let _ncol = items.len();
    let nrow = items
        .first()
        .and_then(|c| match heap.object(*c) {
            Some(RObject::Real(v)) => Some(v.len()),
            Some(RObject::Integer(v)) => Some(v.len()),
            Some(RObject::Logical(v)) => Some(v.len()),
            Some(RObject::String(v)) => Some(v.len()),
            _ => None,
        })
        .unwrap_or(0);
    // Per-column widths: widest of header and every cell.
    let mut widths: Vec<usize> = Vec::new();
    for (j, col) in items.iter().enumerate() {
        let mut width = col_names.get(j).map(|n| n.len()).unwrap_or(0);
        if let Some(column) = heap.object(*col) {
            for i in 0..nrow {
                width = width.max(format_cell(heap, interner, column, i).len());
            }
        }
        widths.push(width);
    }
    let label_width = row_names.iter().map(|n| n.len()).max().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&" ".repeat(label_width + 1));
    for (j, name) in col_names.iter().enumerate() {
        if j > 0 {
            out.push(' ');
        }
        out.push_str(&" ".repeat(widths[j] - name.len()));
        out.push_str(name);
    }
    for i in 0..nrow {
        out.push('\n');
        let label = row_names.get(i).cloned().unwrap_or_else(|| (i + 1).to_string());
        out.push_str(&label);
        out.push_str(&" ".repeat(label_width - label.len() + 1));
        for (j, col) in items.iter().enumerate() {
            if j > 0 {
                out.push(' ');
            }
            let cell = match heap.object(*col) {
                Some(column) => format_cell(heap, interner, column, i),
                None => "?".to_string(),
            };
            out.push_str(&" ".repeat(widths[j] - cell.len()));
            out.push_str(&cell);
        }
    }
    out
}

fn is_difftime(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    crate::attrib::get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "difftime")),
            _ => None,
        })
        .unwrap_or(false)
}

fn is_summary_default(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    crate::attrib::get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "summaryDefault")),
            _ => None,
        })
        .unwrap_or(false)
}

fn is_factor(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match crate::attrib::get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "factor"),
            _ => false,
        },
        None => false,
    }
}

/// `[1] b a b c` over unquoted labels, then `Levels: a b c` — NA codes
/// print as NA without a label exactly as GNU.
fn format_factor(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let codes = match heap.object(value) {
        Some(RObject::Integer(values)) => values.clone(),
        _ => return "<factor>".to_string(),
    };
    let levels: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "levels") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    // GNU's NA-level rendering exactly: a level that IS the missing
    // marker — materialized when exclude omits NA — prints <NA> both in
    // the values and the levels line, distinct from missing codes' NA.
    let render_level = |l: &String| if l == "\0NA" { "<NA>".to_string() } else { l.clone() };
    let labels: Vec<String> = codes
        .iter()
        .map(|c| {
            if *c == crate::object::NA_INTEGER {
                "NA".to_string()
            } else {
                levels
                    .get((*c - 1) as usize)
                    .map(render_level)
                    .unwrap_or_else(|| "NA".to_string())
            }
        })
        .collect();
    let width = labels.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut out = String::from("[1]");
    for label in &labels {
        out.push(' ');
        out.push_str(&" ".repeat(width - label.len()));
        out.push_str(label);
    }
    out.push_str("\nLevels:");
    for level in &levels {
        out.push(' ');
        out.push_str(&render_level(level));
    }
    out
}

/// Deparse a language object into R source text exactly as GNU's
/// print.language — infix operators render infix, calls parenthesize
/// comma-space-separated arguments, missing arguments print empty, and
/// atomic literals carry their literal form.
/// GNU's syntactic-name test exactly as make.names' inverse: letters,
/// digits, dot and underscore, never leading with a digit or a digit
/// after an initial dot, and never a reserved word.
fn is_syntactic_name(text: &str) -> bool {
    const RESERVED: &[&str] = &[
        "if", "else", "repeat", "while", "function", "for", "in", "next", "break",
        "TRUE", "FALSE", "NULL", "Inf", "NaN", "NA", "NA_integer_", "NA_real_",
        "NA_complex_", "NA_character_",
    ];
    if text.is_empty() || RESERVED.contains(&text) {
        return false;
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '.') {
        return false;
    }
    if first == '.' {
        if let Some(second) = chars.next() {
            if second.is_ascii_digit() {
                return false;
            }
        } else {
            return false;
        }
    }
    text.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// Deparse a name exactly as GNU: syntactic names render bare,
/// non-syntactic names backtick-quote.
fn deparse_name(text: &str) -> String {
    if is_syntactic_name(text) {
        text.to_string()
    } else {
        format!("`{}`", text)
    }
}

thread_local! {
    static DEPARSE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// GNU's width.cutoff line breaking exactly as deparse.c arranges:
/// tokens accumulate until the line passes the cutoff — default 60
/// exactly as deparse's width.cutoff — then the next comma boundary
/// emits its space and breaks. Continuation lines carry no indent.
fn deparse_wrap_cutoff(flat: &str) -> String {
    if flat.len() <= 60 {
        return flat.to_string();
    }
    let mut out = String::with_capacity(flat.len() + 8);
    let mut width = 0usize;
    let mut in_string = false;
    let chars: Vec<char> = flat.chars().collect();
    let mut i = 0usize;
    // GNU's indentation discipline exactly as deparse.c arranges: a
    // break inside a call's argument list — linebreak() in args2buff —
    // indents the continuation one level of four spaces, while a break
    // inside a vector's c() construction — writeline() in vec2buff —
    // carries no indentation of its own. The paren stack tracks the
    // innermost enclosing construction to tell the two commas apart.
    let mut paren_stack: Vec<bool> = Vec::new(); // true when the opener is c(
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            in_string = !in_string;
        }
        if !in_string && c == '(' {
            // The construction is c( exactly when the identifier
            // immediately before the paren is the bare letter c.
            let before: String = out.chars().rev().take_while(|ch| ch.is_alphanumeric() || *ch == '.' || *ch == '_').collect();
            let before: String = before.chars().rev().collect();
            paren_stack.push(before == "c");
        } else if !in_string && c == ')' {
            paren_stack.pop();
        }
        out.push(c);
        width += 1;
        if !in_string && c == ',' && width >= 60 {
            // The comma's trailing space writes before the break exactly
            // as GNU — the golden lines end in that space.
            if i + 1 < chars.len() && chars[i + 1] == ' ' {
                out.push(' ');
                i += 1;
            }
            out.push('\n');
            let in_vector = paren_stack.last().copied().unwrap_or(false);
            if in_vector {
                width = 0;
            } else {
                out.push_str("    ");
                width = 4;
            }
        }
        i += 1;
    }
    out
}

/// GNU's deparse discipline for attributed objects: names alone inline
/// into the c() construction as `name = value` entries; any further
/// attribute wraps the data rendering in structure() with every
/// attribute deparsed in chain order exactly as deparse.c arranges.
pub fn deparse(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let depth = DEPARSE_DEPTH.with(|d| d.get());
    DEPARSE_DEPTH.with(|d| d.set(depth + 1));
    let flat = deparse_attributed(heap, interner, value);
    DEPARSE_DEPTH.with(|d| d.set(depth));
    if depth == 0 {
        deparse_wrap_cutoff(&flat)
    } else {
        flat
    }
}

fn deparse_attributed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let attrib = heap.get(value).map(|slot| slot.attrib).unwrap_or(crate::id::ValueId::NIL);
    // Collect the chain in insertion order — (name, value) pairs.
    let mut entries: Vec<(String, crate::id::ValueId)> = Vec::new();
    let mut cursor = attrib;
    for _ in 0..1000 {
        if cursor == crate::id::ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    entries.push((interner.symbol_name(*name).to_string(), *car));
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    let atomic = matches!(
        heap.object(value),
        Some(RObject::Real(_))
            | Some(RObject::Integer(_))
            | Some(RObject::Logical(_))
            | Some(RObject::String(_))
            | Some(RObject::Complex(_))
            | Some(RObject::Raw(_))
            | Some(RObject::List(_))
    );
    // S4 objects own their rendering entirely — the new() construction
    // already accounts for class and slots, so the generic attribute
    // wrap never applies exactly as GNU's object-aware deparse arranges.
    if !atomic || entries.is_empty() || s4_class_of(heap, interner, value).is_some() {
        return deparse_base(heap, interner, value);
    }
    // Names-only chains inline into the c() construction exactly as
    // GNU — length-one vectors still render through c() with the name.
    let only_names = entries.iter().all(|(n, _)| n == "names");
    if only_names {
        let names: Vec<String> = match heap.object(entries[0].1) {
            Some(RObject::String(vals)) => vals.iter().map(|v| interner.string_text(*v).to_string()).collect(),
            _ => Vec::new(),
        };
        let elems = deparse_elements(heap, interner, value);
        if !elems.is_empty() && elems.len() == names.len() {
            let parts: Vec<String> = elems
                .iter()
                .zip(names.iter())
                .map(|(v, n)| {
                    if n == "NA" || n.is_empty() {
                        v.clone()
                    } else if n.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_')
                        && !n.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                    {
                        format!("{n} = {v}")
                    } else {
                        format!("`{n}` = {v}")
                    }
                })
                .collect();
            return format!("c({})", parts.join(", "));
        }
        return deparse_base(heap, interner, value);
    }
    // The structure() wrap: base rendering first, attributes following
    // in chain order — names among them exactly as GNU when mixed with
    // dimensional or class attributes.
    let base = deparse_base(heap, interner, value);
    let mut parts: Vec<String> = vec![base];
    for (name, attr_value) in &entries {
        let rendered = deparse(heap, interner, *attr_value);
        parts.push(format!("{name} = {rendered}"));
    }
    format!("structure({})", parts.join(", "))
}

/// Element-wise deparse of an atomic vector for the named-inline
/// construction — scalar renderings exactly as GNU's c() entries.
fn deparse_elements(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match heap.object(value) {
        Some(RObject::Real(vals)) => vals.iter().map(|v| format_real(*v)).collect(),
        Some(RObject::Integer(vals)) => vals
            .iter()
            .map(|&v| if v == crate::object::NA_INTEGER { "NA_integer_".to_string() } else { format!("{v}L") })
            .collect(),
        Some(RObject::Logical(vals)) => vals
            .iter()
            .map(|&v| if v == crate::object::NA_INTEGER { "NA".to_string() } else if v != 0 { "TRUE".to_string() } else { "FALSE".to_string() })
            .collect(),
        Some(RObject::String(vals)) => vals.iter().map(|v| format!("\"{}\"", interner.string_text(*v))).collect(),
        _ => Vec::new(),
    }
}

fn deparse_base(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    // S4 objects deparse as their constructing new() call exactly as GNU —
    // class name first, slots in definition order with deparsed values.
    if let Some(class_text) = s4_class_of(heap, interner, value)
        && let Some(RObject::List(items)) = heap.object(value) {
            let items = items.clone();
            let slot_names: Vec<String> = crate::attrib::attrib_names(heap, interner, value);
            let mut parts: Vec<String> = Vec::new();
            for (index, item) in items.iter().enumerate() {
                let name = slot_names.get(index).cloned().unwrap_or_default();
                let value_text = deparse(heap, interner, *item);
                if name.is_empty() {
                    parts.push(value_text);
                } else {
                    parts.push(format!("{} = {}", name, value_text));
                }
            }
            // GNU's .Data discipline exactly: a class containing list
            // carries its payload as the object's own elements, rendered
            // back through the .Data pseudo-slot as a list() call.
            let contains_list = {
                let mut current = Some(class_text.clone());
                let mut found = false;
                while let Some(name) = current {
                    if name == "list" {
                        found = true;
                        break;
                    }
                    current = match heap.s4_classes.get(&name) {
                        Some((_, parent, _validity)) => parent.clone(),
                        None => None,
                    };
                }
                found
            };
            if contains_list && !items.is_empty() && slot_names.iter().all(|n| n.is_empty()) {
                let element_text: Vec<String> = items.iter().map(|item| deparse(heap, interner, *item)).collect();
                parts = vec![format!(".Data = list({})", element_text.join(", "))];
            }
            return format!("new(\"{}\"{}{})", class_text, if parts.is_empty() { "" } else { ", " }, parts.join(", "));
        }
    match heap.object(value) {
        // Non-syntactic names quote through backticks exactly as GNU's
        // deparse validity discipline.
        Some(RObject::Symbol { name, .. }) => deparse_name(interner.symbol_name(*name)),
        // Expression vectors deparse inside the expression() wrapper with
        // each element comma-separated exactly as GNU's print shape.
        Some(RObject::Expression(items)) => {
            if items.is_empty() {
                return "expression()".to_string();
            }
            let parts: Vec<String> = items
                .iter()
                .map(|item| deparse(heap, interner, *item))
                .collect();
            return format!("expression({})", parts.join(", "));
        }
        Some(RObject::Language { car, cdr, .. }) => {
            let op_text = match heap.object(*car) {
                Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                _ => return deparse(heap, interner, *car),
            };
            // Collect the argument expressions in order.
            let mut items: Vec<ValueId> = Vec::new();
            let mut item_tags: Vec<Option<String>> = Vec::new();
            let mut cursor = *cdr;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag, .. }) => {
                        items.push(*car);
                        let tag_text = match heap.object(*tag) {
                            Some(RObject::Symbol { name, .. }) => {
                                let text = interner.symbol_name(*name).to_string();
                                if text.is_empty() { None } else { Some(text) }
                            }
                            _ => None,
                        };
                        item_tags.push(tag_text);
                        cursor = *cdr;
                    }
                    _ => break,
                }
            }
            // GNU's control-flow forms exactly as deparse.c's PP sheets:
            // if renders with its parenthesized predicate and the else
            // arm inline; braces join their statements with semicolons;
            // the loop forms carry their headers verbatim.
            if op_text == "if" && (items.len() == 2 || items.len() == 3) {
                let cond = deparse(heap, interner, items[0]);
                let yes = deparse(heap, interner, items[1]);
                if items.len() == 3 {
                    let no = deparse(heap, interner, items[2]);
                    return format!("if ({}) {} else {}", cond, yes, no);
                }
                return format!("if ({}) {}", cond, yes);
            }
            if op_text == "{" {
                if items.is_empty() {
                    return "{ }".to_string();
                }
                let parts: Vec<String> = items
                    .iter()
                    .map(|item| deparse(heap, interner, *item))
                    .collect();
                return format!("{{  {} }}", parts.join("; "));
            }
            if op_text == "while" && items.len() == 2 {
                return format!("while ({}) {}", deparse(heap, interner, items[0]), deparse(heap, interner, items[1]));
            }
            if op_text == "for" && items.len() == 3 {
                return format!(
                    "for ({} in {}) {}",
                    deparse(heap, interner, items[0]),
                    deparse(heap, interner, items[1]),
                    deparse(heap, interner, items[2])
                );
            }
            if op_text == "repeat" && items.len() == 1 {
                return format!("repeat {}", deparse(heap, interner, items[0]));
            }
            if op_text == "function" && items.len() == 2 {
                return format!("function ({}) {}", deparse(heap, interner, items[0]), deparse(heap, interner, items[1]));
            }
            if op_text == "(" && items.len() == 1 {
                return format!("({})", deparse(heap, interner, items[0]));
            }
            const INFIX: &[&str] = &[
                "+", "-", "*", "/", "^", "%%", "<", ">", "<=", ">=", "==", "!=", "|", "&", "~", "=", "<-", ":=", "%in%", "|>", "||", "&&",
            ];
            // `:` renders infix without spaces exactly as GNU — 1:3 —
            // while the arithmetic and comparison operators pad.
            if op_text == ":" && items.len() == 2 {
                let left = deparse(heap, interner, items[0]);
                let right = deparse(heap, interner, items[1]);
                return format!("{}:{}", left, right);
            }
            if INFIX.contains(&op_text.as_str())
                && items.len() == 2 {
                    let left = deparse(heap, interner, items[0]);
                    let right = deparse(heap, interner, items[1]);
                    return format!("{} {} {}", left, op_text, right);
                }
            // Unary operators hug their operand exactly as GNU — -x,
            /// !x, ~x — no space intervening.
            const UNARY: &[&str] = &["-", "+", "!", "~"];
            if UNARY.contains(&op_text.as_str()) && items.len() == 1 {
                return format!("{}{}", op_text, deparse(heap, interner, items[0]));
            }
            if op_text == "(" {
                return format!("({})", items.first().map(|v| deparse(heap, interner, *v)).unwrap_or_default());
            }
            if op_text == "{" {
                let inner: Vec<String> = items.iter().map(|v| deparse(heap, interner, *v)).collect();
                return format!("{{{}}}", inner.join("; "));
            }
            let rendered: Vec<String> = items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    // Tagged arguments render name = value exactly as GNU's
                    // deparse of a call object.
                    let value_text = if *v == crate::id::ValueId::MISSING {
                        String::new()
                    } else {
                        deparse(heap, interner, *v)
                    };
                    match item_tags.get(i).cloned().flatten() {
                        Some(tag) => format!("{} = {}", deparse_name(&tag), value_text),
                        None => value_text,
                    }
                })
                .collect();
            format!("{}({})", op_text, rendered.join(", "))
        }
        Some(RObject::Real(vals)) if vals.len() == 1 => format_real(vals[0]),
        Some(RObject::Integer(vals)) if vals.len() == 1 => {
            if vals[0] == crate::object::NA_INTEGER {
                "NA_integer_".to_string()
            } else {
                format!("{}L", vals[0])
            }
        }
        Some(RObject::Logical(vals)) if vals.len() == 1 => {
            if vals[0] == crate::object::NA_INTEGER {
                "NA".to_string()
            } else if vals[0] != 0 {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Some(RObject::String(vals)) if vals.len() == 1 => {
            let text = interner.string_text(vals[0]);
            format!("\"{}\"", text)
        }
        // Zero-length atomic vectors deparse through their typed
        // constructor exactly as GNU — integer(0), numeric(0) — the
        // roundtrip reconstructing the precise type.
        Some(RObject::Real(vals)) if vals.is_empty() => "numeric(0)".to_string(),
        Some(RObject::Integer(vals)) if vals.is_empty() => "integer(0)".to_string(),
        Some(RObject::Logical(vals)) if vals.is_empty() => "logical(0)".to_string(),
        Some(RObject::String(vals)) if vals.is_empty() => "character(0)".to_string(),
        Some(RObject::Complex(vals)) if vals.is_empty() => "complex(0)".to_string(),
        Some(RObject::Raw(vals)) if vals.is_empty() => "raw(0)".to_string(),
        Some(RObject::Nil) => "NULL".to_string(),
        // The NIL sentinel itself — parse produces it for the NULL literal
        // exactly as GNU's R_NilValue deparses.
        None => "NULL".to_string(),
        // Multi-element atomic vectors deparse to their c() construction
        // exactly as GNU — each element rendering through the scalar forms.
        Some(RObject::Real(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals.iter().map(|v| format_real(*v)).collect();
            format!("c({})", parts.join(", "))
        }
        Some(RObject::Integer(vals)) if !vals.is_empty() => {
            // GNU's consecutive-sequence compaction exactly: integer
            // vectors stepping by one deparse through the colon
            // construction — dim = 2:3 rather than c(2L, 3L).
            let consecutive = vals.len() >= 2
                && vals.iter().all(|&v| v != crate::object::NA_INTEGER)
                && vals.windows(2).all(|w| w[1] - w[0] == 1 || w[1] - w[0] == -1)
                && vals.windows(2).map(|w| w[1] - w[0]).collect::<std::collections::HashSet<_>>().len() == 1;
            if consecutive {
                format!("{}:{}", vals[0], vals[vals.len() - 1])
            } else {
                let parts: Vec<String> = vals
                    .iter()
                    .map(|&v| {
                        if v == crate::object::NA_INTEGER {
                            "NA_integer_".to_string()
                        } else {
                            format!("{}L", v)
                        }
                    })
                    .collect();
                format!("c({})", parts.join(", "))
            }
        }
        Some(RObject::Logical(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals
                .iter()
                .map(|&v| {
                    if v == crate::object::NA_INTEGER {
                        "NA".to_string()
                    } else if v != 0 {
                        "TRUE".to_string()
                    } else {
                        "FALSE".to_string()
                    }
                })
                .collect();
            format!("c({})", parts.join(", "))
        }
        Some(RObject::String(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals
                .iter()
                .map(|v| format!("\"{}\"", interner.string_text(*v)))
                .collect();
            format!("c({})", parts.join(", "))
        }
        // Generic vectors deparse through their list() construction
        // exactly as GNU — each element deparsed comma-separated, NULL
        // rendering bare, names attaching as `name = value` entries.
        Some(RObject::List(items)) => {
            let items = items.clone();
            let names: Vec<String> = match crate::attrib::get_attrib(heap, interner, value, "names") {
                Some(n) => crate::attrib::string_vec_pub(heap, interner, n),
                None => Vec::new(),
            };
            let parts: Vec<String> = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let body = deparse(heap, interner, *item);
                    match names.get(index) {
                        Some(n) if !n.is_empty() => format!("{n} = {body}"),
                        _ => body,
                    }
                })
                .collect();
            format!("list({})", parts.join(", "))
        }
        _ => "?".to_string(),
    }
}

/// The object's class when it names a registered S4 class — the slice's
/// S4-bit reading exactly as the isS4 predicate computes it.
fn s4_class_of(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<String> {
    let attrib = crate::attrib::get_attrib_public(heap, interner, value, "class")?;
    if let Some(RObject::String(vals)) = heap.object(attrib) {
        for v in vals.iter() {
            let text = interner.string_text(*v).to_string();
            if heap.s4_classes.contains_key(&text) {
                return Some(text);
            }
        }
    }
    None
}
