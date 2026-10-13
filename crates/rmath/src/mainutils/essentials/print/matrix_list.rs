use super::*;

// ---------------------------------------------------------------------------
// Print / summary methods
// ---------------------------------------------------------------------------

/// R's `print.matrix(x)` — print a matrix with proper row/col formatting.
pub unsafe fn do_print_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        let (nrow, ncol) =
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr).add(1) as R_xlen_t,
                )
            } else {
                let n = XLENGTH(x).max(1);
                (n, 1)
            };

        // Get colnames
        let colnames = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
        let col_names_vec: Vec<String> =
            if !colnames.is_null() && TYPEOF(colnames) == SEXPTYPE::VECSXP && LENGTH(colnames) >= 2
            {
                let cn = VECTOR_ELT(colnames, 1);
                if !cn.is_null() && TYPEOF(cn) == SEXPTYPE::STRSXP {
                    let m = XLENGTH(cn).min(ncol);
                    (0..m).map(|i| elt_to_string(cn, i)).collect()
                } else {
                    (0..ncol).map(|i| format!("[,{}]", i + 1)).collect()
                }
            } else {
                (0..ncol).map(|i| format!("[,{}]", i + 1)).collect()
            };

        // Print column headers
        let mut header = String::from("     ");
        for name in &col_names_vec {
            let _ = std::fmt::Write::write_fmt(&mut header, format_args!("{:>12}", name));
        }
        println!("{}", header);

        // Print rows
        for r in 0..nrow {
            let row_label = format!("[{},]", r + 1);
            print!("{:>4} ", row_label);
            for c in 0..ncol {
                let idx = c * nrow + r;
                let s = elt_to_string(x, idx as R_xlen_t);
                print!("{:>12}", s);
            }
            println!();
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.list(x)` — print a list with element names.
pub unsafe fn do_print_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let n = XLENGTH(x);
        // Get names
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

        for i in 0..n {
            let name = if has_names && i < XLENGTH(names) {
                let s = elt_to_string(names, i);
                if s.is_empty() {
                    format!("${}", i + 1)
                } else {
                    format!("${}", s)
                }
            } else {
                format!("${}", i + 1)
            };
            let elem = VECTOR_ELT(x, i as i64);
            let type_str = if elem.is_null() {
                "NULL".to_string()
            } else {
                let t = TYPEOF(elem);
                match t {
                    t if t == SEXPTYPE::REALSXP => "num".to_string(),
                    t if t == SEXPTYPE::INTSXP => "int".to_string(),
                    t if t == SEXPTYPE::LGLSXP => "logi".to_string(),
                    t if t == SEXPTYPE::STRSXP => "chr".to_string(),
                    t if t == SEXPTYPE::VECSXP => "list".to_string(),
                    _ => "obj".to_string(),
                }
            };
            let preview = if elem.is_null() {
                "NULL".to_string()
            } else {
                let m = XLENGTH(elem).min(3);
                let parts: Vec<String> = (0..m).map(|j| elt_to_string(elem, j)).collect();
                format!("{}: {}", type_str, parts.join(" "))
            };
            println!("{}\n{}", name, preview);
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
