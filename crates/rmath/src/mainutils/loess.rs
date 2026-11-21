//! R-facing adapters for the owned, pointer-free LOESS numerical engine.
use crate::library::stats::loess::{Config, Execution, Model};
use crate::mainutils::essentials::{base_error, elt_to_string};
use crate::sexp::attrib_core::setAttrib;
use crate::sexp::attrib_core::{R_ClassSymbol, R_DimSymbol, R_NamesSymbol};
use crate::sexp::{
    accessors::*,
    constructors::*,
    context::r_error,
    ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE},
    globals::R_NilValue,
    protect::{ProtectGuard, protect},
};
use std::collections::HashSet;

fn check_execution() -> Result<(), String> {
    if crate::sexp::instance::is_cancellation_requested() {
        Err("operation cancelled".into())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Checked-access boundary helpers
//
// The LOESS adapters below run inside evaluated R calls, so every raw SEXP
// they handle is live under the active runtime. The few operations that still
// need unsafe are kept here as small audited boundaries so the adapters can
// stay on checked element access.
// ---------------------------------------------------------------------------

/// Checked node factory bound to the active runtime session.
fn node_factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: LOESS helpers only run inside evaluated R calls.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| r_error(error.to_string())),
    )
}

fn wrapped(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    node_factory()
        .wrap(x)
        .unwrap_or_else(|error| r_error(error.to_string()))
}

fn mutable_vector(x: SEXP) -> crate::sexp::object::SexpMut<'static> {
    node_factory()
        .wrap(x)
        .and_then(crate::sexp::object::SexpMut::try_from_checked)
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked element of a generic vector; matches `VECTOR_ELT` on live input.
fn vector_element(x: SEXP, i: R_xlen_t) -> SEXP {
    wrapped(x)
        .try_vector_elt(i)
        .map(|element| element.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked attribute lookup on a live object.
fn attribute(x: SEXP, symbol: SEXP) -> SEXP {
    crate::sexp::attrib_core::getAttrib(x, symbol)
}

/// Named-attribute write on a live object.
fn set_attribute(x: SEXP, symbol: SEXP, value: SEXP) {
    setAttrib(x, symbol, value)
}

/// Audited boundary: evaluate a live expression in a live environment.
fn eval_sexp(expr: SEXP, env: SEXP) -> SEXP {
    // SAFETY: evaluates under the same ambient contract as the evaluator.
    unsafe { crate::eval::eval::Rf_eval(expr, env) }
}

/// Bind one symbol in a live environment (GNU `defineVar`).
fn define_var(symbol: SEXP, value: SEXP, env: SEXP) {
    let _ = crate::sexp::envir::define_var_updates(symbol, value, env);
}

/// Audited boundary: evaluate a base wrapper function on live arguments.
fn apply_base(name: &'static str, source: &str, args: SEXP, env: SEXP, full: bool) -> SEXP {
    // SAFETY: apply builds and evaluates the wrapper under the active runtime.
    unsafe { crate::mainutils::base_wrappers::apply(name, source, args, env, full) }
}

/// Audited boundary: read a live symbol's print name as Rust text.
fn symbol_name(symbol: SEXP) -> String {
    String::from_utf8_lossy(&crate::mainutils::printutils::charsxp_bytes(PRINTNAME(
        symbol,
    )))
    .into_owned()
}

/// Checked CHARSXP from Rust text; matches `Rf_mkChar` on the session heap.
fn mk_char(text: &str) -> SEXP {
    node_factory()
        .character(text)
        .map(|chars| chars.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked STRSXP(1) scalar; matches `Rf_mkString` on the session heap.
fn mk_string(text: &str) -> SEXP {
    node_factory()
        .strings(&[text])
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked vector allocation; matches `Rf_allocVector3` on the session heap.
fn alloc_vector(kind: SEXPTYPE, length: R_xlen_t) -> SEXP {
    node_factory()
        .allocate(|arena| Some(arena.alloc_vector(kind, length)))
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked REALSXP built from Rust values on the session heap.
fn numbers(values: &[f64]) -> SEXP {
    let mut vector = mutable_vector(alloc_vector(SEXPTYPE::REALSXP, values.len() as R_xlen_t));
    for (i, value) in values.iter().enumerate() {
        vector
            .try_set_real_elt(i as R_xlen_t, *value)
            .unwrap_or_else(|error| r_error(error.to_string()));
    }
    vector.freeze().as_raw()
}

/// Checked INTSXP built from Rust values on the session heap.
fn integers(values: &[i32]) -> SEXP {
    let mut vector = mutable_vector(alloc_vector(SEXPTYPE::INTSXP, values.len() as R_xlen_t));
    for (i, value) in values.iter().enumerate() {
        vector
            .try_set_integer_elt(i as R_xlen_t, *value)
            .unwrap_or_else(|error| r_error(error.to_string()));
    }
    vector.freeze().as_raw()
}

/// Checked `names<-` on a freshly allocated vector.
fn set_names(x: SEXP, names: SEXP) {
    set_attribute(x, R_NamesSymbol(), names);
}

pub(crate) fn do_loess(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    apply_base("loess", include_str!("loess.R"), args, rho, false)
}
pub(crate) fn do_control(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    apply_base(
        "loess.control",
        r#"function(surface=c('interpolate','direct'), statistics=c('approximate','exact','none'), trace.hat=c('exact','approximate'), cell=0.2,iterations=4L,iterTrace=FALSE,...) {
        stopifnot(length(iterations)==1L, !is.na(iterations), iterations>0L)
        list(surface=match.arg(surface,c("interpolate","direct")),statistics=match.arg(statistics,c("approximate","exact","none")),trace.hat=match.arg(trace.hat,c("exact","approximate")),cell=cell,iterations=iterations,iterTrace=iterTrace)
    }"#,
        args,
        rho,
        false,
    )
}
pub(crate) fn do_predict(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    apply_base(
        "predict",
        "function(object,...) UseMethod('predict')",
        args,
        rho,
        false,
    )
}

pub(crate) fn do_predict_lm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, CDR, TAG};
    use crate::sexp::constructors::Rf_cons;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::protect;
    let mut ev = R_NilValue();
    let mut tail = R_NilValue();
    let mut cell = args;
    while !cell.is_null() && cell != R_NilValue() {
        let val = eval_sexp(CAR(cell), rho);
        let node = Rf_cons(val, R_NilValue());
        let _n = protect(node);
        crate::sexp::accessors::SETTAG(node, TAG(cell));
        if ev.is_null() || ev == R_NilValue() {
            ev = node;
            tail = node;
        } else {
            crate::sexp::accessors::SETCDR(tail, node);
            tail = node;
        }
        cell = CDR(cell);
    }
    crate::mainutils::essentials::do_predict_lm(call, op, ev, rho)
}
pub(crate) fn do_predict_loess(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    apply_base(
        "predict.loess",
        r#"function(object,newdata=NULL,se=FALSE,na.action=na.pass,...) {
                if (!inherits(object, "loess")) stop('first argument must be a "loess" object')
                .rport_loess_predict(object,newdata,se)
            }"#,
        args,
        rho,
        false,
    )
}

struct ListBuilder {
    fields: Vec<(&'static str, SEXP)>,
    roots: Vec<ProtectGuard<'static>>,
}
impl ListBuilder {
    fn new() -> Self {
        Self {
            fields: vec![],
            roots: vec![],
        }
    }
    fn push(&mut self, key: &'static str, value: SEXP) {
        self.roots.push(protect(value));
        self.fields.push((key, value));
    }
    fn finish(self) -> SEXP {
        let result = alloc_vector(SEXPTYPE::VECSXP, self.fields.len() as R_xlen_t);
        let _root = protect(result);
        let names = alloc_vector(SEXPTYPE::STRSXP, self.fields.len() as R_xlen_t);
        let _names = protect(names);
        let mut result_mut = mutable_vector(result);
        let mut names_mut = mutable_vector(names);
        for (i, (key, value)) in self.fields.iter().enumerate() {
            result_mut
                .try_set_vector_elt(i as R_xlen_t, wrapped(*value))
                .unwrap_or_else(|error| r_error(error.to_string()));
            names_mut
                .try_set_string_elt(i as R_xlen_t, wrapped(mk_char(key)))
                .unwrap_or_else(|error| r_error(error.to_string()));
        }
        set_names(result, names);
        result
    }
}
macro_rules! rlist {($($key:literal => $value:expr),* $(,)?)=>{{let mut list=ListBuilder::new();$(list.push($key,$value);)*list.finish()}};}

fn numeric(x: SEXP) -> Vec<f64> {
    if x == R_NilValue() {
        return vec![];
    }
    if !matches!(
        SEXPTYPE(TYPEOF(x)),
        SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
    ) {
        base_error("LOESS predictors, responses and weights must be numeric");
    }
    let sexp = wrapped(x);
    (0..XLENGTH(x))
        .map(|i| match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::REALSXP => sexp
                .try_real_elt(i)
                .unwrap_or_else(|error| r_error(error.to_string())),
            SEXPTYPE::INTSXP => {
                let v = sexp
                    .try_integer_elt(i)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                if v == i32::MIN { f64::NAN } else { v as f64 }
            }
            _ => {
                let v = sexp
                    .try_logical_elt(i)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                if v == i32::MIN { f64::NAN } else { v as f64 }
            }
        })
        .collect()
}

fn field(x: SEXP, name: &str) -> SEXP {
    if TYPEOF(x) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let names = attribute(x, R_NamesSymbol());
    if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) != XLENGTH(x) {
        return R_NilValue();
    }
    for i in 0..XLENGTH(x) {
        if elt_to_string(names, i) == name {
            return vector_element(x, i);
        }
    }
    R_NilValue()
}

fn scalar(x: SEXP) -> f64 {
    let v = numeric(x);
    if v.len() != 1 {
        base_error("expected a scalar LOESS parameter");
    }
    v[0]
}
fn string(x: SEXP) -> String {
    if TYPEOF(x) != SEXPTYPE::STRSXP || XLENGTH(x) != 1 {
        base_error("expected a character LOESS parameter");
    }
    elt_to_string(x, 0)
}
fn positional(args: SEXP) -> Vec<SEXP> {
    let mut result = vec![];
    let mut p = args;
    while !p.is_null() && p != R_NilValue() {
        result.push(CAR(p));
        p = CDR(p);
    }
    result
}
fn predictors(expr: SEXP, out: &mut Vec<SEXP>) {
    if TYPEOF(expr) == SEXPTYPE::LANGSXP && TYPEOF(CAR(expr)) == SEXPTYPE::SYMSXP {
        let name = symbol_name(CAR(expr));
        if name == "+" {
            predictors(CADR(expr), out);
            if CDR(CDR(expr)) != R_NilValue() {
                predictors(CAR(CDR(CDR(expr))), out);
            }
            return;
        }
        if matches!(name.as_ref(), "*" | ":" | "-" | "|") {
            base_error("LOESS formula requires additive numeric predictors");
        }
    }
    out.push(expr);
}
fn data_environment(data: SEXP, parent: SEXP) -> SEXP {
    if TYPEOF(data) == SEXPTYPE::ENVSXP {
        return data;
    }
    if data != R_NilValue() && TYPEOF(data) != SEXPTYPE::VECSXP {
        base_error("'data' must be a data frame, list or environment");
    }
    let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
    let _env = protect(env);
    let names = attribute(data, R_NamesSymbol());
    if data != R_NilValue() {
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) != XLENGTH(data) {
            base_error("LOESS data columns must be named");
        }
        for i in 0..XLENGTH(data) {
            define_var(
                crate::sexp::symbol::install_str(&elt_to_string(names, i)),
                vector_element(data, i),
                env,
            );
        }
    }
    env
}
fn predictor_columns(expressions: &[SEXP], env: SEXP) -> Vec<Vec<f64>> {
    predictor_data(expressions, env).0
}

/// Match the column labels produced by GNU's as.matrix(model.frame(...)).
fn predictor_data(expressions: &[SEXP], env: SEXP) -> (Vec<Vec<f64>>, Vec<String>) {
    let mut columns = vec![];
    let mut names = vec![];
    for expr in expressions {
        let value = eval_sexp(*expr, env);
        let _value = protect(value);
        let values = numeric(value);
        let label = crate::mainutils::deparse::deparse1line(*expr, false);
        let label = elt_to_string(label, 0);
        let dim = attribute(value, R_DimSymbol());
        if dim == R_NilValue() {
            columns.push(values);
            names.push(label);
            continue;
        }
        if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 2 {
            base_error("invalid LOESS predictor matrix");
        }
        let dims = wrapped(dim);
        let dim0 = dims
            .try_integer_elt(0)
            .unwrap_or_else(|error| r_error(error.to_string()));
        let dim1 = dims
            .try_integer_elt(1)
            .unwrap_or_else(|error| r_error(error.to_string()));
        if dim0 < 0 || dim1 < 1 {
            base_error("invalid LOESS predictor matrix");
        }
        let n = dim0 as usize;
        let d = dim1 as usize;
        if n.checked_mul(d) != Some(values.len()) {
            base_error("invalid LOESS predictor dimensions");
        }
        let dimnames = attribute(value, crate::sexp::attrib_core::R_DimNamesSymbol());
        let colnames = if TYPEOF(dimnames) == SEXPTYPE::VECSXP && XLENGTH(dimnames) == 2 {
            vector_element(dimnames, 1)
        } else {
            R_NilValue()
        };
        for j in 0..d {
            columns.push(values[j * n..(j + 1) * n].to_vec());
            names.push(if d == 1 {
                label.clone()
            } else {
                let column =
                    if TYPEOF(colnames) == SEXPTYPE::STRSXP && XLENGTH(colnames) == d as i64 {
                        elt_to_string(colnames, j as i64)
                    } else {
                        (j + 1).to_string()
                    };
                format!("{label}.{column}")
            });
        }
    }
    (columns, names)
}

/// GNU interprets these arguments as subscripts into a named predictor vector,
/// then matches the selected names back to the original names. Use the same R
/// operations so logical recycling, numeric exclusion, missing indices and
/// character selection retain their ordinary subscript semantics.
fn selected_flags(selection: SEXP, names: &[String], rho: SEXP) -> Vec<bool> {
    let labels = alloc_vector(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
    let _labels = protect(labels);
    let mut labels_mut = mutable_vector(labels);
    for (i, name) in names.iter().enumerate() {
        labels_mut
            .try_set_string_elt(i as R_xlen_t, wrapped(mk_char(name)))
            .unwrap_or_else(|error| r_error(error.to_string()));
    }
    let tail = Rf_cons(selection, R_NilValue());
    let _tail = protect(tail);
    let args = Rf_cons(labels, tail);
    let _args = protect(args);
    let normalized = apply_base(
        ".rport_loess_predictor_flags",
        "function(nmx, selected) { names(nmx) <- nmx; match(nmx, nmx[selected], 0L) > 0L }",
        args,
        rho,
        true,
    );
    let _normalized = protect(normalized);
    flags(normalized, names.len())
}

fn flags(x: SEXP, d: usize) -> Vec<bool> {
    let v = numeric(x);
    if v.iter().any(|x| !x.is_finite()) || (v.len() != 1 && v.len() != d) {
        base_error("wrong length for LOESS predictor flags");
    }
    v.iter().cycle().take(d).map(|x| *x != 0.).collect()
}

pub(crate) fn do_fit(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let a = positional(args);
        if a.len() != 15 {
            base_error("invalid LOESS internal arguments");
        }
        let formula = a[0];
        if TYPEOF(formula) != SEXPTYPE::LANGSXP || Rf_length(formula) != 3 {
            base_error("invalid LOESS formula");
        }
        let parent = attribute(formula, crate::sexp::symbol::install_str(".Environment"));
        let parent = if parent == R_NilValue() { rho } else { parent };
        let env = data_environment(a[1], parent);
        let _env = protect(env);
        let mut expressions = vec![];
        predictors(CAR(CDR(CDR(formula))), &mut expressions);
        let eval = |expr| eval_sexp(expr, env);
        let y_value = eval(CADR(formula));
        let _y_value = protect(y_value);
        let y = numeric(y_value);
        let n = y.len();
        let (cols, predictor_names) = predictor_data(&expressions, env);
        let d = cols.len();
        if !(1..=4).contains(&d) {
            base_error("only 1-4 predictors are allowed");
        }
        if cols.iter().any(|c| c.len() != n) {
            base_error("variable lengths differ in LOESS model");
        }
        let mut weights = numeric(eval(a[2]));
        if weights.is_empty() {
            weights = vec![1.; n];
        }
        if weights.len() != n {
            base_error("invalid LOESS weights length");
        }
        let control = a[11];
        let surface = string(field(control, "surface"));
        let statistics = string(field(control, "statistics"));
        let family = string(a[10]);
        if !matches!(surface.as_str(), "direct" | "interpolate")
            || !matches!(statistics.as_str(), "exact" | "approximate" | "none")
        {
            base_error("invalid LOESS control parameters");
        }
        let iterations = scalar(field(control, "iterations"));
        if !iterations.is_finite() || iterations < 1. || iterations > i32::MAX as f64 {
            base_error("invalid LOESS iterations");
        }
        if y.iter()
            .chain(&weights)
            .chain(cols.iter().flatten())
            .any(|v| v.is_infinite())
        {
            base_error("infinite values in LOESS data");
        }
        let degree = scalar(a[6]);
        if degree.fract() != 0. || !(0. ..=2.).contains(&degree) {
            base_error("'degree' must be 0, 1 or 2");
        }
        let mut span = scalar(a[4]);
        let drops = selected_flags(a[8], &predictor_names, rho);
        if a[5] != R_NilValue() {
            let target = scalar(a[5]);
            if target <= 0. {
                base_error("invalid 'enp.target'");
            }
            let tau = match degree as usize {
                0 => 1,
                1 => d + 1,
                _ => (d + 1) * (d + 2) / 2 - drops.iter().filter(|x| **x).count(),
            };
            span = 1.2 * tau as f64 / target;
        }
        if scalar(field(control, "iterTrace")) != 0. {
            base_error("LOESS iterTrace is not implemented");
        }
        let config = Config {
            span,
            degree: degree as usize,
            normalize: scalar(a[9]) != 0.,
            parametric: selected_flags(a[7], &predictor_names, rho),
            drop_square: drops,
            interpolate: surface == "interpolate",
            cell: scalar(field(control, "cell")),
            iterations: if family == "symmetric" {
                iterations as usize
            } else {
                1
            },
            exact: statistics == "exact",
            approximate_trace: string(field(control, "trace.hat")) == "approximate",
        };
        let subset = eval(a[3]);
        let selected: Vec<usize> = if subset == R_NilValue() {
            (0..n).collect()
        } else {
            let v = numeric(subset);
            if v.iter().any(|x| !x.is_finite()) {
                base_error("missing LOESS subset indices are not supported");
            }
            if TYPEOF(subset) == SEXPTYPE::LGLSXP {
                if v.len() > n {
                    base_error("LOESS logical subset is longer than data");
                }
                (0..n)
                    .filter(|i| !v.is_empty() && v[i % v.len()] != 0.)
                    .collect()
            } else if v.iter().any(|x| *x < 0.) {
                if v.iter().any(|x| *x > 0.) {
                    base_error("only 0's may be mixed with negative subscripts");
                }
                (0..n)
                    .filter(|i| !v.iter().any(|x| (-x.trunc()) == (*i + 1) as f64))
                    .collect()
            } else {
                v.iter()
                    .filter(|x| x.trunc() != 0.)
                    .map(|x| {
                        if x.trunc() > n as f64 {
                            base_error("LOESS subset index out of bounds");
                        }
                        x.trunc() as usize - 1
                    })
                    .collect()
            }
        };
        let complete: Vec<_> = selected
            .iter()
            .copied()
            .filter(|i| {
                y[*i].is_finite()
                    && weights[*i].is_finite()
                    && cols.iter().all(|c| c[*i].is_finite())
            })
            .collect();
        let na_name = if TYPEOF(a[14]) == SEXPTYPE::SYMSXP {
            symbol_name(a[14])
        } else {
            String::new()
        };
        if na_name == "na.fail" && complete.len() != selected.len() {
            base_error("missing values in object");
        }
        if !matches!(na_name.as_str(), "na.omit" | "na.exclude" | "na.fail") {
            base_error("LOESS currently supports na.omit, na.exclude and na.fail");
        }
        let na_action = omission_action(a[1], y_value, &selected, &complete, &na_name);
        let _na_action = protect(na_action);
        let x: Vec<Vec<_>> = complete
            .iter()
            .map(|i| cols.iter().map(|c| c[*i]).collect())
            .collect();
        let y: Vec<_> = complete.iter().map(|i| y[*i]).collect();
        let weights: Vec<_> = complete.iter().map(|i| weights[*i]).collect();
        let execution = Execution::new(&check_execution);
        let mut model =
            Model::fit_with_execution(x.clone(), y.clone(), weights.clone(), config, &execution)
                .unwrap_or_else(|e| base_error(e));
        // GNU's ehg131 diagnostics surface as R warnings during the fit,
        // attributed to the calling closure (GNU .C pushes no
        // warning-visible context).
        for message in execution.take_warnings() {
            let mut bytes = message.into_bytes();
            bytes.push(0);
            crate::mainutils::errors::warningcall(
                crate::mainutils::errors::findCall(),
                bytes.as_ptr() as *const std::ffi::c_char,
            );
        }
        if statistics == "none" {
            model.trace = 0.;
            model.delta1 = 0.;
            model.delta2 = 0.;
            model.s = if model.s == 0. || model.s.is_nan() {
                f64::NAN
            } else {
                f64::INFINITY
            };
        }
        let frame = if scalar(a[12]) != 0. {
            let frame = alloc_vector(SEXPTYPE::VECSXP, (expressions.len() + 1) as R_xlen_t);
            let _frame = protect(frame);
            let names = alloc_vector(SEXPTYPE::STRSXP, (expressions.len() + 1) as R_xlen_t);
            let _names = protect(names);
            let mut frame_mut = mutable_vector(frame);
            let mut names_mut = mutable_vector(names);
            for (j, expr) in std::iter::once(&CADR(formula))
                .chain(expressions.iter())
                .enumerate()
            {
                let value = eval(*expr);
                let _value = protect(value);
                // Preserve matrix-valued predictors in the model frame.
                let dim = attribute(value, R_DimSymbol());
                let nc = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    wrapped(dim)
                        .try_integer_elt(1)
                        .unwrap_or_else(|error| r_error(error.to_string()))
                        as usize
                } else {
                    1
                };
                let raw = numeric(value);
                let filtered = numbers(
                    &(0..nc)
                        .flat_map(|c| {
                            complete.iter().map({
                                let raw = &raw;
                                move |i| raw[c * n + *i]
                            })
                        })
                        .collect::<Vec<_>>(),
                );
                let _filtered = protect(filtered);
                if dim != R_NilValue() {
                    let dims = integers(&[complete.len() as i32, nc as i32]);
                    set_attribute(filtered, R_DimSymbol(), dims);
                }
                frame_mut
                    .try_set_vector_elt(j as R_xlen_t, wrapped(filtered))
                    .unwrap_or_else(|error| r_error(error.to_string()));
                let label = crate::mainutils::deparse::deparse1line(*expr, false);
                names_mut
                    .try_set_string_elt(j as R_xlen_t, wrapped(STRING_ELT(label, 0)))
                    .unwrap_or_else(|error| r_error(error.to_string()));
            }
            set_names(frame, names);
            set_attribute(frame, R_ClassSymbol(), mk_string("data.frame"));
            let rows = integers(&[i32::MIN, -(complete.len() as i32)]);
            set_attribute(frame, crate::sexp::symbol::install_str("row.names"), rows);
            frame
        } else {
            R_NilValue()
        };
        let _frame = protect(frame);
        let result = model_to_r(&model, &x, formula, control, a[13], frame, na_action);

        result
    }
}

fn model_to_r(
    m: &Model,
    x: &[Vec<f64>],
    formula: SEXP,
    control: SEXP,
    call: SEXP,
    frame: SEXP,
    na_action: SEXP,
) -> SEXP {
    {
        let n = m.y.len();
        let d = m.divisor.len();
        let matrix = numbers(
            &(0..d)
                .flat_map(|j| x.iter().map(move |row| row[j]))
                .collect::<Vec<_>>(),
        );
        let _matrix = protect(matrix);
        let dim = integers(&[n as i32, d as i32]);
        set_attribute(matrix, R_DimSymbol(), dim);
        let pars = rlist!("span"=>Rf_ScalarReal(m.config.span),"degree"=>Rf_ScalarInteger(m.config.degree as i32),"normalize"=>Rf_ScalarLogical(i32::from(m.config.normalize)),"parametric"=>numbers(&m.config.parametric.iter().map(|x|f64::from(*x)).collect::<Vec<_>>()),"drop.square"=>numbers(&m.config.drop_square.iter().map(|x|f64::from(*x)).collect::<Vec<_>>()),"surface"=>mk_string(if m.config.interpolate{"interpolate"}else{"direct"}),"cell"=>Rf_ScalarReal(m.config.cell),"family"=>mk_string(if m.config.iterations>1{"symmetric"}else{"gaussian"}),"iterations"=>Rf_ScalarInteger(m.config.iterations as i32));
        let _pars = protect(pars);
        let mut result_builder = ListBuilder::new();
        result_builder.push("n", Rf_ScalarInteger(n as i32));
        result_builder.push("fitted", numbers(&m.fitted));
        result_builder.push(
            "residuals",
            numbers(
                &m.y.iter()
                    .zip(&m.fitted)
                    .map(|(y, f)| y - f)
                    .collect::<Vec<_>>(),
            ),
        );
        result_builder.push("enp", Rf_ScalarReal(m.delta1 + 2. * m.trace - n as f64));
        result_builder.push("s", Rf_ScalarReal(m.s));
        result_builder.push("one.delta", Rf_ScalarReal(m.delta1));
        result_builder.push("two.delta", Rf_ScalarReal(m.delta2));
        result_builder.push("trace.hat", Rf_ScalarReal(m.trace));
        result_builder.push("divisor", numbers(&m.divisor));
        result_builder.push("robust", numbers(&m.robust));
        result_builder.push("pars", pars);
        result_builder.push("x", matrix);
        result_builder.push("y", numbers(&m.y));
        result_builder.push("weights", numbers(&m.weights));
        result_builder.push("formula", formula);
        result_builder.push("control", control);
        result_builder.push("call", call);
        if na_action != R_NilValue() {
            result_builder.push("na.action", na_action);
        }
        let result = result_builder.finish();
        let _result = protect(result);
        let result = if frame != R_NilValue() {
            let mut list = ListBuilder::new();
            // Keys are static so the rooted builder cannot retain borrowed names.
            for key in [
                "n",
                "fitted",
                "residuals",
                "enp",
                "s",
                "one.delta",
                "two.delta",
                "trace.hat",
                "divisor",
                "robust",
                "pars",
                "x",
                "y",
                "weights",
                "formula",
                "control",
                "call",
            ] {
                list.push(key, field(result, key));
            }
            list.push("model", frame);
            if na_action != R_NilValue() {
                list.push("na.action", na_action);
            }
            list.finish()
        } else {
            result
        };
        let _result = protect(result);
        set_attribute(result, R_ClassSymbol(), mk_string("loess"));
        result
    }
}

pub(crate) fn do_predict_core(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let a = positional(args);
        if a.len() != 3 {
            base_error("invalid LOESS prediction arguments");
        }
        let obj = a[0];
        // GNU predict.loess returns fitted(object) before admitting numerical
        // prediction metadata when no new data or uncertainty was requested.
        if a[1] == R_NilValue() && scalar(a[2]) == 0.0 {
            let fitted = field(obj, "fitted");
            let _fitted = protect(fitted);
            let action = field(obj, "na.action");
            return if is_exclude_action(action) {
                restore_excluded_fit(&numeric(fitted), action)
            } else {
                fitted
            };
        }
        let pars = field(obj, "pars");
        let control = field(obj, "control");
        let y = numeric(field(obj, "y"));
        let weights = numeric(field(obj, "weights"));
        let matrix = field(obj, "x");
        let dim = attribute(matrix, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 2 {
            base_error("invalid LOESS model matrix");
        }
        let dims = wrapped(dim);
        let dim0 = dims
            .try_integer_elt(0)
            .unwrap_or_else(|error| r_error(error.to_string()));
        let dim1 = dims
            .try_integer_elt(1)
            .unwrap_or_else(|error| r_error(error.to_string()));
        if dim0 < 1 || !(1..=4).contains(&dim1) {
            base_error("invalid LOESS model matrix");
        }
        let d = dim1 as usize;
        let n = y.len();
        let values = numeric(matrix);
        if n == 0 || dim0 as usize != n || values.len() != n * d || weights.len() != n {
            base_error("invalid LOESS model dimensions");
        }
        let original: Vec<Vec<_>> = (0..n)
            .map(|i| (0..d).map(|j| values[i + j * n]).collect())
            .collect();
        let divisor = numeric(field(obj, "divisor"));
        if divisor.len() != d || divisor.iter().any(|x| !x.is_finite() || *x <= 0.) {
            base_error("invalid LOESS normalization");
        }
        let x = original
            .iter()
            .map(|r| r.iter().zip(&divisor).map(|(v, s)| v / s).collect())
            .collect();
        let config = Config {
            span: scalar(field(pars, "span")),
            degree: scalar(field(pars, "degree")) as usize,
            normalize: scalar(field(pars, "normalize")) != 0.,
            parametric: flags(field(pars, "parametric"), d),
            drop_square: flags(field(pars, "drop.square"), d),
            interpolate: string(field(pars, "surface")) == "interpolate",
            cell: scalar(field(pars, "cell")),
            iterations: scalar(field(pars, "iterations")) as usize,
            exact: string(field(control, "statistics")) == "exact",
            approximate_trace: false,
        };
        let model = Model {
            x,
            y,
            weights,
            divisor,
            config,
            fitted: numeric(field(obj, "fitted")),
            training_diagonal: None,
            robust: numeric(field(obj, "robust")),
            trace: scalar(field(obj, "trace.hat")),
            delta1: scalar(field(obj, "one.delta")),
            delta2: scalar(field(obj, "two.delta")),
            s: scalar(field(obj, "s")),
        };
        if model.robust.len() != n
            || model.fitted.len() != n
            || model.config.degree > 2
            || model.x.iter().flatten().any(|v| !v.is_finite())
        {
            base_error("invalid LOESS model");
        }
        let queries = if a[1] == R_NilValue() {
            original
        } else if TYPEOF(a[1]) == SEXPTYPE::REALSXP || TYPEOF(a[1]) == SEXPTYPE::INTSXP {
            let v = numeric(a[1]);
            if d == 1 {
                v.into_iter().map(|x| vec![x]).collect()
            } else {
                let dim = attribute(a[1], R_DimSymbol());
                if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 2 {
                    base_error("newdata must have the same predictor columns");
                }
                let dims = wrapped(dim);
                let dim0 = dims
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                let dim1 = dims
                    .try_integer_elt(1)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                if dim0 < 0 || dim1 as usize != d {
                    base_error("newdata must have the same predictor columns");
                }
                let nr = dim0 as usize;
                if nr.checked_mul(d) != Some(v.len()) {
                    base_error("invalid newdata matrix dimensions");
                }
                (0..nr)
                    .map(|i| (0..d).map(|j| v[i + j * nr]).collect())
                    .collect()
            }
        } else {
            let env = data_environment(a[1], rho);
            let _env = protect(env);
            let mut expressions = vec![];
            let f = field(obj, "formula");
            if TYPEOF(f) != SEXPTYPE::LANGSXP || Rf_length(f) != 3 {
                base_error("invalid LOESS formula");
            }
            predictors(CAR(CDR(CDR(f))), &mut expressions);
            let cols = predictor_columns(&expressions, env);
            if cols.len() != d {
                base_error("newdata must have the same predictor columns");
            }
            let nr = cols[0].len();
            if cols.iter().any(|c| c.len() != nr) {
                base_error("newdata predictor lengths differ");
            }
            (0..nr)
                .map(|i| cols.iter().map(|c| c[i]).collect())
                .collect()
        };
        let se = scalar(a[2]) != 0.;
        let (fit, error) = model
            .predict_with_execution(&queries, se, &Execution::new(&check_execution))
            .unwrap_or_else(|e| base_error(e));
        if se {
            rlist!("fit"=>numbers(&fit),"se.fit"=>numbers(&error),"residual.scale"=>Rf_ScalarReal(model.s),"df"=>Rf_ScalarReal(model.delta1*model.delta1/model.delta2))
        } else {
            let action = field(obj, "na.action");
            if a[1] == R_NilValue() && is_exclude_action(action) {
                restore_excluded_fit(&fit, action)
            } else {
                numbers(&fit)
            }
        }
    }
}

fn omission_action(
    data: SEXP,
    response: SEXP,
    selected: &[usize],
    complete: &[usize],
    action_name: &str,
) -> SEXP {
    {
        if complete.len() == selected.len() || action_name == "na.fail" {
            return R_NilValue();
        }
        let complete_set: HashSet<usize> = complete.iter().copied().collect();
        let dropped: Vec<(usize, usize)> = selected
            .iter()
            .enumerate()
            .filter(|(_, index)| !complete_set.contains(index))
            .map(|(position, index)| (position, *index))
            .collect();
        if dropped.is_empty() {
            return R_NilValue();
        }
        let action = alloc_vector(SEXPTYPE::INTSXP, dropped.len() as R_xlen_t);
        let _action = protect(action);
        let names = alloc_vector(SEXPTYPE::STRSXP, dropped.len() as R_xlen_t);
        let _names = protect(names);
        let mut action_mut = mutable_vector(action);
        let mut names_mut = mutable_vector(names);
        for (out, (position, original)) in dropped.iter().enumerate() {
            action_mut
                .try_set_integer_elt(out as R_xlen_t, (*position + 1) as i32)
                .unwrap_or_else(|error| r_error(error.to_string()));
            let label = row_label(data, response, *original);
            names_mut
                .try_set_string_elt(out as R_xlen_t, wrapped(mk_char(&label)))
                .unwrap_or_else(|error| r_error(error.to_string()));
        }
        set_names(action, names);
        let class_name = if action_name == "na.exclude" {
            "exclude"
        } else {
            "omit"
        };
        set_attribute(action, R_ClassSymbol(), mk_string(class_name));
        action
    }
}

fn row_label(data: SEXP, response: SEXP, index: usize) -> String {
    {
        let row_names = attribute(data, crate::sexp::attrib_core::R_RowNamesSymbol());
        if TYPEOF(row_names) == SEXPTYPE::STRSXP && index < XLENGTH(row_names) as usize {
            return elt_to_string(row_names, index as i64);
        }
        let names = attribute(response, R_NamesSymbol());
        if TYPEOF(names) == SEXPTYPE::STRSXP && index < XLENGTH(names) as usize {
            return elt_to_string(names, index as i64);
        }
        if TYPEOF(row_names) == SEXPTYPE::INTSXP {
            let rows = wrapped(row_names);
            if XLENGTH(row_names) == 2 {
                let first = rows
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                let second = rows
                    .try_integer_elt(1)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                if first == i32::MIN && second < 0 {
                    return (index + 1).to_string();
                }
            }
            if index < XLENGTH(row_names) as usize {
                let value = rows
                    .try_integer_elt(index as R_xlen_t)
                    .unwrap_or_else(|error| r_error(error.to_string()));
                if value != NA_INTEGER {
                    return value.to_string();
                }
            }
        }
        (index + 1).to_string()
    }
}

fn is_exclude_action(action: SEXP) -> bool {
    let class = attribute(action, R_ClassSymbol());
    TYPEOF(class) == SEXPTYPE::STRSXP && XLENGTH(class) > 0 && elt_to_string(class, 0) == "exclude"
}

fn restore_excluded_fit(fit: &[f64], action: SEXP) -> SEXP {
    {
        if TYPEOF(action) != SEXPTYPE::INTSXP {
            base_error("invalid LOESS omission metadata");
        }
        let omitted_i64 = XLENGTH(action);
        let omitted = usize::try_from(omitted_i64)
            .ok()
            .filter(|_| omitted_i64 >= 0)
            .unwrap_or_else(|| base_error("invalid LOESS omission metadata"));
        let total = fit
            .len()
            .checked_add(omitted)
            .filter(|length| i64::try_from(*length).is_ok())
            .unwrap_or_else(|| base_error("invalid LOESS omission metadata"));
        let mut seen = HashSet::with_capacity(omitted);
        let action_sexp = wrapped(action);
        for i in 0..omitted_i64 {
            let position = action_sexp
                .try_integer_elt(i)
                .unwrap_or_else(|error| r_error(error.to_string()));
            if position <= 0 || position as usize > total || !seen.insert(position as usize) {
                base_error("invalid LOESS omission metadata");
            }
        }
        let restored = alloc_vector(SEXPTYPE::REALSXP, total as R_xlen_t);
        let _restored = protect(restored);
        let mut restored_mut = mutable_vector(restored);
        for i in 0..total {
            restored_mut
                .try_set_real_elt(i as R_xlen_t, NA_REAL)
                .unwrap_or_else(|error| r_error(error.to_string()));
        }
        let mut source = 0usize;
        for position in 1..=total {
            let excluded = seen.contains(&position);
            if !excluded {
                restored_mut
                    .try_set_real_elt((position - 1) as R_xlen_t, fit[source])
                    .unwrap_or_else(|error| r_error(error.to_string()));
                source += 1;
            }
        }
        restored
    }
}

/// GNU `loess.smooth(x, y, span, degree, evaluation)` — interpolate on a grid.
pub fn do_loess_smooth(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let x = CAR(args);
        let y = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x).min(XLENGTH(y)) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let mut span = 2.0 / 3.0;
        let mut degree: usize = 1;
        let mut evaluation: usize = 50;
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                symbol_name(tag)
            } else {
                String::new()
            };
            let v = CAR(cell);
            let first_real = |v: SEXP| {
                wrapped(v)
                    .try_real_elt(0)
                    .unwrap_or_else(|error| r_error(error.to_string()))
            };
            let first_int = |v: SEXP| {
                wrapped(v)
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| r_error(error.to_string()))
            };
            if name == "span" {
                let s = if TYPEOF(v) == SEXPTYPE::REALSXP {
                    first_real(v)
                } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                    first_int(v) as f64
                } else {
                    span
                };
                if s.is_finite() && s > 0.0 {
                    span = s;
                }
            } else if name == "degree" {
                let d = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    first_int(v) as usize
                } else {
                    first_real(v) as usize
                };
                if d <= 2 {
                    degree = d;
                }
            } else if name == "evaluation" {
                let e = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    first_int(v) as usize
                } else {
                    first_real(v) as usize
                };
                if e >= 2 {
                    evaluation = e;
                }
            }
            cell = CDR(cell);
        }
        let x_sexp = wrapped(x);
        let y_sexp = wrapped(y);
        let mut xs = Vec::with_capacity(n);
        let mut ys = Vec::with_capacity(n);
        for i in 0..n {
            let xv = if TYPEOF(x) == SEXPTYPE::REALSXP {
                x_sexp
                    .try_real_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| r_error(error.to_string()))
            } else {
                x_sexp
                    .try_integer_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| r_error(error.to_string())) as f64
            };
            let yv = if TYPEOF(y) == SEXPTYPE::REALSXP {
                y_sexp
                    .try_real_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| r_error(error.to_string()))
            } else {
                y_sexp
                    .try_integer_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| r_error(error.to_string())) as f64
            };
            if xv.is_finite() && yv.is_finite() {
                xs.push(xv);
                ys.push(yv);
            }
        }
        if xs.len() < 2 {
            return R_NilValue();
        }
        let xmin = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let xmax = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let rows: Vec<Vec<f64>> = xs.iter().map(|v| vec![*v]).collect();
        let weights = vec![1.0; xs.len()];
        let config = Config {
            span,
            degree,
            normalize: false,
            parametric: vec![false],
            drop_square: vec![false],
            interpolate: true,
            cell: 0.2,
            iterations: 1,
            exact: false,
            approximate_trace: false,
        };
        let model = match Model::fit_with_execution(
            rows,
            ys,
            weights,
            config,
            &Execution::new(&check_execution),
        ) {
            Ok(m) => m,
            Err(_) => return R_NilValue(),
        };
        let mut grid = Vec::with_capacity(evaluation);
        let mut queries = Vec::with_capacity(evaluation);
        for i in 0..evaluation {
            let t = if evaluation == 1 {
                xmin
            } else {
                xmin + (xmax - xmin) * (i as f64) / ((evaluation - 1) as f64)
            };
            grid.push(t);
            queries.push(vec![t]);
        }
        let pred = match model.predict_with_execution(
            &queries,
            false,
            &Execution::new(&check_execution),
        ) {
            Ok((fit, _)) => fit,
            Err(_) => return R_NilValue(),
        };
        let xout = numbers(&grid);
        let _xo = protect(xout);
        let yout = numbers(
            &(0..evaluation)
                .map(|i| pred.get(i).copied().unwrap_or(f64::NAN))
                .collect::<Vec<_>>(),
        );
        let _yo = protect(yout);
        let result = alloc_vector(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        let mut result_mut = mutable_vector(result);
        result_mut
            .try_set_vector_elt(0, wrapped(xout))
            .unwrap_or_else(|error| r_error(error.to_string()));
        result_mut
            .try_set_vector_elt(1, wrapped(yout))
            .unwrap_or_else(|error| r_error(error.to_string()));
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        result
    }
}

#[cfg(test)]
mod predictor_flag_tests {
    use crate::{mainutils::paths::RuntimePathPolicy, sexp::session::RSession};

    // Independently verified with the pinned GNU R 4.7 oracle. The public
    // arguments select predictor names through ordinary vector subscripting.
    const PUBLIC_SELECTIONS: &str = r"
        x <- seq(0, 1, length.out=40); z <- cos(7*x); y <- sin(5*x)+z
        ctl <- loess.control(surface='direct', statistics='none')
        cases <- list(NA, NA_integer_, NaN, Inf, NULL, FALSE,
                      c(TRUE,FALSE,TRUE), 1, 2.9, -1, 'z', 'absent')
        expected <- list(c(FALSE,FALSE),c(FALSE,FALSE),c(FALSE,FALSE),
                         c(FALSE,FALSE),c(FALSE,FALSE),c(FALSE,FALSE),
                         c(TRUE,FALSE),c(TRUE,FALSE),c(FALSE,TRUE),
                         c(FALSE,TRUE),c(FALSE,TRUE),c(FALSE,FALSE))
        for (i in seq_along(cases)) {
            f <- loess(y~x+z, parametric=cases[[i]], control=ctl)
            g <- loess(y~x+z, drop.square=cases[[i]], control=ctl)
            stopifnot(identical(as.logical(f$pars$parametric),expected[[i]]),
                      identical(as.logical(g$pars$drop.square),expected[[i]]),
                      all(is.finite(f$fitted)),all(is.finite(g$fitted)))
        }
        stopifnot(inherits(tryCatch(loess(y~x+z,parametric=TRUE,control=ctl),error=function(e)e),'error'),
                  inherits(tryCatch(loess(y~x+z,parametric=c(-1,NA),control=ctl),error=function(e)e),'error'))
        X <- cbind(u=x,v=z)
        f <- loess(y~X,parametric='X.v',drop.square='X.u',control=ctl)
        stopifnot(identical(as.logical(f$pars$parametric),c(FALSE,TRUE)),
                  identical(as.logical(f$pars$drop.square),c(TRUE,FALSE)))
        TRUE
    ";

    #[test]
    fn loess_predictor_flag_selection_matches_gnu_under_both_package_policies() {
        for policy in ["native", "portable", "builtin"] {
            let mut session = match policy {
                "portable" => {
                    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
                }
                "builtin" => RSession::new_without_default_packages(),
                _ => RSession::new(),
            };
            let (value, output, _) = session.eval_code_with_output_capture(PUBLIC_SELECTIONS);
            assert_eq!(
                value
                    .unwrap_or_else(|error| panic!(
                        "policy={policy}: {}: {output:?}",
                        error.message
                    ))
                    .logical_elt(0),
                Some(1),
                "policy={policy}: {output:?}"
            );
        }
    }
}
