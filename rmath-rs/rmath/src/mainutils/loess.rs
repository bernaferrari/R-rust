//! R-facing adapters for the owned, pointer-free LOESS numerical engine.
use crate::eval::attrib_core::{R_ClassSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::library::stats::loess::{Config, Execution, Model};
use crate::mainutils::essentials::{base_error, elt_to_string};
use crate::sexp::{
    accessors::*,
    constructors::*,
    ffi::{NA_INTEGER, NA_REAL, SEXP, SEXPTYPE},
    globals::R_NilValue,
    protect::{ProtectGuard, protect},
    symbol::Rf_install,
};
use std::collections::HashSet;

fn check_execution() -> Result<(), String> {
    if crate::sexp::instance::is_cancellation_requested() {
        Err("operation cancelled".into())
    } else {
        Ok(())
    }
}

pub(crate) unsafe fn do_loess(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply("loess", include_str!("loess.R"), args, rho, false)
    }
}
pub(crate) unsafe fn do_control(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
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
}
pub(crate) unsafe fn do_predict(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "predict",
            "function(object,...) UseMethod('predict')",
            args,
            rho,
            false,
        )
    }
}
pub(crate) unsafe fn do_predict_loess(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "predict.loess",
            "function(object,newdata=NULL,se=FALSE,na.action=na.pass,...) .rport_loess_predict(object,newdata,se)",
            args,
            rho,
            false,
        )
    }
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
    unsafe fn push(&mut self, key: &'static str, value: SEXP) {
        unsafe {
            self.roots.push(protect(value));
            self.fields.push((key, value));
        }
    }
    unsafe fn finish(self) -> SEXP {
        unsafe {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, self.fields.len() as i64);
            let _root = protect(result);
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, self.fields.len() as i64);
            let _names = protect(names);
            for (i, (key, value)) in self.fields.iter().enumerate() {
                SET_VECTOR_ELT(result, i as i64, *value);
                SET_STRING_ELT(
                    names,
                    i as i64,
                    Rf_mkChar(std::ffi::CString::new(*key).unwrap().as_ptr()),
                );
            }
            setAttrib(result, R_NamesSymbol(), names);
            result
        }
    }
}
macro_rules! rlist {($($key:literal => $value:expr),* $(,)?)=>{{let mut list=ListBuilder::new();$(list.push($key,$value);)*list.finish()}};}
unsafe fn numbers(values: &[f64]) -> SEXP {
    unsafe {
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as i64);
        for (i, v) in values.iter().enumerate() {
            *REAL(x).add(i) = *v;
        }
        x
    }
}
unsafe fn numeric(x: SEXP) -> Vec<f64> {
    unsafe {
        if x == R_NilValue() {
            return vec![];
        }
        if !matches!(
            SEXPTYPE(TYPEOF(x)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) {
            base_error("LOESS predictors, responses and weights must be numeric");
        }
        (0..XLENGTH(x))
            .map(|i| match SEXPTYPE(TYPEOF(x)) {
                SEXPTYPE::REALSXP => *REAL(x).add(i as usize),
                SEXPTYPE::INTSXP => {
                    let v = *INTEGER(x).add(i as usize);
                    if v == i32::MIN { f64::NAN } else { v as f64 }
                }
                _ => {
                    let v = *LOGICAL(x).add(i as usize);
                    if v == i32::MIN { f64::NAN } else { v as f64 }
                }
            })
            .collect()
    }
}
unsafe fn field(x: SEXP, name: &str) -> SEXP {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = getAttrib(x, R_NamesSymbol());
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) != XLENGTH(x) {
            return R_NilValue();
        }
        for i in 0..XLENGTH(x) {
            if elt_to_string(names, i) == name {
                return VECTOR_ELT(x, i);
            }
        }
        R_NilValue()
    }
}
unsafe fn scalar(x: SEXP) -> f64 {
    unsafe {
        let v = numeric(x);
        if v.len() != 1 {
            base_error("expected a scalar LOESS parameter");
        }
        v[0]
    }
}
unsafe fn string(x: SEXP) -> String {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::STRSXP || XLENGTH(x) != 1 {
            base_error("expected a character LOESS parameter");
        }
        elt_to_string(x, 0)
    }
}
unsafe fn positional(args: SEXP) -> Vec<SEXP> {
    unsafe {
        let mut result = vec![];
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            result.push(CAR(p));
            p = CDR(p);
        }
        result
    }
}
unsafe fn predictors(expr: SEXP, out: &mut Vec<SEXP>) {
    unsafe {
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && TYPEOF(CAR(expr)) == SEXPTYPE::SYMSXP {
            let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(CAR(expr)))).to_string_lossy();
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
}
unsafe fn data_environment(data: SEXP, parent: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(data) == SEXPTYPE::ENVSXP {
            return data;
        }
        if data != R_NilValue() && TYPEOF(data) != SEXPTYPE::VECSXP {
            base_error("'data' must be a data frame, list or environment");
        }
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
        let _env = protect(env);
        let names = getAttrib(data, R_NamesSymbol());
        if data != R_NilValue() {
            if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) != XLENGTH(data) {
                base_error("LOESS data columns must be named");
            }
            for i in 0..XLENGTH(data) {
                let name = std::ffi::CString::new(elt_to_string(names, i)).unwrap();
                crate::sexp::envir::defineVar(Rf_install(name.as_ptr()), VECTOR_ELT(data, i), env);
            }
        }
        env
    }
}
unsafe fn predictor_columns(expressions: &[SEXP], env: SEXP) -> Vec<Vec<f64>> {
    unsafe {
        let mut columns = vec![];
        for expr in expressions {
            let value = crate::eval::eval::Rf_eval(*expr, env);
            let values = numeric(value);
            let dim = getAttrib(value, R_DimSymbol());
            if dim == R_NilValue() {
                columns.push(values);
                continue;
            }
            if TYPEOF(dim) != SEXPTYPE::INTSXP
                || XLENGTH(dim) != 2
                || *INTEGER(dim) < 0
                || *INTEGER(dim).add(1) < 1
            {
                base_error("invalid LOESS predictor matrix");
            }
            let n = *INTEGER(dim) as usize;
            let d = *INTEGER(dim).add(1) as usize;
            if n.checked_mul(d) != Some(values.len()) {
                base_error("invalid LOESS predictor dimensions");
            }
            for j in 0..d {
                columns.push(values[j * n..(j + 1) * n].to_vec());
            }
        }
        columns
    }
}
unsafe fn flags(x: SEXP, d: usize) -> Vec<bool> {
    unsafe {
        let v = numeric(x);
        if v.iter().any(|x| !x.is_finite()) || (v.len() != 1 && v.len() != d) {
            base_error("wrong length for LOESS predictor flags");
        }
        v.iter().cycle().take(d).map(|x| *x != 0.).collect()
    }
}

pub(crate) unsafe fn do_fit(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let a = positional(args);
        if a.len() != 15 {
            base_error("invalid LOESS internal arguments");
        }
        let formula = a[0];
        if TYPEOF(formula) != SEXPTYPE::LANGSXP || Rf_length(formula) != 3 {
            base_error("invalid LOESS formula");
        }
        let parent = getAttrib(formula, Rf_install(c".Environment".as_ptr()));
        let parent = if parent == R_NilValue() { rho } else { parent };
        let env = data_environment(a[1], parent);
        let _env = protect(env);
        let mut expressions = vec![];
        predictors(CAR(CDR(CDR(formula))), &mut expressions);
        let eval = |expr| crate::eval::eval::Rf_eval(expr, env);
        let y_value = eval(CADR(formula));
        let _y_value = protect(y_value);
        let y = numeric(y_value);
        let n = y.len();
        let cols = predictor_columns(&expressions, env);
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
        let drops = flags(a[8], d);
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
            parametric: flags(a[7], d),
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
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(a[14])))
                .to_string_lossy()
                .into_owned()
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
        let mut model = Model::fit_with_execution(
            x.clone(),
            y.clone(),
            weights.clone(),
            config,
            &Execution::new(&check_execution),
        )
        .unwrap_or_else(|e| base_error(e));
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
            let frame = Rf_allocVector3(SEXPTYPE::VECSXP, (expressions.len() + 1) as i64);
            let _frame = protect(frame);
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, (expressions.len() + 1) as i64);
            let _names = protect(names);
            for (j, expr) in std::iter::once(&CADR(formula))
                .chain(expressions.iter())
                .enumerate()
            {
                let value = eval(*expr);
                let _value = protect(value);
                // Preserve matrix-valued predictors in the model frame.
                let dim = getAttrib(value, R_DimSymbol());
                let nc = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
                    *INTEGER(dim).add(1) as usize
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
                    let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                    *INTEGER(dims) = complete.len() as i32;
                    *INTEGER(dims).add(1) = nc as i32;
                    setAttrib(filtered, R_DimSymbol(), dims);
                }
                SET_VECTOR_ELT(frame, j as i64, filtered);
                let label = crate::mainutils::deparse::deparse1line(*expr, false);
                SET_STRING_ELT(names, j as i64, STRING_ELT(label, 0));
            }
            setAttrib(frame, R_NamesSymbol(), names);
            setAttrib(frame, R_ClassSymbol(), Rf_mkString(c"data.frame".as_ptr()));
            let rows = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(rows) = i32::MIN;
            *INTEGER(rows).add(1) = -(complete.len() as i32);
            setAttrib(frame, Rf_install(c"row.names".as_ptr()), rows);
            frame
        } else {
            R_NilValue()
        };
        let _frame = protect(frame);
        let result = model_to_r(&model, &x, formula, control, a[13], frame, na_action);

        result
    }
}

unsafe fn model_to_r(
    m: &Model,
    x: &[Vec<f64>],
    formula: SEXP,
    control: SEXP,
    call: SEXP,
    frame: SEXP,
    na_action: SEXP,
) -> SEXP {
    unsafe {
        let n = m.y.len();
        let d = m.divisor.len();
        let matrix = numbers(
            &(0..d)
                .flat_map(|j| x.iter().map(move |row| row[j]))
                .collect::<Vec<_>>(),
        );
        let _matrix = protect(matrix);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = n as i32;
        *INTEGER(dim).add(1) = d as i32;
        setAttrib(matrix, R_DimSymbol(), dim);
        let pars = rlist!("span"=>Rf_ScalarReal(m.config.span),"degree"=>Rf_ScalarInteger(m.config.degree as i32),"normalize"=>Rf_ScalarLogical(i32::from(m.config.normalize)),"parametric"=>numbers(&m.config.parametric.iter().map(|x|f64::from(*x)).collect::<Vec<_>>()),"drop.square"=>numbers(&m.config.drop_square.iter().map(|x|f64::from(*x)).collect::<Vec<_>>()),"surface"=>Rf_mkString(if m.config.interpolate{c"interpolate"}else{c"direct"}.as_ptr()),"cell"=>Rf_ScalarReal(m.config.cell),"family"=>Rf_mkString(if m.config.iterations>1{c"symmetric"}else{c"gaussian"}.as_ptr()),"iterations"=>Rf_ScalarInteger(m.config.iterations as i32));
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
        setAttrib(result, R_ClassSymbol(), Rf_mkString(c"loess".as_ptr()));
        result
    }
}

pub(crate) unsafe fn do_predict_core(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let a = positional(args);
        if a.len() != 3 {
            base_error("invalid LOESS prediction arguments");
        }
        let obj = a[0];
        let pars = field(obj, "pars");
        let control = field(obj, "control");
        let y = numeric(field(obj, "y"));
        let weights = numeric(field(obj, "weights"));
        let matrix = field(obj, "x");
        let dim = getAttrib(matrix, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) != 2
            || *INTEGER(dim) < 1
            || *INTEGER(dim).add(1) < 1
            || *INTEGER(dim).add(1) > 4
        {
            base_error("invalid LOESS model matrix");
        }
        let d = *INTEGER(dim).add(1) as usize;
        let n = y.len();
        let values = numeric(matrix);
        if n == 0 || *INTEGER(dim) as usize != n || values.len() != n * d || weights.len() != n {
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
                let dim = getAttrib(a[1], R_DimSymbol());
                if TYPEOF(dim) != SEXPTYPE::INTSXP
                    || XLENGTH(dim) != 2
                    || *INTEGER(dim) < 0
                    || *INTEGER(dim).add(1) as usize != d
                {
                    base_error("newdata must have the same predictor columns");
                }
                let nr = *INTEGER(dim) as usize;
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

unsafe fn omission_action(
    data: SEXP,
    response: SEXP,
    selected: &[usize],
    complete: &[usize],
    action_name: &str,
) -> SEXP {
    unsafe {
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
        let action = Rf_allocVector3(SEXPTYPE::INTSXP, dropped.len() as i64);
        let _action = protect(action);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, dropped.len() as i64);
        let _names = protect(names);
        for (out, (position, original)) in dropped.iter().enumerate() {
            *INTEGER(action).add(out) = (*position + 1) as i32;
            let label = row_label(data, response, *original);
            let label = std::ffi::CString::new(label).unwrap_or_default();
            SET_STRING_ELT(names, out as i64, Rf_mkChar(label.as_ptr()));
        }
        setAttrib(action, R_NamesSymbol(), names);
        let class_name = if action_name == "na.exclude" {
            "exclude"
        } else {
            "omit"
        };
        let class = Rf_mkString(
            std::ffi::CString::new(class_name)
                .unwrap_or_default()
                .as_ptr(),
        );
        setAttrib(action, R_ClassSymbol(), class);
        action
    }
}

unsafe fn row_label(data: SEXP, response: SEXP, index: usize) -> String {
    unsafe {
        let row_names = getAttrib(data, crate::eval::attrib_core::R_RowNamesSymbol());
        if TYPEOF(row_names) == SEXPTYPE::STRSXP && index < XLENGTH(row_names) as usize {
            return elt_to_string(row_names, index as i64);
        }
        let names = getAttrib(response, R_NamesSymbol());
        if TYPEOF(names) == SEXPTYPE::STRSXP && index < XLENGTH(names) as usize {
            return elt_to_string(names, index as i64);
        }
        if TYPEOF(row_names) == SEXPTYPE::INTSXP && XLENGTH(row_names) == 2 {
            let first = *INTEGER(row_names);
            let second = *INTEGER(row_names).add(1);
            if first == i32::MIN && second < 0 {
                return (index + 1).to_string();
            }
        }
        if TYPEOF(row_names) == SEXPTYPE::INTSXP && index < XLENGTH(row_names) as usize {
            let value = *INTEGER(row_names).add(index);
            if value != NA_INTEGER {
                return value.to_string();
            }
        }
        (index + 1).to_string()
    }
}

unsafe fn is_exclude_action(action: SEXP) -> bool {
    unsafe {
        let class = getAttrib(action, R_ClassSymbol());
        TYPEOF(class) == SEXPTYPE::STRSXP
            && XLENGTH(class) > 0
            && elt_to_string(class, 0) == "exclude"
    }
}

unsafe fn restore_excluded_fit(fit: &[f64], action: SEXP) -> SEXP {
    unsafe {
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
        for i in 0..omitted_i64 {
            let position = *INTEGER(action).add(i as usize);
            if position <= 0 || position as usize > total || !seen.insert(position as usize) {
                base_error("invalid LOESS omission metadata");
            }
        }
        let restored = Rf_allocVector3(SEXPTYPE::REALSXP, total as i64);
        let _restored = protect(restored);
        for i in 0..total {
            *REAL(restored).add(i) = NA_REAL;
        }
        let mut source = 0usize;
        for position in 1..=total {
            let excluded = seen.contains(&position);
            if !excluded {
                *REAL(restored).add(position - 1) = fit[source];
                source += 1;
            }
        }
        restored
    }
}
