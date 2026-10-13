//! Terms/formula parsing: C_termsform, mark_terms, term labels, terms/scope ops

use super::*;

fn collect_term_labels(expr: SEXP, out: &mut Vec<String>, nodes: &mut Vec<SEXP>, sign: i32) {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return;
        }
        let record = |name: String, node: SEXP, out: &mut Vec<String>, nodes: &mut Vec<SEXP>| {
            if sign < 0 {
                if let Some(i) = out.iter().position(|s| s == &name) {
                    out.remove(i);
                    nodes.remove(i);
                }
            } else if !out.iter().any(|s| s == &name) {
                out.push(name);
                nodes.push(node);
            }
        };
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let Some(name) = symbol_chars(expr) else {
                return;
            };
            if !matches!(
                name.as_str(),
                "~" | "+" | "-" | "*" | ":" | "/" | "^" | "I" | "("
            ) {
                let stored = if crate::mainutils::deparse::local_parse_data::is_valid_r_name_bytes(
                    name.as_bytes(),
                ) {
                    name
                } else {
                    format!("`{name}`")
                };
                record(stored, expr, out, nodes);
            }
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP {
            let op = CAR(expr);
            if let Some(name) = symbol_chars(op) {
                if name == "offset" {
                    return;
                }
                if name == "+" {
                    let mut cell = CDR(expr);
                    while !cell.is_null() && cell != R_NilValue() {
                        collect_term_labels(CAR(cell), out, nodes, sign);
                        cell = CDR(cell);
                    }
                    return;
                }
                if name == "-" {
                    let n = crate::sexp::constructors::Rf_length(expr);
                    if n == 2 {
                        collect_term_labels(CADR(expr), out, nodes, -sign);
                    } else {
                        collect_term_labels(CADR(expr), out, nodes, sign);
                        collect_term_labels(CADDR(expr), out, nodes, -sign);
                    }
                    return;
                }
                if name == "*" {
                    let mut left_labels = Vec::new();
                    let mut left_nodes = Vec::new();
                    let mut right_labels = Vec::new();
                    let mut right_nodes = Vec::new();
                    collect_term_labels(CADR(expr), &mut left_labels, &mut left_nodes, 1);
                    collect_term_labels(CADDR(expr), &mut right_labels, &mut right_nodes, 1);
                    let mut combined_labels = Vec::new();
                    let mut combined_nodes = Vec::new();
                    for (label, node) in left_labels.iter().zip(left_nodes.iter().copied()) {
                        combined_labels.push(label.clone());
                        combined_nodes.push(node);
                    }
                    for (label, node) in right_labels.iter().zip(right_nodes.iter().copied()) {
                        combined_labels.push(label.clone());
                        combined_nodes.push(node);
                    }
                    for left in &left_labels {
                        for right in &right_labels {
                            combined_labels.push(format!("{left}:{right}"));
                            combined_nodes.push(expr);
                        }
                    }
                    for (label, node) in combined_labels.into_iter().zip(combined_nodes) {
                        record(label, node, out, nodes);
                    }
                    return;
                }
                if name == "^" {
                    // GNU cross power: `(a+b)^2` expands to the main
                    // effects plus all interactions up to order 2.
                    let exponent = formula_power_exponent(expr);
                    let Some(k) = exponent else {
                        // Non-numeric exponent (e.g. I()): keep operand
                        // terms as-is like the generic recursion does.
                        let mut cell = CDR(expr);
                        while !cell.is_null() && cell != R_NilValue() {
                            collect_term_labels(CAR(cell), out, nodes, sign);
                            cell = CDR(cell);
                        }
                        return;
                    };
                    let mut base_labels = Vec::new();
                    let mut base_nodes = Vec::new();
                    collect_term_labels(CADR(expr), &mut base_labels, &mut base_nodes, 1);
                    // Unique atomic factors, in first-appearance order.
                    let mut atoms: Vec<&str> = Vec::new();
                    let mut atom_nodes: Vec<SEXP> = Vec::new();
                    for (label, node) in base_labels.iter().zip(base_nodes.iter().copied()) {
                        for part in label.split(':') {
                            if !atoms.iter().any(|&a| a == part) {
                                atoms.push(part);
                                atom_nodes.push(node);
                            }
                        }
                    }
                    let k = (k as usize).min(atoms.len());
                    for size in 1..=k {
                        let mut combo: Vec<usize> = (0..size).collect();
                        loop {
                            let label = combo
                                .iter()
                                .map(|&i| atoms[i])
                                .collect::<Vec<_>>()
                                .join(":");
                            // Main-effect rows keep their own variable
                            // node so the variables list never sees the
                            // synthetic `(...)^k` expression.
                            let node = if size == 1 {
                                atom_nodes[combo[0]]
                            } else {
                                expr
                            };
                            record(label, node, out, nodes);
                            // Next combination, last index that can advance.
                            let Some(advance) =
                                (0..size).rev().find(|&i| combo[i] < atoms.len() - size + i)
                            else {
                                break;
                            };
                            combo[advance] += 1;
                            for i in (advance + 1)..size {
                                combo[i] = combo[i - 1] + 1;
                            }
                        }
                    }
                    return;
                }
                if matches!(name.as_str(), "/" | "(" | "~") {
                    let mut cell = CDR(expr);
                    while !cell.is_null() && cell != R_NilValue() {
                        collect_term_labels(CAR(cell), out, nodes, sign);
                        cell = CDR(cell);
                    }
                    return;
                }
                if name == ":" {
                    let mut parts = Vec::new();
                    collect_formula_symbols(CADR(expr), &mut parts);
                    collect_formula_symbols(CADDR(expr), &mut parts);
                    if !parts.is_empty() {
                        let quoted: Vec<String> = parts
                            .into_iter()
                            .map(|name| {
                                if crate::mainutils::deparse::local_parse_data::is_valid_r_name_bytes(
                                    name.as_bytes(),
                                ) {
                                    name
                                } else {
                                    format!("`{name}`")
                                }
                            })
                            .collect();
                        record(quoted.join(":"), expr, out, nodes);
                    }
                    return;
                }
                record(deparse_call(expr), expr, out, nodes);
                return;
            }
        }
    }
}

/// Numeric exponent of a `base ^ k` formula node, if it is a literal
/// constant. Anything else (symbols, calls) yields `None`.
fn formula_power_exponent(expr: SEXP) -> Option<i32> {
    unsafe {
        let exponent = CADDR(expr);
        if exponent.is_null() || exponent == R_NilValue() {
            return None;
        }
        let value = match TYPEOF(exponent) {
            t if t == SEXPTYPE::INTSXP => *INTEGER(exponent) as f64,
            t if t == SEXPTYPE::REALSXP => *REAL(exponent),
            _ => return None,
        };
        if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= u32::MAX as f64 {
            Some(value as i32)
        } else {
            None
        }
    }
}

/// `offset(expr)` is a variable of the model frame, not a term.
fn collect_offsets(expr: SEXP, out: &mut Vec<SEXP>) {
    unsafe {
        if expr.is_null() || expr == R_NilValue() || TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return;
        }
        let op = CAR(expr);
        if !op.is_null() && TYPEOF(op) == SEXPTYPE::SYMSXP {
            let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op)))
                .to_string_lossy()
                .into_owned();
            if name == "offset" {
                // walk_vars already records this call. Pushing it again makes
                // model.offset add the same column twice (PR#18456).
                if !out.contains(&expr) {
                    out.push(expr);
                }
                return;
            }
        }
        let mut cell = CDR(expr);
        while !cell.is_null() && cell != R_NilValue() {
            collect_offsets(CAR(cell), out);
            cell = CDR(cell);
        }
    }
}

fn symbol_print_name(sym: SEXP) -> String {
    symbol_chars(sym).unwrap_or_default()
}

fn formula_constant(expr: SEXP) -> Option<f64> {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return None;
        }
        if TYPEOF(expr) == SEXPTYPE::INTSXP && XLENGTH(expr) == 1 {
            let v = *INTEGER(expr);
            if v == NA_INTEGER {
                None
            } else {
                Some(v as f64)
            }
        } else if TYPEOF(expr) == SEXPTYPE::REALSXP && XLENGTH(expr) == 1 {
            Some(*REAL(expr))
        } else {
            None
        }
    }
}

/// `~ x - 1` and `~ x + 0` have no intercept. `+ 1` puts it back.
fn formula_intercept(expr: SEXP) -> i32 {
    let mut intercept = 1;
    note_formula_intercept(expr, 1, &mut intercept);
    intercept
}

fn note_formula_intercept(expr: SEXP, sign: i32, intercept: &mut i32) {
    unsafe {
        if let Some(value) = formula_constant(expr) {
            if value == 1.0 {
                *intercept = if sign < 0 { 0 } else { 1 };
            } else if value == 0.0 && sign > 0 {
                *intercept = 0;
            }
            return;
        }
        if expr.is_null() || TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return;
        }
        let op = symbol_print_name(CAR(expr));
        if op == "~" {
            let mut cell = CDR(expr);
            let mut last = R_NilValue();
            while !cell.is_null() && cell != R_NilValue() {
                last = CAR(cell);
                cell = CDR(cell);
            }
            note_formula_intercept(last, sign, intercept);
        } else if op == "+" {
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                note_formula_intercept(CAR(cell), sign, intercept);
                cell = CDR(cell);
            }
        } else if op == "-" {
            let n = crate::sexp::constructors::Rf_length(expr);
            if n == 2 {
                note_formula_intercept(CADR(expr), -sign, intercept);
            } else if n >= 3 {
                note_formula_intercept(CADR(expr), sign, intercept);
                note_formula_intercept(CADDR(expr), -sign, intercept);
            }
        } else if op == "(" {
            note_formula_intercept(CADR(expr), sign, intercept);
        }
    }
}

fn formula_response_name(form: SEXP) -> String {
    unsafe {
        if form.is_null() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            return "y".to_string();
        }
        let rhs = CDR(form);
        if rhs.is_null() || rhs == R_NilValue() {
            return "y".to_string();
        }
        let lhs = CAR(rhs);
        if lhs.is_null() || lhs == R_NilValue() {
            return "y".to_string();
        }
        if TYPEOF(lhs) == SEXPTYPE::SYMSXP {
            let name = symbol_print_name(lhs);
            return if name.is_empty() {
                "y".to_string()
            } else {
                name
            };
        }
        // GNU terms.c names non-symbol responses (e.g. a cbind(...) LHS)
        // by deparsing the expression.
        let deparsed = crate::mainutils::deparse::deparse1WithCutoff(
            lhs,
            false,
            500,
            true,
            crate::mainutils::deparse::DEFAULT_USER_DEPARSE,
            0,
        );
        if deparsed.is_null()
            || deparsed == R_NilValue()
            || TYPEOF(deparsed) != SEXPTYPE::STRSXP
            || XLENGTH(deparsed) == 0
        {
            return "y".to_string();
        }
        let name = crate::mainutils::essentials::elt_to_string(deparsed, 0);
        if name.is_empty() {
            "y".to_string()
        } else {
            name
        }
    }
}

fn mark_terms(form: SEXP, response: i32, specials: SEXP) -> SEXP {
    unsafe {
        // GNU termsform replaces the whole attribute list with a fresh one
        // (dropping the input formula's class/.Environment); the R wrapper
        // appends class and .Environment afterwards.
        crate::sexp::accessors::SET_ATTRIB(form, R_NilValue());
        let mut labels = Vec::new();
        // Term labels come from the RHS. The response expression
        // (y1 - y2) must not contribute covariates.
        let rhs = if response > 0 && TYPEOF(form) == SEXPTYPE::LANGSXP {
            CADDR(form)
        } else {
            form
        };
        let intercept = Rf_ScalarInteger(formula_intercept(rhs));
        let _icpt = protect(intercept);
        let mut term_nodes: Vec<SEXP> = Vec::new();
        collect_term_labels(rhs, &mut labels, &mut term_nodes, 1);
        // GNU termsform step 3: stably reorder terms by size (main effects
        // first, then two-way interactions, ...).
        {
            let mut order: Vec<usize> = (0..labels.len()).collect();
            order.sort_by_key(|&i| split_top_level_colon(&labels[i]).len());
            let sorted_labels: Vec<String> = order.iter().map(|&i| labels[i].clone()).collect();
            let sorted_nodes: Vec<SEXP> = order.iter().map(|&i| term_nodes[i]).collect();
            labels = sorted_labels;
            term_nodes = sorted_nodes;
        }
        let lab = Rf_allocVector3(SEXPTYPE::STRSXP, labels.len() as i64);
        let _lb = protect(lab);
        for (i, name) in labels.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(lab, i as i64, Rf_mkChar(c.as_ptr()));
        }

        let order = Rf_allocVector3(SEXPTYPE::INTSXP, labels.len() as i64);
        let _ord = protect(order);
        for i in 0..labels.len() {
            let mut depth = 0i32;
            let mut colons = 0i32;
            for ch in labels[i].chars() {
                match ch {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth = depth.saturating_sub(1),
                    ':' if depth == 0 => colons += 1,
                    _ => {}
                }
            }
            *INTEGER(order).add(i) = colons + 1;
        }

        let mut var_syms: Vec<SEXP> = Vec::new();
        if response > 0 {
            let resp = CDR(form);
            if !resp.is_null() && resp != R_NilValue() {
                var_syms.push(CAR(resp));
            }
        }
        fn walk_vars(expr: SEXP, var_syms: &mut Vec<SEXP>) {
            unsafe {
                if expr.is_null() || expr == R_NilValue() {
                    return;
                }
                if TYPEOF(expr) == SEXPTYPE::SYMSXP {
                    if !var_syms
                        .iter()
                        .any(|&s| s == expr || formula_vars_equal(s, expr))
                    {
                        var_syms.push(expr);
                    }
                    return;
                }
                if TYPEOF(expr) != SEXPTYPE::LANGSXP {
                    return;
                }
                let name = symbol_print_name(CAR(expr));
                if name == "-" {
                    // `x - 1` / `x - 0` strips the intercept; the numeric
                    // constant is not a variable.
                    let mut cell = CDR(expr);
                    while !cell.is_null() && cell != R_NilValue() {
                        let arg = CAR(cell);
                        let constant = (TYPEOF(arg) == SEXPTYPE::INTSXP
                            || TYPEOF(arg) == SEXPTYPE::REALSXP
                            || TYPEOF(arg) == SEXPTYPE::LGLSXP)
                            && XLENGTH(arg) == 1;
                        if !constant {
                            walk_vars(arg, var_syms);
                        }
                        cell = CDR(cell);
                    }
                    return;
                }
                if name == "^" && formula_power_exponent(expr).is_some() {
                    // Numeric cross power: only the base's atoms are
                    // variables, never the whole `(...)^k` node.
                    walk_vars(CADR(expr), var_syms);
                    return;
                }
                if name == "~"
                    || name == "+"
                    || name == "*"
                    || name == ":"
                    || name == "/"
                    || name == "%in%"
                    || name == "("
                {
                    let mut cell = CDR(expr);
                    while !cell.is_null() && cell != R_NilValue() {
                        walk_vars(CAR(cell), var_syms);
                        cell = CDR(cell);
                    }
                    return;
                }
                if !var_syms
                    .iter()
                    .any(|&s| s == expr || formula_vars_equal(s, expr))
                {
                    var_syms.push(expr);
                }
            }
        }
        walk_vars(rhs, &mut var_syms);
        for (name, node) in labels.iter().zip(term_nodes.iter().copied()) {
            if is_formula_interaction(name) {
                for raw_part in split_top_level_colon(name) {
                    // Term labels backtick-quote non-syntactic names; the
                    // symbol's print name never contains the quotes.
                    let part = raw_part
                        .strip_prefix('`')
                        .and_then(|s| s.strip_suffix('`'))
                        .unwrap_or(&raw_part);
                    if var_syms
                        .iter()
                        .any(|&s| symbol_print_name(s) == part || deparse_call(s) == part)
                    {
                        continue;
                    }
                    if let Some(node) = term_nodes
                        .iter()
                        .copied()
                        .find(|&node| symbol_print_name(node) == part || deparse_call(node) == part)
                    {
                        var_syms.push(node);
                        continue;
                    }
                    let c = std::ffi::CString::new(part).unwrap_or_default();
                    var_syms.push(crate::sexp::symbol::Rf_install(c.as_ptr()));
                }
            } else if !var_syms.iter().any(|&s| {
                s == node
                    || formula_vars_equal(s, node)
                    || (!deparse_call(node).is_empty() && deparse_call(s) == deparse_call(node))
            }) {
                var_syms.push(node);
            }
        }
        collect_offsets(form, &mut var_syms);
        let mut offset_idx: Vec<i32> = Vec::new();
        for (i, &sym) in var_syms.iter().enumerate() {
            if TYPEOF(sym) == SEXPTYPE::LANGSXP {
                let op = CAR(sym);
                if !op.is_null()
                    && TYPEOF(op) == SEXPTYPE::SYMSXP
                    && std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op))).to_bytes() == b"offset"
                {
                    offset_idx.push((i + 1) as i32);
                }
            }
        }
        let mut varlist = R_NilValue();
        for &sym in var_syms.iter().rev() {
            varlist = Rf_cons(sym, varlist);
        }
        let list_sym = crate::sexp::symbol::Rf_install(c"list".as_ptr());
        let variables = Rf_cons(list_sym, varlist);
        if !variables.is_null() {
            crate::sexp::accessors::SET_TYPEOF(variables, SEXPTYPE::LANGSXP.as_c_int());
        }
        let _vl = protect(variables);
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"variables".as_ptr()),
            variables,
        );
        if !offset_idx.is_empty() {
            let ov = Rf_allocVector3(SEXPTYPE::INTSXP, offset_idx.len() as i64);
            let _ov = protect(ov);
            for (i, &v) in offset_idx.iter().enumerate() {
                *INTEGER(ov).add(i) = v;
            }
            crate::sexp::attrib_core::setAttrib(
                form,
                crate::sexp::symbol::Rf_install(c"offset".as_ptr()),
                ov,
            );
        }
        let mut specials_spec = R_NilValue();
        if TYPEOF(specials) == SEXPTYPE::STRSXP && XLENGTH(specials) > 0 {
            let mut spec = R_NilValue();
            let nspec = XLENGTH(specials) as usize;
            let mut roots = Vec::new();
            for s in (0..nspec).rev() {
                let want = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(specials, s as i64)))
                    .to_string_lossy()
                    .into_owned();
                let mut hits: Vec<i32> = Vec::new();
                for (i, &sym) in var_syms.iter().enumerate() {
                    if TYPEOF(sym) == SEXPTYPE::LANGSXP {
                        let op = CAR(sym);
                        if !op.is_null()
                            && TYPEOF(op) == SEXPTYPE::SYMSXP
                            && std::ffi::CStr::from_ptr(CHAR(PRINTNAME(op))).to_string_lossy()
                                == want
                        {
                            hits.push((i + 1) as i32);
                        }
                    }
                }
                let val = if hits.is_empty() {
                    R_NilValue()
                } else {
                    let iv = Rf_allocVector3(SEXPTYPE::INTSXP, hits.len() as i64);
                    for (k, &h) in hits.iter().enumerate() {
                        *INTEGER(iv).add(k) = h;
                    }
                    iv
                };
                let _val_guard = protect(val);
                spec = crate::sexp::constructors::Rf_cons(val, spec);
                roots.push(protect(spec));
                let tag = crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(want).unwrap_or_default().as_ptr(),
                );
                crate::sexp::accessors::SETTAG(spec, tag);
            }
            let _spec_guard = protect(spec);
            specials_spec = spec;
        }
        let mut vars = Vec::new();
        for lab in &labels {
            for part in split_top_level_colon(lab) {
                if !vars.iter().any(|s| s == part) {
                    vars.push(part.to_string());
                }
            }
        }
        let nrows = vars.len() + if response > 0 { 1 } else { 0 };
        let ncols = labels.len();
        if ncols > 0 && nrows > 0 {
            let fac = crate::mainutils::array::allocMatrix(
                SEXPTYPE::INTSXP.as_c_int(),
                nrows as i32,
                ncols as i32,
            );
            let _f = protect(fac);
            for i in 0..nrows * ncols {
                *INTEGER(fac).add(i) = 0;
            }
            let row0 = if response > 0 { 1 } else { 0 };
            for (j, lab) in labels.iter().enumerate() {
                let parts = split_top_level_colon(lab);
                for (i, var) in vars.iter().enumerate() {
                    if parts.iter().any(|p| *p == var) {
                        *INTEGER(fac).add(row0 + i + j * nrows) = 1;
                    }
                }
            }
            let rn = Rf_allocVector3(SEXPTYPE::STRSXP, nrows as i64);
            let _rn = protect(rn);
            let mut r = 0;
            if response > 0 {
                let resp = formula_response_name(form);
                let c = std::ffi::CString::new(resp).unwrap_or_default();
                SET_STRING_ELT(rn, 0, Rf_mkChar(c.as_ptr()));
                r = 1;
            }
            for var in &vars {
                let c = std::ffi::CString::new(var.as_str()).unwrap_or_default();
                SET_STRING_ELT(rn, r, Rf_mkChar(c.as_ptr()));
                r += 1;
            }
            let cn = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as i64);
            let _cn = protect(cn);
            for (j, lab) in labels.iter().enumerate() {
                let c = std::ffi::CString::new(lab.as_str()).unwrap_or_default();
                SET_STRING_ELT(cn, j as i64, Rf_mkChar(c.as_ptr()));
            }
            let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _dn = protect(dn);
            SET_VECTOR_ELT(dn, 0, rn);
            SET_VECTOR_ELT(dn, 1, cn);
            crate::sexp::attrib_core::setAttrib(
                fac,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dn,
            );
            crate::sexp::attrib_core::setAttrib(
                form,
                crate::sexp::symbol::Rf_install(c"factors".as_ptr()),
                fac,
            );
        }
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
            lab,
        );
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"specials".as_ptr()),
            specials_spec,
        );
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"order".as_ptr()),
            order,
        );
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"intercept".as_ptr()),
            intercept,
        );
        crate::sexp::attrib_core::setAttrib(
            form,
            crate::sexp::symbol::Rf_install(c"response".as_ptr()),
            Rf_ScalarInteger(response),
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _cl = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"terms".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"formula".as_ptr()));
        crate::sexp::attrib_core::setAttrib(form, crate::sexp::attrib_core::R_ClassSymbol(), class);
        form
    }
}

/// `.` on a formula RHS is every data column except the response.
unsafe fn expand_formula_dot(form: SEXP, data: SEXP) {
    unsafe {
        if data.is_null()
            || data == R_NilValue()
            || (TYPEOF(data) != SEXPTYPE::VECSXP && TYPEOF(data) != SEXPTYPE::LISTSXP)
        {
            return;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) == 0 {
            return;
        }
        let response = CADR(form);
        // GNU excludes every variable named in the response expression
        // (e.g. `log(Days + 2.5)` excludes Days), not just a bare symbol.
        let mut response_names: Vec<String> = Vec::new();
        if !response.is_null() && response != R_NilValue() {
            if TYPEOF(response) == SEXPTYPE::SYMSXP {
                response_names.push(
                    std::ffi::CStr::from_ptr(CHAR(PRINTNAME(response)))
                        .to_string_lossy()
                        .into_owned(),
                );
            } else {
                let mut parts = Vec::new();
                collect_formula_symbols(response, &mut parts);
                response_names.extend(parts);
            }
        }
        let mut cols = Vec::new();
        let mut seen = Vec::new();
        for i in 0..XLENGTH(names) {
            let name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)))
                .to_string_lossy()
                .into_owned();
            if seen.iter().any(|existing| existing == &name) {
                let msg = std::ffi::CString::new(format!(
                    "duplicated name '{name}' in data frame using '.'"
                ))
                .unwrap_or_default();
                crate::main::errors::Rf_error(msg.as_ptr());
            }
            seen.push(name.clone());
            if !response_names.contains(&name) {
                cols.push(name);
            }
        }
        if cols.is_empty() {
            return;
        }
        let rhs = CDR(CDR(form));
        if rhs.is_null() || rhs == R_NilValue() {
            return;
        }
        SETCAR(rhs, replace_formula_dot(CAR(rhs), &cols));
    }
}

unsafe fn replace_formula_dot(expr: SEXP, cols: &[String]) -> SEXP {
    unsafe {
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(expr)))
                .to_string_lossy()
                .into_owned();
            if name == "." {
                return plus_chain(cols);
            }
            return expr;
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP {
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                SETCAR(cell, replace_formula_dot(CAR(cell), cols));
                cell = CDR(cell);
            }
        }
        expr
    }
}

unsafe fn plus_chain(cols: &[String]) -> SEXP {
    unsafe {
        let plus = crate::sexp::symbol::Rf_install(c"+".as_ptr());
        let mut acc = crate::sexp::symbol::Rf_install(
            std::ffi::CString::new(cols[0].as_str())
                .unwrap_or_default()
                .as_ptr(),
        );
        for name in &cols[1..] {
            let sym = crate::sexp::symbol::Rf_install(
                std::ffi::CString::new(name.as_str())
                    .unwrap_or_default()
                    .as_ptr(),
            );
            acc = crate::sexp::constructors::Rf_lang3(plus, acc, sym);
        }
        acc
    }
}

fn formula_vars_equal(a: SEXP, b: SEXP) -> bool {
    if a == b {
        return true;
    }
    unsafe {
        let ta = TYPEOF(a);
        let tb = TYPEOF(b);
        if ta == SEXPTYPE::SYMSXP && tb == SEXPTYPE::SYMSXP {
            return symbol_print_name(a) == symbol_print_name(b);
        }
        if ta == SEXPTYPE::LANGSXP && tb == SEXPTYPE::LANGSXP {
            let mut pa = a;
            let mut pb = b;
            while !pa.is_null() && pa != R_NilValue() && !pb.is_null() && pb != R_NilValue() {
                if !formula_vars_equal(CAR(pa), CAR(pb)) {
                    return false;
                }
                pa = CDR(pa);
                pb = CDR(pb);
            }
            return (pa.is_null() || pa == R_NilValue()) && (pb.is_null() || pb == R_NilValue());
        }
        if ta == SEXPTYPE::CPLXSXP || tb == SEXPTYPE::CPLXSXP {
            return false;
        }
        if ta == SEXPTYPE::STRSXP || tb == SEXPTYPE::STRSXP {
            if ta == SEXPTYPE::STRSXP
                && tb == SEXPTYPE::STRSXP
                && XLENGTH(a) == 0
                && XLENGTH(b) == 0
            {
                return true;
            }
            if ta != SEXPTYPE::STRSXP
                || tb != SEXPTYPE::STRSXP
                || XLENGTH(a) != 1
                || XLENGTH(b) != 1
            {
                return false;
            }
            let sa = STRING_ELT(a, 0);
            let sb = STRING_ELT(b, 0);
            if sa == crate::sexp::globals::R_NaString() && sb == crate::sexp::globals::R_NaString()
            {
                return true;
            }
            return std::ffi::CStr::from_ptr(CHAR(sa)).to_bytes()
                == std::ffi::CStr::from_ptr(CHAR(sb)).to_bytes();
        }
        let num = |s: SEXP, t: i32| -> Option<f64> {
            if XLENGTH(s) != 1 {
                return None;
            }
            if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                let iv = *INTEGER(s);
                if iv == crate::sexp::ffi::NA_INTEGER {
                    Some(f64::NAN)
                } else {
                    Some(iv as f64)
                }
            } else if t == SEXPTYPE::REALSXP {
                let r = *REAL(s);
                if r.is_nan() { Some(f64::NAN) } else { Some(r) }
            } else {
                None
            }
        };
        match (num(a, ta), num(b, tb)) {
            (Some(x), Some(y)) if x.is_nan() && y.is_nan() => true,
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    }
}

/// GNU `.External(C_termsform, x, specials, data, keep.order, allowDotAsName)`.
pub unsafe fn termsform(args: SEXP) -> SEXP {
    unsafe {
        let args = CDR(args);
        let form = CAR(args);
        if form.is_null() || TYPEOF(form) != SEXPTYPE::LANGSXP {
            crate::main::errors::Rf_error(
                b"argument is not a valid model\0".as_ptr() as *const std::os::raw::c_char
            );
        }
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        if CAR(form) != tilde {
            crate::main::errors::Rf_error(
                b"argument is not a valid model\0".as_ptr() as *const std::os::raw::c_char
            );
        }
        let dup = crate::mainutils::duplicate::Rf_duplicate(form);
        let _d = protect(dup);
        let rest = CDR(dup);
        let third = if rest.is_null() {
            R_NilValue()
        } else {
            CDR(rest)
        };
        let response = if third.is_null() || third == R_NilValue() {
            0
        } else {
            1
        };
        let specials_cell = CDR(args);
        let specials = if specials_cell.is_null() || specials_cell == R_NilValue() {
            R_NilValue()
        } else {
            CAR(specials_cell)
        };
        let data = {
            let after = CDR(specials_cell);
            if after.is_null() || after == R_NilValue() {
                R_NilValue()
            } else {
                CAR(after)
            }
        };
        expand_formula_dot(dup, data);
        mark_terms(dup, response, specials)
    }
}

/// GNU `factor.scope(factor, scope)` — add terms not already in the model.
pub unsafe fn do_factor_scope(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let factor = CAR(args);
        let scope = CAR(CDR(args));
        let add = named_list_elt(scope, "add");
        let have = matrix_colnames(factor);
        let extra: Vec<String> = matrix_colnames(add)
            .into_iter()
            .filter(|n| !have.iter().any(|h| h == n))
            .collect();
        let add_out = Rf_allocVector3(SEXPTYPE::STRSXP, extra.len() as i64);
        let _a = protect(add_out);
        for (i, name) in extra.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(add_out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        let drop_out = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        let _d = protect(drop_out);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, drop_out);
        SET_VECTOR_ELT(result, 1, add_out);
        crate::mainutils::essentials::set_string_names(
            result,
            &["drop".to_string(), "add".to_string()],
        );
        result
    }
}

/// GNU `terms(object)` — `$terms`, or a formula as `c("terms","formula")`.
pub unsafe fn do_terms(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let t = named_list_elt(x, "terms");
        if !t.is_null() && t != R_NilValue() {
            return t;
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP || inherits_formula(x) {
            let rest = CDR(x);
            let third = if rest.is_null() {
                R_NilValue()
            } else {
                CDR(rest)
            };
            let response = if third.is_null() || third == R_NilValue() {
                0
            } else {
                1
            };
            return mark_terms(x, response, R_NilValue());
        }
        R_NilValue()
    }
}

/// GNU `delete.response(termobj)` — drop the LHS of a two-sided formula.
pub unsafe fn do_delete_response(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::LANGSXP {
            return x;
        }
        let rest = CDR(x);
        let third = if rest.is_null() {
            R_NilValue()
        } else {
            CDR(rest)
        };
        if third.is_null() || third == R_NilValue() {
            return mark_terms(x, 0, R_NilValue());
        }
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        mark_terms(Rf_lang2(tilde, CAR(third)), 0, R_NilValue())
    }
}

/// GNU `drop.terms(termobj, dropx)` — drop term.labels[dropx].
pub unsafe fn do_drop_terms(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let drop = CAR(CDR(args));
        let drop_i = if drop.is_null() || drop == R_NilValue() {
            0
        } else if TYPEOF(drop) == SEXPTYPE::INTSXP {
            *INTEGER(drop)
        } else if TYPEOF(drop) == SEXPTYPE::REALSXP {
            *REAL(drop) as i32
        } else {
            0
        };
        let labs = crate::sexp::attrib_core::getAttrib(
            obj,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        if labs.is_null() || labs == R_NilValue() || TYPEOF(labs) != SEXPTYPE::STRSXP {
            return do_delete_response(call, op, args, rho);
        }
        let n = XLENGTH(labs);
        let keep_n = if drop_i >= 1 && (drop_i as i64) <= n {
            n - 1
        } else {
            n
        };
        let kept = Rf_allocVector3(SEXPTYPE::STRSXP, keep_n);
        let _k = protect(kept);
        let mut j = 0i64;
        for i in 0..n {
            if drop_i >= 1 && i + 1 == drop_i as i64 {
                continue;
            }
            SET_STRING_ELT(kept, j, STRING_ELT(labs, i));
            j += 1;
        }
        let form = do_reformulate(call, op, Rf_cons(kept, R_NilValue()), rho);
        let _f = protect(form);
        let marked = mark_terms(form, 0, R_NilValue());
        let ic = crate::sexp::attrib_core::getAttrib(
            obj,
            crate::sexp::symbol::Rf_install(c"intercept".as_ptr()),
        );
        if !ic.is_null() && ic != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                marked,
                crate::sexp::symbol::Rf_install(c"intercept".as_ptr()),
                ic,
            );
        }
        marked
    }
}

fn term_label_strings(obj: SEXP) -> Vec<String> {
    unsafe {
        let labs = crate::sexp::attrib_core::getAttrib(
            obj,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        let mut out = Vec::new();
        if labs.is_null() || labs == R_NilValue() || TYPEOF(labs) != SEXPTYPE::STRSXP {
            return out;
        }
        for i in 0..XLENGTH(labs) {
            let raw = CHAR(STRING_ELT(labs, i));
            if raw.is_null() {
                continue;
            }
            out.push(std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned());
        }
        out
    }
}

/// GNU `drop.scope(terms1)` — `term.labels` that can be dropped.
pub unsafe fn do_drop_scope(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let t1 = do_terms(call, op, args, rho);
        let labels = term_label_strings(t1);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, labels.len() as i64);
        let _o = protect(out);
        for (i, name) in labels.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

/// GNU `add.scope(terms1, terms2)` — labels in 2 not in 1.
pub unsafe fn do_add_scope(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let t1 = do_terms(call, op, Rf_cons(CAR(args), R_NilValue()), rho);
        let t2 = do_terms(call, op, Rf_cons(CAR(CDR(args)), R_NilValue()), rho);
        let a = term_label_strings(t1);
        let b = term_label_strings(t2);
        let extra: Vec<String> = b
            .into_iter()
            .filter(|s| !a.iter().any(|t| t == s))
            .collect();
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, extra.len() as i64);
        let _o = protect(out);
        for (i, name) in extra.iter().enumerate() {
            let c = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}
