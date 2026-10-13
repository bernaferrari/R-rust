//! merge() and data-frame name/class helpers.

use super::*;

/// GNU `unstack` for a stacked values/ind data.frame.
pub unsafe fn do_unstack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'form' must be a two-sided formula",
            );
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let mut values = R_NilValue();
        let mut ind = R_NilValue();
        if TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(x) {
                let name = elt_to_string(names, i);
                if name == "values" {
                    values = VECTOR_ELT(x, i);
                } else if name == "ind" {
                    ind = VECTOR_ELT(x, i);
                }
            }
        }
        if values == R_NilValue() || ind == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'form' must be a two-sided formula",
            );
        }
        let n = XLENGTH(values);
        let levels =
            crate::sexp::attrib_core::getAttrib(ind, crate::sexp::attrib_core::R_LevelsSymbol());
        let nlev =
            if !levels.is_null() && levels != R_NilValue() && TYPEOF(levels) == SEXPTYPE::STRSXP {
                XLENGTH(levels)
            } else {
                0
            };
        if nlev == 0 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'form' must be a two-sided formula",
            );
        }
        let mut buckets: Vec<Vec<i64>> = vec![Vec::new(); nlev as usize];
        for i in 0..n {
            let code = if TYPEOF(ind) == SEXPTYPE::INTSXP {
                *INTEGER(ind).add(i as usize)
            } else {
                0
            };
            if code >= 1 && (code as i64) <= nlev {
                buckets[(code as usize) - 1].push(i);
            }
        }
        let nrow = buckets.first().map(|b| b.len() as i64).unwrap_or(0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, nlev);
        let _result = protect(result);
        let mut col_names: Vec<String> = Vec::new();
        for (k, bucket) in buckets.iter().enumerate() {
            let col = Rf_allocVector3(TYPEOF(values), bucket.len() as i64);
            for (dst, &src) in bucket.iter().enumerate() {
                copy_elt(values, src, col, dst as i64);
            }
            SET_VECTOR_ELT(result, k as i64, col);
            col_names.push(elt_to_string(levels, k as i64));
        }
        set_string_names(result, &col_names);
        set_compact_row_names(result, nrow);
        set_data_frame_class(result);
        result
    }
}

pub(super) fn merge_named_true(args: SEXP, name: &str) -> bool {
    unsafe {
        let cname = std::ffi::CString::new(name).unwrap_or_default();
        let sym = crate::sexp::symbol::Rf_installChar(
            cname.as_ptr(),
            name.len() as crate::sexp::ffi::R_xlen_t,
        );
        let mut cell = args;
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let mut v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::PROMSXP {
                v = crate::sexp::accessors::PRVALUE(v);
            }
            let tag = crate::sexp::accessors::TAG(cell);
            let is_true = (TYPEOF(v) == SEXPTYPE::LGLSXP || TYPEOF(v) == SEXPTYPE::INTSXP)
                && !v.is_null()
                && *INTEGER(v) == 1;
            if tag == sym {
                if is_true {
                    return true;
                }
            }
            cell = CDR(cell);
        }
        false
    }
}

pub(super) fn merge_named_value(args: SEXP, name: &str) -> SEXP {
    unsafe {
        let cname = std::ffi::CString::new(name).unwrap_or_default();
        let sym = crate::sexp::symbol::Rf_installChar(
            cname.as_ptr(),
            name.len() as crate::sexp::ffi::R_xlen_t,
        );
        let mut cell = args;
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let tag = crate::sexp::accessors::TAG(cell);
            if tag == sym {
                let mut v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::PROMSXP {
                    v = crate::sexp::accessors::PRVALUE(v);
                    if v == crate::sexp::globals::R_UnboundValue() {
                        v = crate::eval::eval::Rf_eval(
                            CAR(cell),
                            crate::sexp::accessors::PRENV(CAR(cell)),
                        );
                    }
                }
                return v;
            }
            cell = CDR(cell);
        }
        crate::sexp::globals::R_NilValue()
    }
}

pub(super) unsafe fn store_na(out: SEXP, i: i64) {
    unsafe {
        let t = TYPEOF(out);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(out).add(i as usize) = crate::sexp::ffi::NA_INTEGER;
        } else if t == SEXPTYPE::REALSXP {
            *REAL(out).add(i as usize) = crate::sexp::ffi::NA_REAL;
        } else if t == SEXPTYPE::STRSXP {
            SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
        }
    }
}

/// GNU `merge` inner join on intersecting column names.
pub unsafe fn do_merge(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        if TYPEOF(x) == SEXPTYPE::INTSXP && TYPEOF(y) == SEXPTYPE::INTSXP {
            let nx = XLENGTH(x) as usize;
            let ny = XLENGTH(y) as usize;
            if nx == 0 || ny == 0 {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "invalid 'xinds' argument",
                );
            }
            let all = CAR(CDR(CDR(args)));
            let ally = CAR(CDR(CDR(CDR(args))));
            let all_x = !all.is_null()
                && TYPEOF(all) == SEXPTYPE::LGLSXP
                && XLENGTH(all) > 0
                && *INTEGER(all) == TRUE;
            let all_y = !ally.is_null()
                && TYPEOF(ally) == SEXPTYPE::LGLSXP
                && XLENGTH(ally) > 0
                && *INTEGER(ally) == TRUE;
            let mut ix: Vec<(i32, i32)> = (0..nx)
                .map(|i| (*INTEGER(x).add(i), (i as i32) + 1))
                .collect();
            let mut iy: Vec<(i32, i32)> = (0..ny)
                .map(|i| (*INTEGER(y).add(i), (i as i32) + 1))
                .collect();
            ix.sort_by_key(|p| p.0);
            iy.sort_by_key(|p| p.0);
            let nx_lone = ix.iter().position(|p| p.0 > 0).unwrap_or(nx);
            let ny_lone = iy.iter().position(|p| p.0 > 0).unwrap_or(ny);
            let mut pairs: Vec<(i32, i32)> = Vec::new();
            let mut i = nx_lone;
            let mut j = ny_lone;
            while i < nx {
                let tmp = ix[i].0;
                let mut nnx = i;
                while nnx < nx && ix[nnx].0 == tmp {
                    nnx += 1;
                }
                while j < ny && iy[j].0 < tmp {
                    j += 1;
                }
                let j0 = j;
                while j < ny && iy[j].0 == tmp {
                    j += 1;
                }
                for a in i..nnx {
                    for b in j0..j {
                        pairs.push((ix[a].1, iy[b].1));
                    }
                }
                i = nnx;
            }
            let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
            let _a = protect(ans);
            let ansx = Rf_allocVector3(SEXPTYPE::INTSXP, pairs.len() as R_xlen_t);
            let ansy = Rf_allocVector3(SEXPTYPE::INTSXP, pairs.len() as R_xlen_t);
            for (k, (xv, yv)) in pairs.iter().enumerate() {
                *INTEGER(ansx).add(k) = *xv;
                *INTEGER(ansy).add(k) = *yv;
            }
            SET_VECTOR_ELT(ans, 0, ansx);
            SET_VECTOR_ELT(ans, 1, ansy);
            if all_x {
                let lone = Rf_allocVector3(SEXPTYPE::INTSXP, nx_lone as R_xlen_t);
                for k in 0..nx_lone {
                    *INTEGER(lone).add(k) = ix[k].1;
                }
                SET_VECTOR_ELT(ans, 2, lone);
            }
            if all_y {
                let lone = Rf_allocVector3(SEXPTYPE::INTSXP, ny_lone as R_xlen_t);
                for k in 0..ny_lone {
                    *INTEGER(lone).add(k) = iy[k].1;
                }
                SET_VECTOR_ELT(ans, 3, lone);
            }
            let nms = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
            for (k, name) in ["xi", "yi", "x.alone", "y.alone"].iter().enumerate() {
                let c = std::ffi::CString::new(*name).unwrap_or_default();
                crate::sexp::accessors::SET_STRING_ELT(
                    nms,
                    k as R_xlen_t,
                    crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                ans,
                crate::sexp::attrib_core::R_NamesSymbol(),
                nms,
            );
            return ans;
        }
        let x = CAR(args);
        let y = CAR(CDR(args));
        if TYPEOF(x) != SEXPTYPE::VECSXP || TYPEOF(y) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'by' must specify one or more columns as numbers, names or logical",
            );
        }
        let xnames =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let ynames =
            crate::sexp::attrib_core::getAttrib(y, crate::sexp::attrib_core::R_NamesSymbol());
        let mut by: Vec<String> = Vec::new();
        let mut x_by: Vec<usize> = Vec::new();
        let mut y_by: Vec<usize> = Vec::new();
        let by_x = merge_named_value(args, "by.x");
        let by_y = merge_named_value(args, "by.y");
        if TYPEOF(by_x) == SEXPTYPE::STRSXP
            && TYPEOF(by_y) == SEXPTYPE::STRSXP
            && XLENGTH(by_x) > 0
            && XLENGTH(by_x) == XLENGTH(by_y)
        {
            for k in 0..XLENGTH(by_x) {
                let xn = elt_to_string(by_x, k);
                let yn = elt_to_string(by_y, k);
                let xi = (0..XLENGTH(xnames)).find(|&i| elt_to_string(xnames, i) == xn);
                let yi = (0..XLENGTH(ynames)).find(|&i| elt_to_string(ynames, i) == yn);
                if let (Some(xi), Some(yi)) = (xi, yi) {
                    by.push(xn);
                    x_by.push(xi as usize);
                    y_by.push(yi as usize);
                }
            }
        } else if TYPEOF(xnames) == SEXPTYPE::STRSXP && TYPEOF(ynames) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(xnames) {
                let n = elt_to_string(xnames, i);
                for j in 0..XLENGTH(ynames) {
                    if n == elt_to_string(ynames, j) {
                        by.push(n.clone());
                        x_by.push(i as usize);
                        y_by.push(j as usize);
                    }
                }
            }
        }
        let nx = if XLENGTH(x) > 0 {
            XLENGTH(VECTOR_ELT(x, 0))
        } else {
            0
        };
        let ny = if XLENGTH(y) > 0 {
            XLENGTH(VECTOR_ELT(y, 0))
        } else {
            0
        };
        let mut pairs: Vec<(i64, i64)> = Vec::new();
        for i in 0..nx {
            for j in 0..ny {
                let mut ok = true;
                for k in 0..by.len() {
                    if !merge_keys_equal(
                        VECTOR_ELT(x, x_by[k] as i64),
                        i,
                        VECTOR_ELT(y, y_by[k] as i64),
                        j,
                    ) {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    pairs.push((i, j));
                }
            }
        }
        let all_flag = merge_named_true(args, "all");
        let all_x = all_flag || merge_named_true(args, "all.x");
        let all_y = all_flag || merge_named_true(args, "all.y");
        if all_x {
            for i in 0..nx {
                if !pairs.iter().any(|p| p.0 == i) {
                    pairs.push((i, -1));
                }
            }
        }
        if all_y {
            for j in 0..ny {
                if !pairs.iter().any(|p| p.1 == j) {
                    pairs.push((-1, j));
                }
            }
        }
        let sort_arg = merge_named_value(args, "sort");
        let do_sort = sort_arg.is_null()
            || sort_arg == R_NilValue()
            || sort_arg == crate::sexp::globals::R_MissingArg()
            || merge_named_true(args, "sort");
        if do_sort && !by.is_empty() {
            let xkey = VECTOR_ELT(x, x_by[0] as i64);
            let ykey = VECTOR_ELT(y, y_by[0] as i64);
            let key_of = |idx: i64, col: SEXP| -> String {
                if idx < 0 {
                    String::new()
                } else {
                    elt_to_string(col, idx)
                }
            };
            pairs.sort_by(|a, b| {
                let ka = if a.0 >= 0 {
                    key_of(a.0, xkey)
                } else {
                    key_of(a.1, ykey)
                };
                let kb = if b.0 >= 0 {
                    key_of(b.0, xkey)
                } else {
                    key_of(b.1, ykey)
                };
                ka.cmp(&kb)
            });
        }
        let nout = pairs.len() as i64;
        let x_extra: Vec<usize> = (0..XLENGTH(x) as usize)
            .filter(|i| !x_by.contains(i))
            .collect();
        let y_extra: Vec<usize> = (0..XLENGTH(y) as usize)
            .filter(|i| !y_by.contains(i))
            .collect();
        let ncols = by.len() + x_extra.len() + y_extra.len();
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as i64);
        let _result = protect(result);
        let mut names: Vec<String> = Vec::new();
        let mut col = 0i64;
        for (k, name) in by.iter().enumerate() {
            let src = VECTOR_ELT(x, x_by[k] as i64);
            let out = Rf_allocVector3(TYPEOF(src), nout);
            for (dst, (xi, yr)) in pairs.iter().enumerate() {
                if *xi < 0 {
                    let ysrc = VECTOR_ELT(y, y_by[k] as i64);
                    copy_elt(ysrc, *yr, out, dst as i64);
                } else {
                    copy_elt(src, *xi, out, dst as i64);
                }
            }
            SET_VECTOR_ELT(result, col, out);
            names.push(name.clone());
            col += 1;
        }
        for &xi in &x_extra {
            let src = VECTOR_ELT(x, xi as i64);
            let out = Rf_allocVector3(TYPEOF(src), nout);
            for (dst, (xr, _)) in pairs.iter().enumerate() {
                if *xr < 0 {
                    store_na(out, dst as i64);
                } else {
                    copy_elt(src, *xr, out, dst as i64);
                }
            }
            SET_VECTOR_ELT(result, col, out);
            names.push(format!("\u{0}x:{}", elt_to_string(xnames, xi as i64)));
            col += 1;
        }
        for &yi in &y_extra {
            let src = VECTOR_ELT(y, yi as i64);
            let out = Rf_allocVector3(TYPEOF(src), nout);
            for (dst, (_, yr)) in pairs.iter().enumerate() {
                if *yr < 0 {
                    store_na(out, dst as i64);
                } else {
                    copy_elt(src, *yr, out, dst as i64);
                }
            }
            SET_VECTOR_ELT(result, col, out);
            names.push(format!("\u{0}y:{}", elt_to_string(ynames, yi as i64)));
            col += 1;
        }
        let bare_of = |tag: &str| -> String {
            if let Some(rest) = tag.strip_prefix('\u{0}') {
                rest.split_once(':')
                    .map(|(_, b)| b.to_string())
                    .unwrap_or_else(|| tag.to_string())
            } else {
                tag.to_string()
            }
        };
        let raw: Vec<String> = names.iter().map(|n| bare_of(n)).collect();
        let mut fixed = Vec::with_capacity(names.len());
        for (i, tag) in names.iter().enumerate() {
            let bare = if let Some(rest) = tag.strip_prefix('\u{0}') {
                rest.split_once(':').map(|(_, b)| b).unwrap_or(tag)
            } else {
                tag.as_str()
            };
            let dup = raw.iter().filter(|r| r.as_str() == bare).count() > 1;
            if dup && tag.starts_with('\u{0}') {
                let side = if tag.as_bytes().get(1) == Some(&b'x') {
                    ".x"
                } else {
                    ".y"
                };
                fixed.push(format!("{bare}{side}"));
            } else {
                fixed.push(bare.to_string());
            }
        }
        names = fixed;
        set_string_names(result, &names);
        set_compact_row_names(result, nout);
        set_data_frame_class(result);
        result
    }
}

pub(super) unsafe fn merge_keys_equal(a: SEXP, i: i64, b: SEXP, j: i64) -> bool {
    unsafe {
        if TYPEOF(a) != TYPEOF(b) {
            return format_grid_elt(a, i) == format_grid_elt(b, j);
        }
        match TYPEOF(a) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(a).add(i as usize) == *INTEGER(b).add(j as usize)
            }
            t if t == SEXPTYPE::REALSXP => *REAL(a).add(i as usize) == *REAL(b).add(j as usize),
            t if t == SEXPTYPE::STRSXP => elt_to_string(a, i) == elt_to_string(b, j),
            _ => format_grid_elt(a, i) == format_grid_elt(b, j),
        }
    }
}

pub(crate) unsafe fn set_compact_row_names(x: SEXP, nrow: R_xlen_t) {
    unsafe {
        let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if rn.is_null() {
            return;
        }
        let _row_names_guard = protect(rn);
        *INTEGER(rn) = NA_INTEGER;
        *INTEGER(rn).add(1) = -(nrow as i32);
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_RowNamesSymbol(), rn);
    }
}

pub(crate) unsafe fn set_data_frame_class(x: SEXP) {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class_vec.is_null() {
            return;
        }
        let _class_guard = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, Rf_mkChar(c"data.frame".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), class_vec);
    }
}

pub(crate) unsafe fn set_summary_default_class(x: SEXP) {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if class_vec.is_null() {
            return;
        }
        let _class_guard = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, Rf_mkChar(c"summaryDefault".as_ptr()));
        SET_STRING_ELT(class_vec, 1, Rf_mkChar(c"table".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), class_vec);
    }
}

pub(super) fn repair_data_frame_names(names: &mut [String]) {
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for (i, name) in names.iter_mut().enumerate() {
        if name.is_empty() {
            *name = format!("X{}", i + 1);
        }
        let base = name.clone();
        let mut suffix = *used.get(&base).unwrap_or(&0);
        if suffix == 0 && !used.contains_key(&base) {
            used.insert(base, 1);
            continue;
        }
        loop {
            let candidate = format!("{base}.{suffix}");
            suffix += 1;
            if !used.contains_key(&candidate) {
                used.insert(base.clone(), suffix);
                used.insert(candidate.clone(), 1);
                *name = candidate;
                break;
            }
        }
    }
}
