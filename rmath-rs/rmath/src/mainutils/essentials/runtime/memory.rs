//! Rprof, Rprofmem, gc, gc.time, gcinfo, gctorture, memory.size,
//! memory.profile, object.size.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
// ---------------------------------------------------------------------------

/// R's `Rprof(filename, ...)` — session-owned profiling.
pub unsafe fn do_Rprof(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = crate::eval::profiling::do_Rprof(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `Rprofmem(filename, ...)` — session-owned memory profiling.
pub unsafe fn do_Rprofmem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = crate::eval::profiling::do_Rprofmem(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RuntimeMemorySnapshot {
    active_nodes: usize,
    free_nodes: usize,
    current_bytes: usize,
    peak_bytes: usize,
}

fn runtime_memory_snapshot() -> RuntimeMemorySnapshot {
    crate::sexp::instance::with_required_current_instance(|instance| {
        let active_nodes = instance.arena.node_count();
        let free_nodes = instance.arena.free_count();
        let current_bytes = instance.arena.total_bytes_allocated();
        let peak_bytes = instance.gc_state.stats.peak_memory.max(current_bytes);

        RuntimeMemorySnapshot {
            active_nodes,
            free_nodes,
            current_bytes,
            peak_bytes,
        }
    })
}

fn bytes_to_mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn set_real_matrix_cell(data: *mut f64, row: usize, col: usize, rows: usize, value: f64) {
    unsafe {
        *data.add(col * rows + row) = value;
    }
}

/// R's `gc()` — garbage collection with session-owned memory counters.
///
/// Mirrors stock `base::gc`: the value is the `.Internal(gc(...))` counters
/// shaped into a 2x7 matrix — rows Ncells/Vcells, columns
/// `used (Mb) gc trigger (Mb) limit (Mb) max used (Mb)` — with the (Mb)
/// cells rounded up to 0.1 Mb. The `limit (Mb)` column reports the node
/// and vector-heap ceilings (NA where unset — upstream `R_MaxNSize` /
/// `R_MaxVSize`); on macOS the vector pool carries a startup default
/// (max(physical memory, 16 Gb)), so the column usually renders there.
/// stock `base::gc` drops the column when it is all-NA.
/// Unlike the old port, the result is *visible* (stock prints it).
pub unsafe fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::memory_main::R_gc();
        let snapshot = runtime_memory_snapshot();
        let node_size = std::mem::size_of::<crate::sexp::ffi::SexprecCore>();
        let ncell_bytes = snapshot.active_nodes.saturating_mul(node_size);
        let ncell_trigger = (snapshot.active_nodes + snapshot.free_nodes)
            .saturating_mul(2)
            .max(snapshot.active_nodes);
        let ncell_peak = snapshot
            .active_nodes
            .saturating_add(crate::sexp::gengc::get_gc_stats().freed);
        let vcell_size = std::mem::size_of::<SEXP>();
        let vcell_used = snapshot.current_bytes / vcell_size;
        let vcell_trigger_bytes = snapshot
            .current_bytes
            .saturating_mul(2)
            .max(snapshot.current_bytes);
        let vcell_peak = snapshot.peak_bytes / vcell_size;

        // R rounds the (Mb) columns up to 0.1 Mb.
        let mb = |bytes: usize| 0.1 * (10.0 * bytes_to_mb(bytes)).ceil();

        // Cell limits for the `limit (Mb)` column (memory.c do_gc): NA
        // unless a ceiling is set; Ncells counts nodes, Vcells is bytes.
        let max_n = crate::mainutils::memory_main::R_GetMaxNSize_memory();
        let max_v = crate::mainutils::memory_main::R_GetMaxVSize_memory();
        let limit = (
            if max_n == u64::MAX {
                NA_REAL
            } else {
                mb((max_n.saturating_mul(node_size as u64)) as usize)
            },
            if max_v == u64::MAX {
                NA_REAL
            } else {
                mb(max_v as usize)
            },
        );

        // Full stock layout, column-major with 2 rows:
        //   used | (Mb) | gc trigger | (Mb) | limit (Mb) | max used | (Mb)
        let full: [(f64, f64); 7] = [
            (snapshot.active_nodes as f64, vcell_used as f64),
            (mb(ncell_bytes), mb(snapshot.current_bytes)),
            (
                ncell_trigger as f64,
                (vcell_trigger_bytes / vcell_size) as f64,
            ),
            (
                mb(ncell_trigger.saturating_mul(node_size)),
                mb(vcell_trigger_bytes),
            ),
            limit,
            (ncell_peak as f64, vcell_peak as f64),
            (
                mb(ncell_peak.saturating_mul(node_size)),
                mb(snapshot.peak_bytes),
            ),
        ];

        // base::gc drops the `limit (Mb)` column when it is all-NA.
        let drop_limit = full[4].0.is_nan() && full[4].1.is_nan();
        let cols: Vec<&(f64, f64)> = if drop_limit {
            full.iter()
                .enumerate()
                .filter(|(i, _)| *i != 4)
                .map(|(_, col)| col)
                .collect()
        } else {
            full.iter().collect()
        };
        let col_names: Vec<&str> = if drop_limit {
            vec!["used", "(Mb)", "gc trigger", "(Mb)", "max used", "(Mb)"]
        } else {
            vec![
                "used",
                "(Mb)",
                "gc trigger",
                "(Mb)",
                "limit (Mb)",
                "max used",
                "(Mb)",
            ]
        };

        let ncols = cols.len();
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, (2 * ncols) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for (col, (ncell, vcell)) in cols.iter().enumerate() {
            set_real_matrix_cell(dst, 0, col, 2, *ncell);
            set_real_matrix_cell(dst, 1, col, 2, *vcell);
        }

        // dim = c(2, ncol)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _p2 = protect(dim);
            let d = INTEGER(dim);
            *d.add(0) = 2;
            *d.add(1) = ncols as c_int;
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dim".as_ptr()), dim);
        }
        // dimnames = list(c("Ncells", "Vcells"), col_names)
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if !dn.is_null() {
            let _p3 = protect(dn);
            let row_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            if !row_names.is_null() {
                let _p4 = protect(row_names);
                let s1 = c"Ncells";
                let s2 = c"Vcells";
                SET_STRING_ELT(
                    row_names,
                    0,
                    crate::sexp::constructors::Rf_mkChar(s1.as_ptr()),
                );
                SET_STRING_ELT(
                    row_names,
                    1,
                    crate::sexp::constructors::Rf_mkChar(s2.as_ptr()),
                );
                SET_VECTOR_ELT(dn, 0, row_names);
            }
            let col_names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
            if !col_names_vec.is_null() {
                let _p5 = protect(col_names_vec);
                for (i, name) in col_names.iter().enumerate() {
                    let cstr = CString::new(*name).unwrap_or_default();
                    SET_STRING_ELT(
                        col_names_vec,
                        i as R_xlen_t,
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                    );
                }
                SET_VECTOR_ELT(dn, 1, col_names_vec);
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dimnames".as_ptr()), dn);
        }
        result
    }
}

/// R's `gc.time()` — current GC timing counters.
pub unsafe fn do_gc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::REALSXP, 5) }
}

/// R's `gcinfo(on)` — set session-local GC reporting verbosity.
pub unsafe fn do_gcinfo(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            base_error("argument \"verbose\" is missing, with no default");
        }
        let old = crate::mainutils::memory_main::do_gcinfo(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `gctorture(on = TRUE)` — set session-local GC torture mode.
pub unsafe fn do_gctorture(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let on = if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            Rf_ScalarLogical(TRUE)
        } else {
            CAR(args)
        };
        let normalized = Rf_cons(on, R_NilValue());
        let _args_guard = protect(normalized);
        let old = crate::mainutils::memory_main::do_gctorture(call, op, normalized, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `gctorture2(step, wait = 0, inhibit_release = FALSE)` session state.
pub unsafe fn do_gctorture2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            base_error("argument \"step\" is missing, with no default");
        }

        let step = CAR(args);
        let wait =
            if CDR(args).is_null() || CDR(args) == R_NilValue() || CAR(CDR(args)) == R_MissingArg()
            {
                Rf_ScalarInteger(0)
            } else {
                CAR(CDR(args))
            };
        let _wait_guard = protect(wait);
        let tail = Rf_cons(wait, R_NilValue());
        let _tail_guard = protect(tail);
        let normalized = Rf_cons(step, tail);
        let _args_guard = protect(normalized);
        let old = crate::mainutils::memory_main::do_gctorture2(call, op, normalized, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `memory.size(max)` — current or peak arena memory in MB.
pub unsafe fn do_memory_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let max = crate::mainutils::coerce::asLogical(CAR(args));
        let snapshot = runtime_memory_snapshot();
        let bytes = if max == TRUE {
            snapshot.peak_bytes
        } else {
            snapshot.current_bytes
        };
        Rf_ScalarReal(bytes_to_mb(bytes))
    }
}

/// R's `memory.profile()` — session-local object counts by SEXPTYPE class.
pub unsafe fn do_memory_profile(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    const PROFILE_TYPES: [(&str, SEXPTYPE); 24] = [
        ("NULL", SEXPTYPE::NILSXP),
        ("symbol", SEXPTYPE::SYMSXP),
        ("pairlist", SEXPTYPE::LISTSXP),
        ("closure", SEXPTYPE::CLOSXP),
        ("environment", SEXPTYPE::ENVSXP),
        ("promise", SEXPTYPE::PROMSXP),
        ("language", SEXPTYPE::LANGSXP),
        ("special", SEXPTYPE::SPECIALSXP),
        ("builtin", SEXPTYPE::BUILTINSXP),
        ("char", SEXPTYPE::CHARSXP),
        ("logical", SEXPTYPE::LGLSXP),
        ("integer", SEXPTYPE::INTSXP),
        ("double", SEXPTYPE::REALSXP),
        ("complex", SEXPTYPE::CPLXSXP),
        ("character", SEXPTYPE::STRSXP),
        ("...", SEXPTYPE::DOTSXP),
        ("any", SEXPTYPE::ANYSXP),
        ("list", SEXPTYPE::VECSXP),
        ("expression", SEXPTYPE::EXPRSXP),
        ("bytecode", SEXPTYPE::BCODESXP),
        ("externalptr", SEXPTYPE::EXTPTRSXP),
        ("weakref", SEXPTYPE::WEAKREFSXP),
        ("raw", SEXPTYPE::RAWSXP),
        ("S4", SEXPTYPE::S4SXP),
    ];

    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, PROFILE_TYPES.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let data = INTEGER(result);
        for i in 0..PROFILE_TYPES.len() {
            *data.add(i) = 0;
        }
        *data = 1;

        crate::sexp::instance::with_required_current_instance(|instance| {
            for node in instance.arena.active_nodes() {
                let ty = TYPEOF(node);
                if let Some((idx, _)) = PROFILE_TYPES
                    .iter()
                    .enumerate()
                    .find(|(_, (_, profile_ty))| ty == *profile_ty)
                {
                    // `S4SXP` shares the OBJSXP tag; match GNU R's public bucket name.
                    *data.add(idx) = (*data.add(idx)).saturating_add(1);
                }
            }
        });

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, PROFILE_TYPES.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in PROFILE_TYPES.iter().enumerate() {
                SET_STRING_ELT(
                    names,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        result
    }
}

/// R's `object.size(x)` — estimate object size in bytes (simplified).
/// Returns a numeric scalar with class "object_size".
pub unsafe fn do_object_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let result = Rf_ScalarReal(0.0);
            let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if !class_vec.is_null() {
                let _p2 = protect(class_vec);
                let cstr = c"object_size";
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                    *cdata.add(0) = charsxp;
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(c"class".as_ptr()),
                    class_vec,
                );
            }
            return result;
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let size: f64 = match t {
            t if t == SEXPTYPE::REALSXP => (n as usize * std::mem::size_of::<f64>()) as f64,
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                (n as usize * std::mem::size_of::<i32>()) as f64
            }
            t if t == SEXPTYPE::STRSXP => {
                let mut total: usize = 0;
                for i in 0..n {
                    let charsxp = STRING_ELT(x, i);
                    if !charsxp.is_null() {
                        let s = CHAR(charsxp);
                        if !s.is_null() {
                            let cstr = std::ffi::CStr::from_ptr(s);
                            total += cstr.to_bytes().len() + 1;
                        }
                    }
                }
                total as f64
            }
            t if t == SEXPTYPE::VECSXP => {
                let mut total: usize = std::mem::size_of::<SEXP>() * n as usize;
                for i in 0..n {
                    let elt = VECTOR_ELT(x, i);
                    if !elt.is_null() {
                        let elt_size = do_object_size(
                            _call,
                            _op,
                            {
                                // Create a temporary pairlist with elt as first arg
                                let cell = Rf_cons(elt, R_NilValue());
                                cell
                            },
                            _rho,
                        );
                        total += real_or_default(elt_size, 0.0) as usize;
                    }
                }
                total as f64
            }
            _ => 64.0, // Default estimate for headers
        };
        let result = Rf_ScalarReal(size);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _p2 = protect(class_vec);
            let cstr = c"object_size";
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                *cdata.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        }
        result
    }
}
