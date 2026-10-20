//! output — extracted verbatim from output.rs.

use super::*;

pub(crate) fn array_dims(x: Sexp<'_>) -> Option<Vec<usize>> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim = Sexp::from_raw(dim)?;
        if dim.clone().typeof_() != SEXPTYPE::INTSXP || dim.clone().len() < 3 {
            return None;
        }
        let mut dims = Vec::with_capacity(dim.clone().len() as usize);
        let mut product = 1usize;
        for i in 0..dim.clone().len() {
            let n = dim.clone().integer_elt(i)? as usize;
            product = product.checked_mul(n)?;
            dims.push(n);
        }
        if product > x.len() as usize {
            return None;
        }
        Some(dims)
    }
}

pub(crate) fn ceil_div(a: usize, b: usize) -> usize {
    if b == 0 { 0 } else { a.div_ceil(b) }
}

pub(crate) fn sexp_string_at(x: SEXP, i: R_xlen_t) -> Option<String> {
    unsafe {
        if x.is_null()
            || x == R_NilValue()
            || TYPEOF(x) != SEXPTYPE::STRSXP
            || i < 0
            || i >= XLENGTH(x)
        {
            return None;
        }
        let elt = STRING_ELT(x, i);
        if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
            return Some("NA".to_string());
        }
        let chars = CHAR(elt);
        if chars.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// GNU `printArray` banner: `, , <dimname>` or `, , <axis> = <dimname>`.
pub(crate) fn array_slice_banner(x: Sexp<'_>, dims: &[usize], slice: usize) -> String {
    unsafe {
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        let dnn = if !dimnames.is_null()
            && dimnames != R_NilValue()
            && TYPEOF(dimnames) == SEXPTYPE::VECSXP
        {
            crate::sexp::attrib_core::getAttrib(dimnames, crate::sexp::attrib_core::R_NamesSymbol())
        } else {
            R_NilValue()
        };
        let has_dnn = !dnn.is_null() && dnn != R_NilValue() && TYPEOF(dnn) == SEXPTYPE::STRSXP;
        let mut header = String::from(", ");
        let mut k = 1usize;
        for (j, &extent) in dims.iter().enumerate().skip(2) {
            let l = if extent == 0 {
                1
            } else {
                (slice / k) % extent + 1
            };
            let dn = if !dimnames.is_null()
                && dimnames != R_NilValue()
                && TYPEOF(dimnames) == SEXPTYPE::VECSXP
                && (j as R_xlen_t) < XLENGTH(dimnames)
            {
                VECTOR_ELT(dimnames, j as R_xlen_t)
            } else {
                R_NilValue()
            };
            if let Some(name) = sexp_string_at(dn, (l as R_xlen_t) - 1) {
                if has_dnn {
                    let axis = sexp_string_at(dnn, j as R_xlen_t).unwrap_or_default();
                    header.push_str(&format!(", {axis} = {name}"));
                } else {
                    header.push_str(&format!(", {name}"));
                }
            } else {
                header.push_str(&format!(", {l}"));
            }
            k = k.saturating_mul(extent.max(1));
        }
        header
    }
}

pub(crate) fn format_array_slice_body(
    x: Sexp<'_>,
    nr: usize,
    use_nr: usize,
    use_nc: usize,
    offset: usize,
) -> String {
    let index_at = move |r: usize, c: usize| (offset + r + c * nr) as i64;
    if x.clone().typeof_() == SEXPTYPE::STRSXP {
        let quote = print_quote_flag()
            && !has_class(x.clone(), "noquote")
            && !has_class(x.clone(), "table");
        return format_character_matrix_with(x.clone(), use_nr, use_nc, |r, c| {
            printed_string_element(x.clone(), index_at(r, c), quote)
        });
    }
    format_matrix_with(x.clone(), use_nr, use_nc, |r, c| {
        let index = index_at(r, c);
        match x.clone().typeof_() {
            SEXPTYPE::LGLSXP => format_logical_element(x.clone(), index),
            SEXPTYPE::INTSXP => format_integer_element(x.clone(), index),
            SEXPTYPE::REALSXP => format_real_element(x.clone(), index),
            SEXPTYPE::CPLXSXP => format_complex_element(x.clone(), index),
            SEXPTYPE::RAWSXP => format_raw_element(x.clone(), index),
            _ => "NA".to_string(),
        }
    })
}

pub(crate) fn format_array(x: Sexp<'_>) -> Option<String> {
    let dims = array_dims(x.clone())?;
    let nr = dims[0];
    let nc = dims[1];
    let b = nr.saturating_mul(nc);
    let mut nb = 1usize;
    for &d in &dims[2..] {
        nb = nb.saturating_mul(d);
    }
    let max = print_max_cells();
    let max_reached = b > 0 && nb > 0 && max / b < nb;
    let (nb_pr, nc_last, nr_last) = if max_reached {
        let mut nb_pr = ceil_div(max, b);
        let ne_last = max.saturating_sub(b.saturating_mul(nb_pr.saturating_sub(1)));
        let mut nc_last = ne_last.min(nc);
        let mut nr_last = if ne_last < nc { 1 } else { ne_last / nc };
        if nr_last == 0 {
            nb_pr = nb_pr.saturating_sub(1);
            nc_last = nc;
            nr_last = nr;
        }
        (nb_pr.max(1), nc_last, nr_last)
    } else {
        (nb.max(1), nc, nr)
    };

    let mut sections = Vec::new();
    for ii in 0..nb_pr {
        let i_last = ii + 1 == nb_pr;
        let use_nc = if i_last { nc_last } else { nc };
        let use_nr = if i_last { nr_last } else { nr };
        let header = array_slice_banner(x.clone(), &dims, ii);
        let offset = ii.saturating_mul(b);
        let body = format_array_slice_body(x.clone(), nr, use_nr, use_nc, offset);
        // Caller appends one newline. Ending the body on a single newline
        // leaves the blank line GNU prints after the last slice, not two.
        sections.push(format!("{header}\n\n{body}"));
    }
    let mut text = sections.join("\n\n");
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if max_reached {
        let mut msg = String::from(" [ reached 'max' / getOption(\"max.print\") -- omitted");
        if nb_pr < nb {
            let omitted = nb - nb_pr;
            msg.push_str(&format!(
                " {} slice{}",
                omitted,
                if omitted == 1 { "" } else { "s" }
            ));
        } else if nb_pr == nb {
            let nr_rem = nr.saturating_sub(nr_last);
            if nr_rem > 0 {
                msg.push_str(&format!(
                    " {} row{}",
                    nr_rem,
                    if nr_rem == 1 { "" } else { "s" }
                ));
            }
            let nc_rem = nc.saturating_sub(nc_last);
            if nc_rem > 0 {
                msg.push_str(&format!(
                    " {} column{}",
                    nc_rem,
                    if nc_rem == 1 { "" } else { "s" }
                ));
            }
        }
        msg.push_str(" ] ");
        text.push('\n');
        text.push_str(&msg);
    }
    Some(text)
}
