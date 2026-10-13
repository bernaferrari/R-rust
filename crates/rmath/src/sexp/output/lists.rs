//! output — extracted verbatim from output.rs.

use super::*;

pub(crate) fn printable_attribute_name(attr: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(attr);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let print_name = PRINTNAME(tag);
        if print_name.is_null() || print_name == R_NilValue() {
            return None;
        }
        let chars = CHAR(print_name);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

pub(crate) fn is_structural_print_attribute(name: &str) -> bool {
    matches!(name, "names" | "dim" | "dimnames" | "row.names")
}

pub(crate) fn is_hidden_noquote_class(name: &str, x: Sexp<'_>) -> bool {
    name == "class" && has_class(x, "noquote")
}

pub(crate) enum ClassForPrint {
    Keep,
    Omit,
    Filtered(SEXP, crate::sexp::protect::ProtectGuard<'static>),
}

pub(crate) fn class_for_print(class: SEXP) -> ClassForPrint {
    unsafe {
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return ClassForPrint::Keep;
        }
        let n = XLENGTH(class);
        let na = crate::sexp::globals::R_NaString();
        let mut keep = Vec::with_capacity(n as usize);
        let mut dropped = false;
        for i in 0..n {
            let elt = STRING_ELT(class, i);
            let is_asis = !elt.is_null() && elt != na && {
                let chars = CHAR(elt);
                !chars.is_null() && std::ffi::CStr::from_ptr(chars).to_bytes() == b"AsIs"
            };
            if is_asis {
                dropped = true;
            } else {
                keep.push(elt);
            }
        }
        if !dropped {
            return ClassForPrint::Keep;
        }
        // GNU print.AsIs drops every "AsIs" element and prints only what remains.
        if keep.is_empty() {
            return ClassForPrint::Omit;
        }
        let out =
            crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, keep.len() as R_xlen_t);
        let guard = crate::sexp::protect::protect(out);
        for (i, elt) in keep.into_iter().enumerate() {
            crate::sexp::accessors::SET_STRING_ELT(out, i as R_xlen_t, elt);
        }
        ClassForPrint::Filtered(out, guard)
    }
}

pub(crate) fn for_each_printable_attribute(
    x: Sexp<'_>,
    mut print: impl for<'attribute> FnMut(&str, Sexp<'attribute>),
) {
    // GNU retains the original chain through callbacks, but reads each next
    // edge after printing the current value. A callback can replace the whole
    // attribute chain, remove the next entry, or replace its value and collect.
    let original = x
        .try_attrib()
        .unwrap_or_else(|error| super::super::context::r_error(error.to_string()));
    let mut attrs = original.clone();
    unsafe {
        while !attrs.is_nil() {
            if let Some(name) = printable_attribute_name(attrs.as_raw())
                && !is_structural_print_attribute(&name)
                && !is_hidden_noquote_class(&name, x.clone())
            {
                let value = attrs
                    .try_car()
                    .unwrap_or_else(|error| super::super::context::r_error(error.to_string()));
                if !value.is_nil() {
                    if name == "class" {
                        match class_for_print(value.as_raw()) {
                            ClassForPrint::Omit => {}
                            ClassForPrint::Keep => print(&name, value),
                            ClassForPrint::Filtered(filtered, _guard) => {
                                let filtered = Sexp::from_raw(filtered)
                                    .expect("protected filtered class vector");
                                print(&name, filtered);
                            }
                        }
                    } else {
                        print(&name, value);
                    }
                }
            }
            attrs = attrs
                .try_cdr()
                .unwrap_or_else(|error| super::super::context::r_error(error.to_string()));
        }
    }
}

pub(crate) fn format_printable_attributes(x: Sexp<'_>) -> String {
    let mut out = String::new();
    for_each_printable_attribute(x, |name, value| {
        let header = format!("attr(,\"{name}\")");
        out.push_str(&format!("\n{header}\n"));
        out.push_str(&format_list_child(value, &header));
    });
    out
}

pub(crate) fn format_list_body_with_attributes(body: String, x: Sexp<'_>) -> String {
    let attrs = format_printable_attributes(x);
    if attrs.is_empty() {
        format!("{body}\n")
    } else {
        format!("{body}\n{attrs}")
    }
}

pub(crate) fn format_with_printable_attributes(base: String, x: Sexp<'_>) -> String {
    format!("{base}{}", format_printable_attributes(x))
}

pub(crate) fn print_with_printable_attributes(base: &str, x: Sexp<'_>) {
    emit(base);
    emit("\n");
    print_list_attributes(x);
}

pub(crate) fn list_names(x: Sexp<'_>) -> Vec<String> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        string_vector_values(names).unwrap_or_default()
    }
}

pub(crate) fn has_class(x: Sexp<'_>, class_name: &str) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        string_vector_contains(class, class_name)
    }
}

pub(crate) fn data_frame_nrows(x: Sexp<'_>) -> R_xlen_t {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
        );
        if let Some(row_names) = Sexp::from_raw(row_names)
            && row_names.clone().typeof_() == SEXPTYPE::INTSXP
            && row_names.clone().len() == 2
            && row_names.integer_elt(0) == Some(NA_INTEGER)
            && let Some(last) = row_names.integer_elt(1)
            && last < 0
        {
            return -(last as R_xlen_t);
        }
    }
    x.iter_vector().map(|col| col.len()).max().unwrap_or(0)
}

pub(crate) fn data_frame_row_labels(x: Sexp<'_>, nrow: R_xlen_t) -> Vec<String> {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
        );
        if let Some(row_names) = Sexp::from_raw(row_names)
            && row_names.clone().typeof_() == SEXPTYPE::STRSXP
            && row_names.clone().len() == nrow
        {
            return (0..nrow)
                .map(|i| match string_element_text(row_names.clone(), i) {
                    Some(Some(value)) => value.to_string(),
                    Some(None) | None => (i + 1).to_string(),
                })
                .collect();
        }
    }
    (1..=nrow).map(|i| i.to_string()).collect()
}

pub(crate) fn format_data_frame_cell(x: Sexp<'_>, row: R_xlen_t) -> String {
    if x.clone().len() == 0 {
        return "NA".to_string();
    }
    let i = row % x.clone().len();
    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => format_integer_element(x, i),
        SEXPTYPE::REALSXP => format_real_element(x, i),
        SEXPTYPE::LGLSXP => format_logical_element(x, i),
        SEXPTYPE::STRSXP => match string_element_text(x, i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "NA".to_string(),
        },
        _ => format_sexp_direct(x),
    }
}

pub(crate) fn format_data_frame_column(col: Sexp<'_>, nrow: R_xlen_t) -> Vec<String> {
    unsafe {
        if matches!(
            col.clone().typeof_(),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::CPLXSXP
        ) {
            let args = crate::sexp::constructors::Rf_cons(col.clone().as_raw(), R_NilValue());

            let _g = crate::sexp::protect::protect(args);
            let formatted = crate::mainutils::essentials::do_format(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                crate::sexp::globals::R_GlobalEnv(),
            );
            if let Some(formatted) = Sexp::from_raw(formatted)
                && formatted.clone().typeof_() == SEXPTYPE::STRSXP
            {
                return (0..nrow)
                    .map(|i| match string_element_text(formatted.clone(), i) {
                        Some(Some(value)) => value.to_string(),
                        Some(None) | None => "NA".to_string(),
                    })
                    .collect();
            }
        }
    }
    (0..nrow)
        .map(|row| format_data_frame_cell(col.clone(), row))
        .collect()
}

pub(crate) fn format_data_frame(x: Sexp<'_>) -> Option<String> {
    if !has_class(x.clone(), "data.frame") {
        return None;
    }
    let names = list_names(x.clone());
    let nrow = data_frame_nrows(x.clone());
    let row_labels = data_frame_row_labels(x.clone(), nrow);
    let columns: Vec<Sexp<'_>> = x.iter_vector().collect();
    let row_width = row_labels.iter().map(String::len).max().unwrap_or(1).max(1);
    let formatted_cols: Vec<Vec<String>> = columns
        .iter()
        .map(|col| format_data_frame_column(col.clone(), nrow))
        .collect();
    let widths: Vec<usize> = formatted_cols
        .iter()
        .enumerate()
        .map(|(i, cells)| {
            let name_width = names.get(i).map(String::len).unwrap_or(0);
            let value_width = cells.iter().map(String::len).max().unwrap_or(0);
            name_width.max(value_width)
        })
        .collect();
    let header = format!(
        "{} {}",
        " ".repeat(row_width),
        names
            .iter()
            .zip(&widths)
            .map(|(name, width)| format!("{name:>width$}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut lines = Vec::with_capacity(nrow as usize + 1);
    lines.push(header);
    for row in 0..nrow {
        let row_name = format!(
            "{:>row_width$}",
            row_labels
                .get(row as usize)
                .cloned()
                .unwrap_or_else(|| (row + 1).to_string())
        );
        let cells = formatted_cols
            .iter()
            .zip(&widths)
            .map(|(col, width)| {
                format!(
                    "{:>width$}",
                    col.get(row as usize)
                        .cloned()
                        .unwrap_or_else(|| "NA".to_string())
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("{row_name} {cells}"));
    }
    Some(lines.join("\n"))
}

pub(crate) fn list_name_tag(name: &str) -> String {
    if crate::mainutils::deparse::is_valid_r_name_bytes(name.as_bytes()) {
        format!("${name}")
    } else {
        format!("$`{name}`")
    }
}

pub(crate) fn list_element_header(index: usize, names: &[String]) -> String {
    match names.get(index) {
        Some(name) if !name.is_empty() => list_name_tag(name),
        _ => format!("[[{}]]", index + 1),
    }
}

pub(crate) fn format_list(x: Sexp<'_>) -> String {
    format_list_with_path(x, "")
}

pub(crate) fn format_list_with_path(x: Sexp<'_>, path: &str) -> String {
    if x.clone().len() == 0 {
        return format_with_printable_attributes(empty_list_body(x.clone()).to_string(), x);
    }
    let names = list_names(x.clone());
    let mut sections = Vec::with_capacity(x.clone().len() as usize);
    for (index, elem) in x.clone().iter_vector().enumerate() {
        let header = format!("{path}{}", list_element_header(index, &names));
        let body = format_list_child(elem, &header);
        sections.push(format!("{header}\n{body}"));
    }
    format_list_body_with_attributes(sections.join("\n\n"), x)
}

pub(crate) fn empty_list_body(x: Sexp<'_>) -> &'static str {
    if has_names_attribute(x) {
        "named list()"
    } else {
        "list()"
    }
}

pub(crate) fn format_pairlist(x: Sexp<'_>) -> String {
    format_pairlist_with_path(x, "")
}

pub(crate) fn format_pairlist_with_path(x: Sexp<'_>, path: &str) -> String {
    unsafe {
        let mut sections = Vec::new();
        let mut cell = x.clone().as_raw();
        let mut index = 0usize;
        while !cell.is_null() && cell != R_NilValue() && TYPEOF(cell) == SEXPTYPE::LISTSXP {
            let tag = match printable_attribute_name(cell) {
                Some(name) if !name.is_empty() => list_name_tag(&name),
                _ => format!("[[{}]]", index + 1),
            };
            let header = format!("{path}{tag}");
            let body = if let Some(elem) = Sexp::from_raw(CAR(cell)) {
                format_list_child(elem, &header)
            } else {
                "NULL".to_string()
            };
            sections.push(format!("{header}\n{body}"));
            cell = CDR(cell);
            index += 1;
        }

        if sections.is_empty() {
            return format_with_printable_attributes("NULL".to_string(), x);
        }
        format_list_body_with_attributes(sections.join("\n\n"), x)
    }
}

pub(crate) fn format_list_child(elem: Sexp<'_>, path: &str) -> String {
    if let Some(dispatched) = format_dispatched_print(elem.clone()) {
        return dispatched;
    }
    match elem.clone().typeof_() {
        SEXPTYPE::VECSXP if format_data_frame(elem.clone()).is_none() => {
            format_list_with_path(elem, path)
        }
        SEXPTYPE::LISTSXP => format_pairlist_with_path(elem, path),
        _ => format_sexp_direct(elem),
    }
}

// Public printing emits headers and completed children before calling the
// next method. A caught error must not discard earlier output or move a
// method's stderr/message traffic out of its original capture/sink order.
pub(crate) fn print_list_child(elem: Sexp<'_>, path: &str) {
    if unsafe { crate::mainutils::coerce::IS_S4_OBJECT(elem.as_raw()) } != 0 {
        print_value(elem);
    } else if dispatch_print_method(elem.clone()).is_some() {
        // The method wrote directly into the caller's capture.
    } else {
        match elem.typeof_() {
            SEXPTYPE::VECSXP
                if !has_class(elem.clone(), "data.frame")
                    && !has_class(elem.clone(), "POSIXlt")
                    && !has_class(elem.clone(), "summary.warnings") =>
            {
                print_list_with_path(elem, path)
            }
            SEXPTYPE::LISTSXP => print_pairlist_with_path(elem, path),
            _ => print_value(elem),
        }
    }
}

pub(crate) fn print_list_attributes(x: Sexp<'_>) {
    for_each_printable_attribute(x, |name, value| {
        let header = format!("attr(,\"{name}\")");
        emit(&format!("{header}\n"));
        print_list_child(value, &header);
    });
}

pub(crate) fn print_list_with_path(x: Sexp<'_>, path: &str) {
    if x.len() == 0 {
        print_with_printable_attributes(empty_list_body(x.clone()), x);
        return;
    }
    let names = list_names(x.clone());
    for (index, elem) in x.clone().iter_vector().enumerate() {
        let header = format!("{path}{}", list_element_header(index, &names));
        emit(&format!("{header}\n"));
        print_list_child(elem, &header);
        emit("\n");
    }
    print_list_attributes(x);
}

pub(crate) fn print_pairlist_with_path(x: Sexp<'_>, path: &str) {
    for (index, cell) in crate::sexp::object::PairlistIter::new(x.clone()).enumerate() {
        let tag = unsafe { printable_attribute_name(cell.as_raw()) };
        let tag = match tag {
            Some(name) if !name.is_empty() => list_name_tag(&name),
            _ => format!("[[{}]]", index + 1),
        };
        let header = format!("{path}{tag}");
        emit(&format!("{header}\n"));
        if let Some(elem) = cell.car() {
            print_list_child(elem, &header);
        } else {
            emit("NULL\n");
        }
        emit("\n");
    }
    print_list_attributes(x);
}
