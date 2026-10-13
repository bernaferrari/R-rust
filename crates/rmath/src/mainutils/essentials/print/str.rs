use super::*;

/// GNU `strOptions()$digits.d` / `$vec.len` defaults.
const STR_DIGITS_D: i32 = 3;
const STR_VEC_LEN: f64 = 4.0;
const STR_OUT_DEC: *const std::os::raw::c_char = b".\0".as_ptr() as *const std::os::raw::c_char;

thread_local! {
    /// Active `str(digits.d = .)` override; `None` = strOptions() default.
    static STR_DIGITS_OVERRIDE: std::cell::Cell<Option<i32>> =
        const { std::cell::Cell::new(None) };
}

/// RAII guard applying a `digits.d` override for the duration of a str call.
pub(crate) struct StrDigitsGuard(Option<i32>);

impl StrDigitsGuard {
    pub(crate) fn new(digits: Option<i32>) -> Self {
        STR_DIGITS_OVERRIDE.with(|c| c.set(digits));
        StrDigitsGuard(digits)
    }
}

impl Drop for StrDigitsGuard {
    fn drop(&mut self) {
        STR_DIGITS_OVERRIDE.with(|c| c.set(None));
    }
}

fn str_digits_d() -> i32 {
    STR_DIGITS_OVERRIDE.with(|c| c.get().unwrap_or(STR_DIGITS_D))
}

fn str_signif(x: f64, digits: i32) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let exp = x.abs().log10().floor();
    let pow = 10f64.powf(f64::from(digits) - 1.0 - exp);
    (x * pow).round() / pow
}

/// GNU `prettyNum(drop0trailing = TRUE)` on one formatted number (format.R):
/// with a decimal part containing `e`, only a trailing `e[+-]0+` suffix is
/// dropped (mantissa zeros stay, so `"0.000000e+00"` -> `"0.000000"`);
/// without `e`, trailing fractional zeros are dropped and an empty fraction
/// drops the decimal mark (`"2.0"` -> `"2"`). No decimal mark: unchanged.
fn str_drop0trailing(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    match s.find('.') {
        None => s.to_string(),
        Some(dot) => {
            let (head, frac) = s.split_at(dot);
            let frac = &frac[1..];
            let new_frac = if frac.contains('e') {
                // sub("e[+-]0+$", '', frac): drop only a zero exponent suffix
                match frac.find(['e', 'E']) {
                    Some(epos) => {
                        let (mant, exp) = frac.split_at(epos);
                        let trimmed = exp.strip_prefix(|c| c == 'e' || c == 'E').and_then(|rest| {
                            let rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
                            if !rest.is_empty() && rest.bytes().all(|b| b == b'0') {
                                Some(())
                            } else {
                                None
                            }
                        });
                        match trimmed {
                            Some(()) => mant.to_string(),
                            None => frac.to_string(),
                        }
                    }
                    None => frac.to_string(),
                }
            } else {
                frac.trim_end_matches('0').to_string()
            };
            if new_frac.is_empty() {
                head.to_string()
            } else {
                format!("{head}.{new_frac}")
            }
        }
    }
}

/// GNU `format(x, trim=TRUE, drop0trailing=TRUE)` under `options(digits=digits.d)`.
unsafe fn str_format_real(x: f64) -> String {
    unsafe {
        if x.is_nan() {
            return if crate::sexp::ffi::is_na_real(x) {
                "NA".to_string()
            } else {
                "NaN".to_string()
            };
        }

        let tmp = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        if tmp.is_null() {
            return x.to_string();
        }
        let _tmp = protect(tmp);
        *REAL(tmp) = x;
        let old = crate::mainutils::format::format_get_R_print();
        crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
            digits: str_digits_d(),
            scipen: old.scipen,
            na_width: old.na_width,
            na_width_noquote: old.na_width_noquote,
        });
        let mut w = 0;
        let mut d = 0;
        let mut e = 0;
        crate::mainutils::format::formatRealS(tmp, 1, &mut w, &mut d, &mut e, 0);
        let encoded = crate::mainutils::printutils::EncodeReal0(x, w, d, e, STR_OUT_DEC);
        crate::mainutils::format::format_set_R_print(old);
        let raw = if encoded.is_null() {
            String::new()
        } else {
            CStr::from_ptr(encoded).to_string_lossy().into_owned()
        };
        str_drop0trailing(raw.trim())
    }
}

unsafe fn str_numeric_integer_like(x: SEXP, n_check: usize) -> bool {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::INTSXP {
            return true;
        }
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return false;
        }
        let n = (XLENGTH(x) as usize).min(n_check);
        for i in 0..n {
            let v = REAL_ELT(x, i as std::os::raw::c_int);
            if v.is_nan() {
                continue;
            }
            let ao = v.abs();
            if !(ao > 1e-10 || v == 0.0) || !(ao < 1e10 || v == 0.0) {
                return false;
            }
            if (v - str_signif(v, str_digits_d())).abs() > 9e-16 * ao {
                return false;
            }
        }
        true
    }
}

/// GNU str.default S4 non-envRefClass header + `..@ slot :` lines.
unsafe fn str_s4_formal_class(x: SEXP) {
    unsafe {
        let class_val =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        let cl = if !class_val.is_null()
            && class_val != R_NilValue()
            && TYPEOF(class_val) == SEXPTYPE::STRSXP
            && XLENGTH(class_val) > 0
        {
            (0..XLENGTH(class_val))
                .map(|i| elt_to_string(class_val, i))
                .collect::<Vec<_>>()
                .join("', '")
        } else {
            String::new()
        };
        let pkg_sym = Rf_install(c"package".as_ptr());
        let pkg_val = crate::sexp::attrib_core::getAttrib(class_val, pkg_sym);
        let pkg = if !pkg_val.is_null()
            && pkg_val != R_NilValue()
            && TYPEOF(pkg_val) == SEXPTYPE::STRSXP
            && XLENGTH(pkg_val) > 0
        {
            elt_to_string(pkg_val, 0)
        } else {
            ".GlobalEnv".to_string()
        };
        let names: Vec<String> = {
            let class_name = if cl.is_empty() {
                None
            } else {
                Some(cl.as_str())
            };
            let from_gnu = class_name.and_then(|name| {
                crate::mainutils::objects::gnu_s4_slot_names(name).filter(|slots| !slots.is_empty())
            });
            let from_def = class_name.and_then(|name| {
                crate::mainutils::objects::s4_all_slots(name).filter(|slots| !slots.is_empty())
            });
            let mut names = from_gnu.or(from_def).unwrap_or_default();
            // GNU class "signature": `names` is the names attribute of the
            // character data part, not a tagged ATTRIB slot.
            let names_attr =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if !names_attr.is_null()
                && names_attr != R_NilValue()
                && !names.iter().any(|existing| existing == "names")
            {
                let idx = names
                    .iter()
                    .position(|n| n == ".Data")
                    .map(|i| i + 1)
                    .unwrap_or(0);
                names.insert(idx, "names".to_string());
            }
            let pkg_slot = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"package".as_ptr()));
            if !pkg_slot.is_null()
                && pkg_slot != R_NilValue()
                && !names.iter().any(|existing| existing == "package")
            {
                names.push("package".to_string());
            }
            names
        };
        let n_slots = names.len() as i64;
        let slot_word = if n_slots == 1 { "slot" } else { "slots" };
        str_emit_line(&format!(
            "Formal class '{cl}' [package \"{pkg}\"] with {n_slots} {slot_word}"
        ));
        let width = names.iter().map(|n| n.len()).max().unwrap_or(0);
        for name in &names {
            let cname = CString::new(name.as_str()).unwrap_or_default();
            let sym = Rf_install(cname.as_ptr());
            let value = crate::mainutils::essentials::R_do_slot(x, sym);
            let _value = protect(value);
            let summary = str_atomic_summary(value);
            str_emit_line(&format!("  ..@ {name:width$}: {summary}"));
        }
    }
}

unsafe fn str_atomic_summary(x: SEXP) -> String {
    unsafe { str_atomic_summary_opts(x, true, false) }
}

unsafe fn str_atomic_summary_opts(x: SEXP, give_length: bool, nested: bool) -> String {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return "NULL".to_string();
        }
        if sexp_has_class_name(x, "factor") {
            return str_factor_summary(x);
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP || sexp_has_class_name(x, "formula") {
            return str_language_summary(x);
        }
        if TYPEOF(x) == SEXPTYPE::VECSXP {
            return str_list_summary(x);
        }
        if sexp_has_class_name(x, "Date") {
            return str_date_summary(x, give_length);
        }

        if sexp_has_class_name(x, "ts") {
            let n = XLENGTH(x);
            let tsp = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"tsp".as_ptr()));
            let (start, end) =
                if !tsp.is_null() && TYPEOF(tsp) == SEXPTYPE::REALSXP && XLENGTH(tsp) >= 2 {
                    (
                        str_format_real(*REAL(tsp)),
                        str_format_real(*REAL(tsp).add(1)),
                    )
                } else {
                    ("1".to_string(), n.to_string())
                };
            let dim =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
            let lestr = if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 1 {
                let dims: Vec<String> = (0..XLENGTH(dim))
                    .map(|i| format!("1:{}", *INTEGER(dim).add(i as usize)))
                    .collect();
                format!("[{}]", dims.join(", "))
            } else {
                format!("[1:{n}]")
            };
            let preview = str_preview_reals_or_ints(x, 0);
            let mid = if give_length { lestr } else { String::new() };
            return format!("Time-Series {mid} from {start} to {end}: {preview}");
        }

        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let has_dim = !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 1;
        let named = !has_dim
            && !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) > 0;
        let named_prefix = if named { "Named " } else { "" };
        let type_name = match t {
            t if t == SEXPTYPE::REALSXP => "num",
            t if t == SEXPTYPE::INTSXP => "int",
            t if t == SEXPTYPE::LGLSXP => "logi",
            t if t == SEXPTYPE::STRSXP => "chr",
            t if t == SEXPTYPE::CPLXSXP => "cplx",
            t if t == SEXPTYPE::RAWSXP => "raw",
            t if t == SEXPTYPE::VECSXP => "List",
            _ => "?",
        };
        let class_name = sexp_first_class_name(x);
        let class_prefix = str_class_mod_prefix(class_name.as_deref(), type_name);
        if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 1 {
            let dims: Vec<String> = (0..XLENGTH(dim))
                .map(|i| format!("1:{}", *INTEGER(dim).add(i as usize)))
                .collect();
            let preview = str_preview_reals_or_ints(x, 6);
            return format!(
                "{class_prefix}{named_prefix}{type_name} [{}] {preview}",
                dims.join(", ")
            );
        }

        if n == 0 {
            return format!("{class_prefix}{named_prefix}{type_name}(0)");
        }
        let preview = str_preview_reals_or_ints(x, if nested { STR_VEC_LEN as usize } else { 0 });

        if preview.is_empty() {
            format!("{class_prefix}{named_prefix}{type_name} [1:{n}]")
        } else if !give_length && n != 1 {
            format!("{class_prefix}{named_prefix}{type_name}  {preview}")
        } else if n == 1 {
            format!("{class_prefix}{named_prefix}{type_name} {preview}")
        } else {
            format!("{class_prefix}{named_prefix}{type_name} [1:{n}] {preview}")
        }
    }
}

/// GNU str.default factor header: `Factor`/`Ord.factor`, quoted levels,
/// integer codes after the colon.
unsafe fn str_factor_summary(x: SEXP) -> String {
    unsafe {
        let ordered = sexp_has_class_name(x, "ordered");
        let kind = if ordered { "Ord.factor" } else { "Factor" };
        let sep = if ordered { "<" } else { "," };
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        let nlev = if !levels.is_null() && TYPEOF(levels) == SEXPTYPE::STRSXP {
            XLENGTH(levels)
        } else {
            0
        };
        let quoted: Vec<String> = (0..nlev)
            .map(|i| format!("\"{}\"", elt_to_string(levels, i)))
            .collect();
        // GNU: include levels until cumulative `3 + nchar(quoted)-2` exceeds 13.
        let mut shown = 0usize;
        let mut acc = 0i32;
        for q in &quoted {
            acc += 3 + (q.len() as i32 - 2);
            shown += 1;
            if acc > 13 {
                break;
            }
        }
        if shown == 0 && nlev > 0 {
            shown = 1;
        }
        let mut level_text = quoted[..shown.min(quoted.len())].join(sep);
        if nlev > shown as i64 {
            level_text.push_str(sep);
            level_text.push_str("..");
        }

        let n = XLENGTH(x) as usize;
        let show = n.min(10);
        let mut parts = Vec::with_capacity(show);
        for i in 0..show {
            let v = INTEGER_ELT(x, i as std::os::raw::c_int);
            if v == NA_INTEGER {
                parts.push("NA".to_string());
            } else {
                parts.push(v.to_string());
            }
        }
        let mut preview = parts.join(" ");
        if n > show {
            preview.push_str(" ...");
        }
        if nlev > 0 {
            format!("{kind} w/ {nlev} levels {level_text}: {preview}")
        } else {
            format!("{kind} w/ {nlev} levels: {preview}")
        }
    }
}

unsafe fn str_date_summary(x: SEXP, give_length: bool) -> String {
    unsafe {
        let n = XLENGTH(x);
        let formatted = crate::mainutils::essentials::do_format_Date(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Rf_cons(x, R_NilValue()),
            R_NilValue(),
        );
        let _formatted = protect(formatted);
        let show = if give_length {
            (n as usize).min(STR_VEC_LEN as usize)
        } else {
            (n as usize).min(2)
        };
        let mut parts = Vec::with_capacity(show);
        if !formatted.is_null() && TYPEOF(formatted) == SEXPTYPE::STRSXP {
            for i in 0..show {
                let elt = STRING_ELT(formatted, i as R_xlen_t);
                if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                    parts.push("NA".to_string());
                } else {
                    parts.push(format!("\"{}\"", elt_to_string(formatted, i as R_xlen_t)));
                }
            }
        }
        let mut preview = parts.join(" ");
        if n as usize > show {
            preview.push_str(" ...");
        }
        let le = if give_length && n > 0 {
            format!("[1:{n}]")
        } else {
            String::new()
        };
        format!("Date{le}, format: {preview}")
    }
}

unsafe fn str_language_summary(x: SEXP) -> String {
    unsafe {
        let deparsed = crate::mainutils::deparse::deparse_symbolic(x, true);
        let _deparsed = protect(deparsed);
        let mut text = String::new();
        if !deparsed.is_null() && TYPEOF(deparsed) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(deparsed) {
                if i > 0 {
                    text.push(' ');
                }
                text.push_str(&elt_to_string(deparsed, i));
            }
        }
        let mut out = if sexp_has_class_name(x, "formula") {
            format!("Class 'formula'  language {text}")
        } else {
            format!("language {text}")
        };
        let env = crate::sexp::attrib_core::getAttrib(x, Rf_install(c".Environment".as_ptr()));
        if !env.is_null() && env != R_NilValue() {
            out.push_str("\n  .. ..- attr(*, \".Environment\")=");
            out.push_str(&str_environment_brief(env));
        }
        out
    }
}

unsafe fn str_environment_brief(env: SEXP) -> String {
    unsafe {
        if env == crate::sexp::globals::R_EmptyEnv() {
            "<environment: R_EmptyEnv>".to_string()
        } else if env == crate::sexp::globals::R_BaseEnv() {
            "<environment: base>".to_string()
        } else if env == crate::sexp::globals::R_GlobalEnv() {
            "<environment: R_GlobalEnv>".to_string()
        } else {
            "<environment: 0x0>".to_string()
        }
    }
}

static STR_LEVEL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(100);
static STR_LIST_LEN: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(99);

unsafe fn str_list_summary(x: SEXP) -> String {
    unsafe { str_list_summary_indent(x, "  ..$ ") }
}

unsafe fn str_list_summary_indent(x: SEXP, child_prefix: &str) -> String {
    unsafe {
        let n = XLENGTH(x);
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        let has_names = !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == n;
        let raw_names: Vec<String> = (0..n)
            .map(|i| {
                if has_names {
                    elt_to_string(names, i)
                } else {
                    String::new()
                }
            })
            .collect();
        let name_width = raw_names.iter().map(String::len).max().unwrap_or(0);
        let level = STR_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
        if level <= 1 {
            return format!("List of {n}");
        }
        let name_width = raw_names.iter().map(String::len).max().unwrap_or(0);
        let mut out = format!("List of {n}");
        let show = n.min(
            STR_LIST_LEN
                .load(std::sync::atomic::Ordering::Relaxed)
                .max(0),
        );
        STR_LEVEL.store(level - 1, std::sync::atomic::Ordering::Relaxed);
        for i in 0..show {
            let name = format!("{:<name_width$}", raw_names[i as usize]);
            let elem = VECTOR_ELT(x, i);
            out.push_str(&format!(
                "\n{child_prefix}{name}: {}",
                str_atomic_summary_opts(elem, true, true)
            ));
        }
        STR_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
        if n > show {
            out.push_str("\n  .. [list output truncated]");
        }
        out
    }
}

unsafe fn str_child_dimnames_lines(x: SEXP) -> Option<String> {
    unsafe {
        let dn =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
        if dn.is_null() || dn == R_NilValue() || TYPEOF(dn) != SEXPTYPE::VECSXP {
            return None;
        }
        Some(format!(
            "  ..- attr(*, \"dimnames\")={}",
            str_list_summary_indent(dn, "  .. ..$ ")
        ))
    }
}

unsafe fn str_preview_ints(x: SEXP, max: usize) -> String {
    unsafe {
        let n = XLENGTH(x) as usize;
        let quote = TYPEOF(x) == SEXPTYPE::STRSXP;
        let take = n.min(40);
        let mut encoded: Vec<String> = Vec::with_capacity(take);
        for i in 0..take {
            if quote {
                let elt = STRING_ELT(x, i as R_xlen_t);
                if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                    encoded.push("NA".to_string());
                } else {
                    encoded.push(str_encode_quoted(&elt_to_string(x, i as R_xlen_t)));
                }
            } else {
                encoded.push(elt_to_string(x, i as R_xlen_t));
            }
        }
        let show = if quote && max == 0 {
            let header = 4 + 1 + format!("chr [1:{n}]").len();

            let budget = 80usize.saturating_sub(header);
            let mut acc = 0usize;
            let mut count = 0usize;
            for s in &encoded {
                acc += 1 + s.len();
                if acc < budget {
                    count += 1;
                } else {
                    break;
                }
            }
            count.max(1).min(encoded.len())
        } else if quote {
            n.min(if max == 0 { STR_VEC_LEN as usize } else { max })
                .min(encoded.len())
        } else {
            n.min(if max == 0 { 10 } else { max }).min(encoded.len())
        };

        let mut text = encoded[..show].join(" ");
        if n > show {
            text.push_str(" ...");
        }
        text
    }
}

unsafe fn str_preview_reals_or_ints(x: SEXP, max: usize) -> String {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            return str_preview_ints(x, max);
        }

        let n = XLENGTH(x) as usize;
        if n == 0 {
            return String::new();
        }
        let integer_like = str_numeric_integer_like(x, n.min(round_2_5()));
        let v_len = if integer_like {
            round_2_5()
        } else {
            (1.25 * STR_VEC_LEN).round() as usize
        };
        let show = n.min(v_len);
        let parts = if TYPEOF(x) == SEXPTYPE::REALSXP {
            str_format_real_slice(x, show)
        } else {
            (0..show)
                .map(|i| {
                    let v = INTEGER_ELT(x, i as std::os::raw::c_int);
                    if v == NA_INTEGER {
                        "NA".to_string()
                    } else {
                        v.to_string()
                    }
                })
                .collect()
        };
        let mut text = parts.join(" ");
        if n > show {
            text.push_str(" ...");
        }
        text
    }
}

/// GNU `format(object[seq_len(ile)], trim=TRUE, drop0trailing=TRUE)` under digits.d.
unsafe fn str_format_real_slice(x: SEXP, show: usize) -> Vec<String> {
    unsafe {
        let tmp = Rf_allocVector3(SEXPTYPE::REALSXP, show as R_xlen_t);
        if tmp.is_null() {
            return (0..show)
                .map(|i| str_format_real(REAL_ELT(x, i as std::os::raw::c_int)))
                .collect();
        }
        let _tmp = protect(tmp);
        for i in 0..show {
            *REAL(tmp).add(i) = REAL_ELT(x, i as std::os::raw::c_int);
        }
        let old = crate::mainutils::format::format_get_R_print();
        crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
            digits: str_digits_d(),
            scipen: old.scipen,
            na_width: old.na_width,
            na_width_noquote: old.na_width_noquote,
        });
        let mut w = 0;
        let mut d = 0;
        let mut e = 0;

        crate::mainutils::format::formatRealS(tmp, show as R_xlen_t, &mut w, &mut d, &mut e, 0);
        let mut parts = Vec::with_capacity(show);
        for i in 0..show {
            let v = REAL_ELT(tmp, i as std::os::raw::c_int);
            if crate::sexp::ffi::is_na_real(v) {
                parts.push("NA".to_string());
                continue;
            }
            if v.is_nan() {
                parts.push("NaN".to_string());
                continue;
            }

            let encoded = crate::mainutils::printutils::EncodeReal0(v, w, d, e, STR_OUT_DEC);
            let raw = if encoded.is_null() {
                String::new()
            } else {
                CStr::from_ptr(encoded).to_string_lossy().into_owned()
            };
            parts.push(str_drop0trailing(raw.trim()));
        }
        crate::mainutils::format::format_set_R_print(old);
        parts
    }
}

fn round_2_5() -> usize {
    (2.5 * STR_VEC_LEN).round() as usize
}

/// GNU encodeString: wrap in `"` and escape `\`, `"`.
fn str_encode_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn str_emit_line(line: &str) {
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(&format!("{line}\n"));
    } else {
        println!("{line}");
    }
}

pub(crate) fn emit_print_text(text: &str) {
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(text);
    } else {
        print!("{text}");
    }
}

unsafe fn str_emit_nonstandard_attrs(x: SEXP, skip: &[&str]) {
    unsafe {
        let mut attrs = crate::sexp::accessors::ATTRIB(x);
        while !attrs.is_null() && attrs != R_NilValue() {
            let tag = TAG(attrs);
            let name = if tag.is_null() || tag == R_NilValue() {
                String::new()
            } else {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            };
            if !name.is_empty() && !skip.iter().any(|s| *s == name) {
                let summary = str_atomic_summary_opts(CAR(attrs), true, true);
                let sep = if summary.starts_with("Class") || summary.starts_with("List") {
                    ""
                } else {
                    " "
                };
                str_emit_line(&format!(" - attr(*, \"{name}\")={sep}{summary}"));
            }
            attrs = CDR(attrs);
        }
    }
}

/// R's `str(x)` — compact structure display.
pub unsafe fn do_str(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // utils::str(digits.d = .): numeric formatting precision override.
        let mut str_digits_arg: Option<i32> = None;
        {
            let digits_sym = Rf_install(c"digits.d".as_ptr());
            let mut p = CDR(args);
            while !p.is_null() && p != R_NilValue() {
                if TAG(p) == digits_sym {
                    let v = CAR(p);
                    if !v.is_null() && v != R_NilValue() {
                        if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                            str_digits_arg = Some(*INTEGER(v));
                        } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                            str_digits_arg = Some(*REAL(v) as i32);
                        }
                    }
                    break;
                }
                p = CDR(p);
            }
        }
        let _str_digits_guard = StrDigitsGuard::new(str_digits_arg);
        let mut x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::PROMSXP {
            x = crate::sexp::envir::forcePromise(x);
        }
        if x.is_null() || x == R_NilValue() {
            str_emit_line(" NULL");
            return R_NilValue();
        }
        if crate::mainutils::coerce::IS_S4_OBJECT(x) != 0 && !sexp_has_class_name(x, "envRefClass")
        {
            str_s4_formal_class(x);
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let t = TYPEOF(x);
        let n = crate::sexp::constructors::Rf_length(x) as R_xlen_t;

        // str.default for is.language && !is.expression: prints
        // " language "/" symbol " followed by the deparsed object
        // (braced blocks collapsed to "{ ... }" on one line). The generic
        // vector path below must not see these: XLENGTH of a pairlist node
        // is garbage.
        if sexp_has_class_name(x, "formula") {
            str_emit_line(&str_language_summary(x));
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::EXPRSXP {
            // str.default for expressions: "  expression(...)" with at most
            // three elements shown (round(0.75 * vec.len)) and " ..." when
            // truncated.
            if t == SEXPTYPE::EXPRSXP {
                let n_expr = XLENGTH(x);
                let show = n_expr.min(3);
                let _sub_guard;
                let deparse_target = if show == n_expr {
                    x
                } else {
                    let sub = Rf_allocVector3(SEXPTYPE::EXPRSXP, show);
                    _sub_guard = protect(sub);
                    for i in 0..show {
                        SET_VECTOR_ELT(sub, i, VECTOR_ELT(x, i));
                    }
                    sub
                };
                let deparsed = crate::mainutils::deparse::deparse_symbolic(deparse_target, true);
                let _deparsed_guard = protect(deparsed);
                let mut text = String::new();
                for i in 0..XLENGTH(deparsed) {
                    if i > 0 {
                        text.push(' ');
                    }
                    text.push_str(&elt_to_string(deparsed, i));
                }
                if show < n_expr {
                    text.push_str(" ...");
                }
                str_emit_line(&format!("  {text}"));
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                return x;
            }
            let deparsed = crate::mainutils::deparse::deparse_symbolic(x, true);
            let _deparsed_guard = protect(deparsed);
            let mut lines: Vec<String> = (0..XLENGTH(deparsed))
                .map(|i| elt_to_string(deparsed, i))
                .collect();
            let last = lines.len().saturating_sub(1);
            if t == SEXPTYPE::LANGSXP
                && lines.len() > 1
                && lines[0].trim() == "{"
                && lines[last].trim() == "}"
                && lines.len() >= 3
            {
                // str.default: trimEnds each middle line (leading space run
                // becomes a single space, trailing spaces stripped) and join
                // with ";".
                let middles: Vec<String> = lines[1..last]
                    .iter()
                    .map(|l| {
                        let body = l.trim_start_matches(' ');
                        let lead = l.len() - body.len();
                        let trimmed = if lead > 0 {
                            format!(" {body}")
                        } else {
                            body.to_string()
                        };
                        trimmed.trim_end_matches(' ').to_string()
                    })
                    .collect();
                lines = vec![lines[0].clone(), middles.join(";"), lines[last].clone()];
            }
            let prefix = if t == SEXPTYPE::LANGSXP {
                let mode_chars = crate::eval::attrib_core::language_implicit_class_chars(x);
                let mode = std::ffi::CStr::from_ptr(CHAR(mode_chars)).to_string_lossy();
                if mode.as_ref() == "(" {
                    " language, mode \"(\":".to_string()
                } else {
                    " language".to_string()
                }
            } else {
                " symbol".to_string()
            };
            str_emit_line(&format!("{} {}", prefix, lines.join(" ")));
            str_emit_nonstandard_attrs(x, &["names"]);
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        if t == SEXPTYPE::VECSXP {
            let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
            let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
            let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
            let is_df = if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == "data.frame")
            } else {
                false
            };

            if is_df {
                let ncol = n;
                let nrow = if ncol > 0 {
                    let first = VECTOR_ELT(x, 0);
                    if first.is_null() { 0 } else { XLENGTH(first) }
                } else {
                    0
                };
                let extra_classes: Vec<String> =
                    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                        (0..XLENGTH(class))
                            .map(|i| elt_to_string(class, i))
                            .filter(|c| c != "data.frame")
                            .collect()
                    } else {
                        Vec::new()
                    };
                let var_word = if ncol == 1 { "variable" } else { "variables" };
                let colon = if ncol > 0 { ":" } else { "" };
                if extra_classes.is_empty() {
                    str_emit_line(&format!(
                        "'data.frame':\t{nrow} obs. of  {ncol} {var_word}{colon}"
                    ));
                } else {
                    let quoted: Vec<String> =
                        extra_classes.iter().map(|c| format!("'{c}'")).collect();
                    str_emit_line(&format!(
                        "Classes {} and 'data.frame':\t{nrow} obs. of  {ncol} {var_word}{colon}",
                        quoted.join(", ")
                    ));
                }
                let raw_names: Vec<String> = (0..ncol)
                    .map(|i| {
                        if has_names && i < XLENGTH(names) {
                            elt_to_string(names, i)
                        } else {
                            format!("{}", i + 1)
                        }
                    })
                    .collect();
                let mut list_len = ncol;
                let mut p = CDR(args);
                while !p.is_null() && p != R_NilValue() {
                    let tag = TAG(p);
                    if !tag.is_null() && tag != R_NilValue() {
                        let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                            crate::sexp::accessors::PRINTNAME(tag),
                        ))
                        .to_string_lossy();
                        if name == "list.len" {
                            let v = CAR(p);
                            if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                                list_len = (*INTEGER(v) as R_xlen_t).max(0);
                            } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                                list_len = (*REAL(v) as R_xlen_t).max(0);
                            }
                        }
                    }
                    p = CDR(p);
                }
                let show = ncol.min(list_len);
                let name_width = raw_names
                    .iter()
                    .take(show as usize)
                    .map(String::len)
                    .max()
                    .unwrap_or(0);
                for i in 0..show {
                    let name = format!("{:<name_width$}", raw_names[i as usize]);
                    let elem = VECTOR_ELT(x, i as i64);
                    str_emit_line(&format!(
                        " $ {name}: {}",
                        str_atomic_summary_opts(elem, false, true)
                    ));
                    if let Some(extra) = str_child_dimnames_lines(elem) {
                        str_emit_line(&extra);
                    }
                }
                if show < ncol {
                    str_emit_line(" [list output truncated]");
                }
                str_emit_nonstandard_attrs(x, &["names", "class", "row.names"]);
            } else {
                let mut max_level = 100i64;
                let mut list_len = 99i64;
                let mut p = CDR(args);
                while !p.is_null() && p != R_NilValue() {
                    let tag = TAG(p);
                    if !tag.is_null() && tag != R_NilValue() {
                        let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                            crate::sexp::accessors::PRINTNAME(tag),
                        ))
                        .to_string_lossy()
                        .into_owned();
                        let v = CAR(p);
                        let num = if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                            *INTEGER(v) as i64
                        } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                            *REAL(v) as i64
                        } else {
                            i64::MIN
                        };
                        if num != i64::MIN && num != crate::sexp::ffi::NA_INTEGER as i64 {
                            if name == "max.level" {
                                max_level = num;
                            } else if name == "list.len" {
                                list_len = num;
                            }
                        }
                    }
                    p = CDR(p);
                }
                STR_LEVEL.store(max_level, std::sync::atomic::Ordering::Relaxed);
                STR_LIST_LEN.store(list_len, std::sync::atomic::Ordering::Relaxed);
                let show = n.min(list_len.max(0) as R_xlen_t);
                let class_attr = crate::sexp::attrib_core::getAttrib(
                    x,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                );
                let mut header = format!("List of {n}");
                if TYPEOF(class_attr) == SEXPTYPE::STRSXP && XLENGTH(class_attr) > 0 {
                    let cname = elt_to_string(class_attr, 0);
                    let len_args = Rf_cons(x, R_NilValue());
                    let _len_guard = protect(len_args);
                    let len_sexp = crate::eval::arithmetic::do_length(
                        R_NilValue(),
                        R_NilValue(),
                        len_args,
                        _rho,
                    );
                    let dispatched =
                        if TYPEOF(len_sexp) == SEXPTYPE::INTSXP && XLENGTH(len_sexp) > 0 {
                            *INTEGER(len_sexp) as i64
                        } else if TYPEOF(len_sexp) == SEXPTYPE::REALSXP && XLENGTH(len_sexp) > 0 {
                            *REAL(len_sexp) as i64
                        } else {
                            n as i64
                        };
                    if dispatched != n as i64 {
                        header = format!("Class '{cname}'  hidden list of {n}");
                    }
                }
                str_emit_line(&header);
                let raw_names: Vec<String> = (0..show)
                    .map(|i| {
                        if has_names && i < XLENGTH(names) {
                            elt_to_string(names, i)
                        } else {
                            String::new()
                        }
                    })
                    .collect();
                let name_width = raw_names.iter().map(String::len).max().unwrap_or(0);
                for i in 0..show {
                    let name = format!("{:<name_width$}", raw_names[i as usize]);
                    let elem = VECTOR_ELT(x, i as i64);
                    str_emit_line(&format!(
                        " $ {name}: {}",
                        str_atomic_summary_opts(elem, true, true)
                    ));

                    if let Some(extra) = str_child_dimnames_lines(elem) {
                        str_emit_line(&extra);
                    }
                }
            }
        } else {
            str_emit_line(&format!(" {}", str_atomic_summary(x)));
            str_emit_nonstandard_attrs(x, &["class", "tsp", "dim", "levels"]);
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
