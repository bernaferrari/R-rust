//! hclust, dendrogram, heatmap and clustering helpers — extracted verbatim from essentials/mathstats.rs.

use super::*;

fn dist_compact(d: &[f64], i: usize, j: usize, n: usize) -> f64 {
    if i == j {
        return 0.0;
    }
    let (a, b) = if i < j { (i, j) } else { (j, i) };
    let mut idx = 0;
    for k in 0..a {
        idx += n - 1 - k;
    }
    d[idx + (b - a - 1)]
}

fn complete_link(d: &[f64], a: &[usize], b: &[usize], n: usize) -> f64 {
    let mut m = f64::NEG_INFINITY;
    for &i in a {
        for &j in b {
            m = m.max(dist_compact(d, i, j, n));
        }
    }
    m
}

/// GNU `hclust(d, method="complete")`.
pub unsafe fn do_hclust(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _ = _rho;
        // GNU stats/R/hclust.R: method order is the Fortran iOpt code.
        const METHODS: [&str; 8] = [
            "ward.D", "single", "complete", "average", "mcquitty", "median", "centroid", "ward.D2",
        ];
        let d = CAR(args);
        if d.is_null() || d == R_NilValue() {
            crate::mainutils::errors::errorcall_str(call, "invalid dissimilarities");
        }
        let mut method = "complete".to_string();
        let mut members_arg = R_NilValue();
        let mut cell = CDR(args);
        let mut pos = 0i32;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            if name == "method" || (name.is_empty() && pos == 0) {
                if TYPEOF(val) == SEXPTYPE::STRSXP && XLENGTH(val) > 0 {
                    let ch = STRING_ELT(val, 0);
                    if !ch.is_null() {
                        method = std::ffi::CStr::from_ptr(CHAR(ch))
                            .to_string_lossy()
                            .into_owned();
                    }
                }
            } else if name == "members" || (name.is_empty() && pos == 1) {
                members_arg = val;
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        if method == "ward" {
            method = "ward.D".to_string();
        }
        let i_meth = METHODS.iter().position(|m| m.starts_with(&method));
        let ambiguous = METHODS.iter().filter(|m| m.starts_with(&method)).count() != 1;
        let Some(i_meth) = i_meth.filter(|_| !ambiguous && !method.is_empty()) else {
            crate::mainutils::errors::errorcall_str(call, "invalid clustering method");
        };
        let iopt = (i_meth as i32) + 1;

        let size_attr = crate::sexp::attrib_core::getAttrib(
            d,
            crate::sexp::symbol::Rf_install(c"Size".as_ptr()),
        );
        let n = if !size_attr.is_null()
            && size_attr != R_NilValue()
            && TYPEOF(size_attr) == SEXPTYPE::INTSXP
            && XLENGTH(size_attr) > 0
        {
            *INTEGER(size_attr) as i32
        } else {
            let len = XLENGTH(d) as usize;
            (((1.0 + (1.0 + 8.0 * len as f64).sqrt()) / 2.0).round()) as i32
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(call, "must have n >= 2 objects to cluster");
        }
        if n > 65536 {
            crate::mainutils::errors::errorcall_str(call, "size cannot be NA nor exceed 65536");
        }
        let len = n as usize * (n as usize - 1) / 2;
        if XLENGTH(d) as usize != len {
            crate::mainutils::errors::errorcall_str(call, "dissimilarities of improper length");
        }
        let mut diss = Vec::with_capacity(len);
        for i in 0..len as i64 {
            diss.push(elt_real_safe(d, i));
        }
        let nu = n as usize;
        let mut ia = vec![0i32; nu];
        let mut ib = vec![0i32; nu];
        let mut crit = vec![0f64; nu];
        let mut membr = vec![1f64; nu];
        if !members_arg.is_null() && members_arg != R_NilValue() && members_arg != R_MissingArg() {
            if XLENGTH(members_arg) as i32 != n {
                crate::mainutils::errors::errorcall_str(call, "invalid length of members");
            }
            for i in 0..nu {
                membr[i] = elt_real_safe(members_arg, i as i64);
            }
        }
        let mut nn = vec![0i32; nu];
        let mut disnn = vec![0f64; nu];
        let mut n_slot = n;
        let mut len_slot = len as i32;
        let mut iopt_slot = iopt;
        crate::library::stats::hclust_f::c_hclust(
            &mut n_slot as *mut i32 as *mut std::os::raw::c_void,
            &mut len_slot as *mut i32 as *mut std::os::raw::c_void,
            &mut iopt_slot as *mut i32 as *mut std::os::raw::c_void,
            ia.as_mut_ptr() as *mut std::os::raw::c_void,
            ib.as_mut_ptr() as *mut std::os::raw::c_void,
            crit.as_mut_ptr() as *mut std::os::raw::c_void,
            membr.as_mut_ptr() as *mut std::os::raw::c_void,
            nn.as_mut_ptr() as *mut std::os::raw::c_void,
            disnn.as_mut_ptr() as *mut std::os::raw::c_void,
            diss.as_mut_ptr() as *mut std::os::raw::c_void,
        );
        let mut iorder = vec![0i32; nu];
        let mut iia = vec![0i32; nu];
        let mut iib = vec![0i32; nu];
        crate::library::stats::hclust_f::c_hcass2(
            &mut n_slot as *mut i32 as *mut std::os::raw::c_void,
            ia.as_mut_ptr() as *mut std::os::raw::c_void,
            ib.as_mut_ptr() as *mut std::os::raw::c_void,
            iorder.as_mut_ptr() as *mut std::os::raw::c_void,
            iia.as_mut_ptr() as *mut std::os::raw::c_void,
            iib.as_mut_ptr() as *mut std::os::raw::c_void,
        );

        let nmerge = n - 1;
        let merge = crate::mainutils::array::allocMatrix(SEXPTYPE::INTSXP.as_c_int(), nmerge, 2);
        let _m = protect(merge);
        for s in 0..nmerge as usize {
            *INTEGER(merge).add(s) = iia[s];
            *INTEGER(merge).add(s + nmerge as usize) = iib[s];
        }
        let height = Rf_allocVector3(SEXPTYPE::REALSXP, nmerge as i64);
        let _h = protect(height);
        for s in 0..nmerge as usize {
            *REAL(height).add(s) = crit[s];
        }
        let order = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
        let _o = protect(order);
        for s in 0..nu {
            *INTEGER(order).add(s) = iorder[s];
        }
        let labels = crate::sexp::attrib_core::getAttrib(
            d,
            crate::sexp::symbol::Rf_install(c"Labels".as_ptr()),
        );
        let method_s = Rf_mkString(
            std::ffi::CString::new(METHODS[i_meth])
                .unwrap_or_default()
                .as_ptr(),
        );
        let _ms = protect(method_s);
        let dist_method = crate::sexp::attrib_core::getAttrib(
            d,
            crate::sexp::symbol::Rf_install(c"method".as_ptr()),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 7);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, merge);
        SET_VECTOR_ELT(result, 1, height);
        SET_VECTOR_ELT(result, 2, order);
        SET_VECTOR_ELT(
            result,
            3,
            if labels.is_null() {
                R_NilValue()
            } else {
                labels
            },
        );
        SET_VECTOR_ELT(result, 4, method_s);
        SET_VECTOR_ELT(result, 5, if call.is_null() { R_NilValue() } else { call });
        SET_VECTOR_ELT(
            result,
            6,
            if dist_method.is_null() {
                R_NilValue()
            } else {
                dist_method
            },
        );
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "merge".to_string(),
                "height".to_string(),
                "order".to_string(),
                "labels".to_string(),
                "method".to_string(),
                "call".to_string(),
                "dist.method".to_string(),
            ],
        );
        let class = Rf_mkString(c"hclust".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

pub(super) fn dist_compact_set(d: &mut [f64], i: usize, j: usize, n: usize, val: f64) {
    if i == j {
        return;
    }
    let (a, b) = if i < j { (i, j) } else { (j, i) };
    let mut idx = 0;
    for k in 0..a {
        idx += n - 1 - k;
    }
    d[idx + (b - a - 1)] = val;
}

/// GNU `mauchly.test(SSD)` — default T=I, Sigma=I sphericity.
pub unsafe fn do_mauchly_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let ssd = list_named_elt(obj, "SSD");
        let dfr = list_named_elt(obj, "df");
        if ssd == R_NilValue() || dfr == R_NilValue() {
            return R_NilValue();
        }
        let dim = crate::sexp::attrib_core::getAttrib(ssd, crate::sexp::attrib_core::R_DimSymbol());
        let p = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 2 {
            *INTEGER(dim).add(1) as usize
        } else {
            let n = XLENGTH(ssd) as usize;
            (n as f64).sqrt().round() as usize
        };
        if p == 0 {
            return R_NilValue();
        }
        let mut u = vec![0.0; p * p];
        for j in 0..p {
            for i in 0..p {
                u[i + j * p] = elt_real_safe(ssd, (i + j * p) as i64);
            }
        }
        let mut tr = 0.0;
        for i in 0..p {
            tr += u[i + i * p];
        }
        let mut a = u.clone();
        let mut det = 1.0;
        for k in 0..p {
            let mut piv = a[k + k * p];
            if piv.abs() < 1e-15 {
                let mut sw = None;
                for i in (k + 1)..p {
                    if a[i + k * p].abs() > 1e-15 {
                        sw = Some(i);
                        break;
                    }
                }
                if let Some(i) = sw {
                    for j in 0..p {
                        a.swap(k + j * p, i + j * p);
                    }
                    det = -det;
                    piv = a[k + k * p];
                } else {
                    det = 0.0;
                    break;
                }
            }
            det *= piv;
            for i in (k + 1)..p {
                let f = a[i + k * p] / piv;
                for j in k..p {
                    a[i + j * p] -= f * a[k + j * p];
                }
            }
        }
        let pf = p as f64;
        let n = elt_real_safe(dfr, 0);
        let logw = if det > 0.0 && tr > 0.0 {
            det.ln() - pf * (tr / pf).ln()
        } else {
            f64::NEG_INFINITY
        };
        let w = logw.exp();
        let rho = 1.0 - (2.0 * pf * pf + pf + 2.0) / (6.0 * pf * n);
        let w2 = (pf + 2.0)
            * (pf - 1.0)
            * (pf - 2.0)
            * (2.0 * pf * pf * pf + 6.0 * pf * pf + 3.0 * pf + 2.0)
            / (288.0 * (n * pf * rho).powi(2));
        let z = -n * rho * logw;
        let fdf = pf * (pf + 1.0) / 2.0 - 1.0;
        let pr1 = crate::dist::chisq::pchisq_inner(z, fdf, false, false);
        let pr2 = crate::dist::chisq::pchisq_inner(z, fdf + 4.0, false, false);
        let pval = pr1 + w2 * (pr2 - pr1);
        let stat = Rf_ScalarReal(w);
        let _s = protect(stat);
        crate::mainutils::essentials::set_string_names(stat, &["W".to_string()]);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, stat);
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(pval));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        result
    }
}

/// GNU `as.hclust(x)` — identity for `hclust` objects.
pub unsafe fn do_as_hclust(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

pub(super) fn class_contains(x: SEXP, name: &str) -> bool {
    unsafe {
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(class) {
            let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i))).to_string_lossy();
            if s == name {
                return true;
            }
        }
        false
    }
}

/// GNU `as.dendrogram(hclust)` — leaf order with class `dendrogram`.
pub unsafe fn do_as_dendrogram(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        if class_contains(obj, "dendrogram") {
            return obj;
        }
        let merge = list_named_elt(obj, "merge");
        let height = list_named_elt(obj, "height");
        let labels = list_named_elt(obj, "labels");
        if merge == R_NilValue() || height == R_NilValue() {
            crate::mainutils::errors::errorcall_str(call, "invalid dendrogram");
        }
        let dim =
            crate::sexp::attrib_core::getAttrib(merge, crate::sexp::attrib_core::R_DimSymbol());
        let n_merge = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 1 {
            *INTEGER(dim) as usize
        } else {
            0
        };
        if n_merge == 0 || XLENGTH(height) as usize != n_merge {
            crate::mainutils::errors::errorcall_str(call, "invalid dendrogram");
        }
        let hang = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            -1.0
        } else {
            elt_real_safe(CADR(args), 0)
        };
        let hmax = elt_real_safe(height, (n_merge as i64) - 1);
        let z = Rf_allocVector3(SEXPTYPE::VECSXP, n_merge as i64);
        let _z = protect(z);

        let member_of = |node: SEXP| -> i32 {
            let m = crate::sexp::attrib_core::getAttrib(
                node,
                crate::sexp::symbol::Rf_install(c"members".as_ptr()),
            );
            if m.is_null() || m == R_NilValue() || XLENGTH(m) < 1 {
                1
            } else {
                elt_real_safe(m, 0) as i32
            }
        };
        let mid_of = |node: SEXP| -> f64 {
            let m = crate::sexp::attrib_core::getAttrib(
                node,
                crate::sexp::symbol::Rf_install(c"midpoint".as_ptr()),
            );
            if m.is_null() || m == R_NilValue() || XLENGTH(m) < 1 {
                0.0
            } else {
                elt_real_safe(m, 0)
            }
        };
        let make_leaf = |id: i32, h0: f64| -> SEXP {
            let leaf = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            *INTEGER(leaf) = id;
            let _leaf = protect(leaf);
            let members_v = Rf_ScalarInteger(1);
            let height_v = Rf_ScalarReal(h0);
            let isleaf_v = Rf_ScalarLogical(crate::sexp::ffi::TRUE);
            let _g1 = protect(members_v);
            let _g2 = protect(height_v);
            let _g3 = protect(isleaf_v);
            crate::sexp::attrib_core::setAttrib(
                leaf,
                crate::sexp::symbol::Rf_install(c"members".as_ptr()),
                members_v,
            );
            crate::sexp::attrib_core::setAttrib(
                leaf,
                crate::sexp::symbol::Rf_install(c"height".as_ptr()),
                height_v,
            );
            crate::sexp::attrib_core::setAttrib(
                leaf,
                crate::sexp::symbol::Rf_install(c"leaf".as_ptr()),
                isleaf_v,
            );

            if !labels.is_null()
                && labels != R_NilValue()
                && TYPEOF(labels) == SEXPTYPE::STRSXP
                && id >= 1
                && (id as i64) <= XLENGTH(labels)
            {
                let s = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                SET_STRING_ELT(s, 0, STRING_ELT(labels, (id as i64) - 1));
                let _s = protect(s);
                crate::sexp::attrib_core::setAttrib(
                    leaf,
                    crate::sexp::symbol::Rf_install(c"label".as_ptr()),
                    s,
                );
            }
            leaf
        };
        let set_node = |node: SEXP, members: i32, midpoint: f64, h: f64| {
            let m_v = Rf_ScalarInteger(members);
            let mp_v = Rf_ScalarReal(midpoint);
            let hh_v = Rf_ScalarReal(h);
            let _g1 = protect(m_v);
            let _g2 = protect(mp_v);
            let _g3 = protect(hh_v);
            crate::sexp::attrib_core::setAttrib(
                node,
                crate::sexp::symbol::Rf_install(c"members".as_ptr()),
                m_v,
            );
            crate::sexp::attrib_core::setAttrib(
                node,
                crate::sexp::symbol::Rf_install(c"midpoint".as_ptr()),
                mp_v,
            );
            crate::sexp::attrib_core::setAttrib(
                node,
                crate::sexp::symbol::Rf_install(c"height".as_ptr()),
                hh_v,
            );
        };

        for k in 0..n_merge {
            let x0 = *INTEGER(merge).add(k);
            let x1 = *INTEGER(merge).add(k + n_merge);
            let hk = elt_real_safe(height, k as i64);
            let h0 = if hang < 0.0 {
                0.0
            } else {
                (hk - hang * hmax).max(0.0)
            };
            let node = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _node = protect(node);
            if x0 < 0 && x1 < 0 {
                SET_VECTOR_ELT(node, 0, make_leaf(-x0, h0));
                SET_VECTOR_ELT(node, 1, make_leaf(-x1, h0));
                set_node(node, 2, 0.5, hk);
            } else if x0 < 0 || x1 < 0 {
                let (leaf_id, internal) = if x0 < 0 { (-x0, x1) } else { (-x1, x0) };
                let internal_node = VECTOR_ELT(z, (internal as i64) - 1);
                let leaf = make_leaf(leaf_id, h0);
                if x0 < 0 {
                    SET_VECTOR_ELT(node, 0, leaf);
                    SET_VECTOR_ELT(node, 1, internal_node);
                } else {
                    SET_VECTOR_ELT(node, 0, internal_node);
                    SET_VECTOR_ELT(node, 1, leaf);
                }
                let left = VECTOR_ELT(node, 0);
                let members = member_of(internal_node) + 1;
                let midpoint = (member_of(left) as f64 + mid_of(internal_node)) / 2.0;
                set_node(node, members, midpoint, hk);
            } else {
                let left = VECTOR_ELT(z, (x0 as i64) - 1);
                let right = VECTOR_ELT(z, (x1 as i64) - 1);
                SET_VECTOR_ELT(node, 0, left);
                SET_VECTOR_ELT(node, 1, right);
                let ml = member_of(left);
                let midpoint = (ml as f64 + mid_of(left) + mid_of(right)) / 2.0;
                set_node(node, ml + member_of(right), midpoint, hk);
            }
            SET_VECTOR_ELT(z, k as i64, node);
        }
        let result = VECTOR_ELT(z, (n_merge as i64) - 1);
        let class = Rf_mkString(c"dendrogram".as_ptr());
        let _c = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

unsafe fn dendrogram_flag(node: SEXP, name: &std::ffi::CStr) -> bool {
    unsafe {
        let attr = crate::sexp::attrib_core::getAttrib(
            node,
            crate::sexp::symbol::Rf_install(name.as_ptr()),
        );
        !attr.is_null()
            && attr != R_NilValue()
            && TYPEOF(attr) == SEXPTYPE::LGLSXP
            && XLENGTH(attr) >= 1
            && *LOGICAL(attr) == crate::sexp::ffi::TRUE
    }
}

unsafe fn dendrogram_attr_real(node: SEXP, name: &std::ffi::CStr, default: f64) -> f64 {
    unsafe {
        let attr = crate::sexp::attrib_core::getAttrib(
            node,
            crate::sexp::symbol::Rf_install(name.as_ptr()),
        );
        if attr.is_null() || attr == R_NilValue() || XLENGTH(attr) < 1 {
            default
        } else {
            elt_real_safe(attr, 0)
        }
    }
}
unsafe fn set_dendrogram_real(node: SEXP, name: &std::ffi::CStr, value: f64) {
    unsafe {
        let scalar = Rf_ScalarReal(value);
        let _scalar = protect(scalar);
        crate::sexp::attrib_core::setAttrib(
            node,
            crate::sexp::symbol::Rf_install(name.as_ptr()),
            scalar,
        );
    }
}

unsafe fn set_dendrogram_int(node: SEXP, name: &std::ffi::CStr, value: i32) {
    unsafe {
        let scalar = Rf_ScalarInteger(value);
        let _scalar = protect(scalar);
        crate::sexp::attrib_core::setAttrib(
            node,
            crate::sexp::symbol::Rf_install(name.as_ptr()),
            scalar,
        );
    }
}

unsafe fn set_dendrogram_value(node: SEXP, value: f64, integer_wts: bool) {
    unsafe {
        if integer_wts
            && value.is_finite()
            && value.fract() == 0.0
            && value >= i32::MIN as f64
            && value <= i32::MAX as f64
        {
            set_dendrogram_int(node, c"value", value as i32);
        } else if integer_wts && !value.is_finite() {
            set_dendrogram_int(node, c"value", crate::sexp::ffi::NA_INTEGER);
        } else {
            set_dendrogram_real(node, c"value", value);
        }
    }
}

unsafe fn copy_dendrogram_attr(dst: SEXP, src: SEXP, name: &std::ffi::CStr) {
    unsafe {
        let value = crate::sexp::attrib_core::getAttrib(
            src,
            crate::sexp::symbol::Rf_install(name.as_ptr()),
        );
        if !value.is_null() && value != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                dst,
                crate::sexp::symbol::Rf_install(name.as_ptr()),
                value,
            );
        }
    }
}

/// GNU `reorder.dendrogram` + `midcache.dendrogram` for `agglo.FUN = sum`.
///
/// The R walk duplicates list nodes on every `[[<-`. Rebuilding the binary
/// tree here keeps the same child order and midpoint formula.
unsafe fn reorder_dendrogram_sum(
    node: SEXP,
    wts: &[f64],
    integer_wts: bool,
) -> (
    SEXP,
    f64,
    Option<crate::sexp::protect::ProtectGuard<'static>>,
) {
    unsafe {
        if dendrogram_flag(node, c"leaf") || TYPEOF(node) != SEXPTYPE::VECSXP {
            let id = if TYPEOF(node) == SEXPTYPE::INTSXP && XLENGTH(node) >= 1 {
                *INTEGER(node)
            } else if XLENGTH(node) >= 1 {
                elt_real_safe(node, 0) as i32
            } else {
                0
            };
            let value = if id >= 1 && (id as usize) <= wts.len() {
                wts[(id as usize) - 1]
            } else {
                crate::sexp::ffi::NA_REAL
            };
            let node = crate::mainutils::duplicate::shallow_duplicate(node);
            let guard = protect(node);
            set_dendrogram_value(node, value, integer_wts);
            return (node, value, Some(guard));
        }
        let k = XLENGTH(node) as usize;
        let rebuilt = Rf_allocVector3(SEXPTYPE::VECSXP, k as i64);
        let guard = protect(rebuilt);
        let mut values = Vec::with_capacity(k);
        for i in 0..k {
            let (child, value, child_guard) =
                reorder_dendrogram_sum(VECTOR_ELT(node, i as i64), wts, integer_wts);
            SET_VECTOR_ELT(rebuilt, i as i64, child);
            drop(child_guard);
            values.push(value);
        }
        let mut order: Vec<usize> = (0..k).collect();
        order.sort_by(|&a, &b| {
            values[a]
                .partial_cmp(&values[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let kids: Vec<SEXP> = (0..k).map(|i| VECTOR_ELT(rebuilt, i as i64)).collect();
        for (i, &src) in order.iter().enumerate() {
            SET_VECTOR_ELT(rebuilt, i as i64, kids[src]);
        }
        let sum: f64 = values.iter().copied().sum();
        copy_dendrogram_attr(rebuilt, node, c"members");
        copy_dendrogram_attr(rebuilt, node, c"height");
        copy_dendrogram_attr(rebuilt, node, c"class");
        set_dendrogram_value(rebuilt, sum, integer_wts);
        (rebuilt, sum, Some(guard))
    }
}

unsafe fn midcache_dendrogram(node: SEXP) {
    unsafe {
        if dendrogram_flag(node, c"leaf") || TYPEOF(node) != SEXPTYPE::VECSXP {
            return;
        }
        let k = XLENGTH(node);
        if k < 1 {
            return;
        }
        for i in 0..k {
            midcache_dendrogram(VECTOR_ELT(node, i));
        }
        let mut mid_sum = 0.0;
        for i in 0..k {
            mid_sum += dendrogram_attr_real(VECTOR_ELT(node, i), c"midpoint", 0.0);
        }
        let first_members = dendrogram_attr_real(VECTOR_ELT(node, 0), c"members", 1.0);
        set_dendrogram_real(node, c"midpoint", (first_members + mid_sum) / 2.0);
    }
}

pub unsafe fn do_reorder_dendrogram(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let node = CAR(args);
        let wts_arg = CADR(args);
        if node.is_null() || node == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                call,
                "'reorder.dendrogram' requires a dendrogram",
            );
        }
        let n = if wts_arg.is_null() || wts_arg == R_NilValue() {
            0
        } else {
            XLENGTH(wts_arg)
        };
        let mut wts = Vec::with_capacity(n as usize);
        for i in 0..n {
            wts.push(elt_real_safe(wts_arg, i));
        }
        let (reordered, _value, guard) =
            reorder_dendrogram_sum(node, &wts, TYPEOF(wts_arg) == SEXPTYPE::INTSXP);
        midcache_dendrogram(reordered);
        std::mem::forget(guard);
        reordered
    }
}

/// GNU `order.dendrogram(x)` — leaf order as a vector.
pub unsafe fn do_order_dendrogram(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !class_contains(x, "dendrogram") {
            crate::mainutils::errors::errorcall_str(
                call,
                "'order.dendrogram' requires a dendrogram",
            );
        }
        let mut ids = Vec::new();
        fn walk(node: SEXP, ids: &mut Vec<f64>, integer: &mut bool) {
            unsafe {
                if TYPEOF(node) == SEXPTYPE::VECSXP {
                    let n = XLENGTH(node);
                    for i in 0..n {
                        walk(VECTOR_ELT(node, i), ids, integer);
                    }
                } else if TYPEOF(node) == SEXPTYPE::INTSXP && XLENGTH(node) >= 1 {
                    let v = *INTEGER(node);
                    ids.push(if v == crate::sexp::ffi::NA_INTEGER {
                        *integer = false;
                        crate::sexp::ffi::NA_REAL
                    } else {
                        v as f64
                    });
                } else if XLENGTH(node) >= 1 {
                    *integer = false;
                    ids.push(elt_real_safe(node, 0));
                }
            }
        }
        let mut integer = TYPEOF(x) != SEXPTYPE::REALSXP;
        walk(x, &mut ids, &mut integer);
        let result = if integer {
            let out = Rf_allocVector3(SEXPTYPE::INTSXP, ids.len() as i64);
            for (i, v) in ids.iter().enumerate() {
                *INTEGER(out).add(i) = *v as i32;
            }
            out
        } else {
            let out = Rf_allocVector3(SEXPTYPE::REALSXP, ids.len() as i64);
            for (i, v) in ids.iter().enumerate() {
                *REAL(out).add(i) = *v;
            }
            out
        };
        result
    }
}

/// GNU `heatmap(x)` — row/column leaf order from `hclust(dist())`.
pub unsafe fn do_heatmap(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) < 2
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must be a numeric matrix",
            );
        }
        let nr = *INTEGER(dim) as i32;
        let nc = *INTEGER(dim).add(1) as i32;
        if nr <= 1 || nc <= 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' must have at least 2 rows and 2 columns",
            );
        }
        let dist_sym = Rf_install(c"dist".as_ptr());
        let hclust_sym = Rf_install(c"hclust".as_ptr());
        let t_sym = Rf_install(c"t".as_ptr());
        let crow = crate::sexp::constructors::Rf_lang2(dist_sym, x);
        let _crow = protect(crow);
        let drow = crate::eval::eval::Rf_eval(crow, rho);
        let _dr = protect(drow);
        let hrowc = crate::sexp::constructors::Rf_lang2(hclust_sym, drow);
        let _hrowc = protect(hrowc);
        let hcrow = crate::eval::eval::Rf_eval(hrowc, rho);
        let _hr = protect(hcrow);
        let tc = crate::sexp::constructors::Rf_lang2(t_sym, x);
        let _tc = protect(tc);
        let xt = crate::eval::eval::Rf_eval(tc, rho);
        let _xt = protect(xt);
        let ccol = crate::sexp::constructors::Rf_lang2(dist_sym, xt);
        let _ccol = protect(ccol);
        let dcol = crate::eval::eval::Rf_eval(ccol, rho);
        let _dc = protect(dcol);
        let hcolc = crate::sexp::constructors::Rf_lang2(hclust_sym, dcol);
        let _hcolc = protect(hcolc);
        let hccol = crate::eval::eval::Rf_eval(hcolc, rho);
        let _hc = protect(hccol);
        let row_ind = list_named_elt(hcrow, "order");
        let col_ind = list_named_elt(hccol, "order");
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, row_ind);
        SET_VECTOR_ELT(result, 1, col_ind);
        SET_VECTOR_ELT(result, 2, R_NilValue());
        SET_VECTOR_ELT(result, 3, R_NilValue());
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "rowInd".to_string(),
                "colInd".to_string(),
                "Rowv".to_string(),
                "Colv".to_string(),
            ],
        );
        result
    }
}

/// GNU `rect.hclust(tree, k=, h=)` — cluster boxes; validates `k`/`h`.
pub unsafe fn do_rect_hclust(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let tree = CAR(args);
        let mut k_arg = R_NilValue();
        let mut h_arg = R_NilValue();
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            if name == "k" || (name.is_empty() && pos == 0) {
                k_arg = val;
            }
            if name == "h" || (name.is_empty() && pos == 3) {
                h_arg = val;
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let k_set = !k_arg.is_null() && k_arg != R_NilValue() && k_arg != R_MissingArg();
        let h_set = !h_arg.is_null() && h_arg != R_NilValue() && h_arg != R_MissingArg();
        if k_set == h_set {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "specify exactly one of 'k' and 'h'",
            );
        }
        let height = list_named_elt(tree, "height");
        let nh = if height.is_null() || height == R_NilValue() {
            0
        } else {
            XLENGTH(height)
        };
        let k = if h_set {
            let h = elt_real_safe(h_arg, 0);
            let mut found = None;
            for i in 0..nh {
                let hv = elt_real_safe(height, nh - 1 - i);
                if hv < h {
                    found = Some((i as i32) + 1);
                    break;
                }
            }
            found.unwrap_or(i32::MAX).max(2)
        } else {
            elt_real_safe(k_arg, 0).floor() as i32
        };
        if k < 2 || (k as i64) > nh {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &format!("k must be between 2 and {nh}"),
            );
        }
        let rect_sym = Rf_install(c"rect".as_ptr());
        let rc = crate::sexp::constructors::Rf_lang2(rect_sym, Rf_ScalarReal(0.0));
        let _rc = protect(rc);
        crate::eval::eval::Rf_eval(rc, rho)
    }
}

/// GNU `dendrapply(X, FUN)` — apply `FUN` to a dendrogram (leaf vector).
pub unsafe fn do_dendrapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let fun = CAR(CDR(args));
        if !class_contains(x, "dendrogram") {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'X' is not a dendrogram",
            );
        }
        if fun.is_null() || fun == R_NilValue() {
            return x;
        }
        let call = crate::sexp::constructors::Rf_lang2(fun, x);
        let _c = protect(call);
        crate::eval::eval::Rf_eval(call, rho)
    }
}

/// GNU `summary.stepfun` — print helper; return `NULL` invisibly.
pub unsafe fn do_summary_stepfun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}
