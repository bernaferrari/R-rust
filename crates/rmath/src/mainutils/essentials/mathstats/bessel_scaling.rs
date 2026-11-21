//! Rooted public `1 + as.logical(expon.scaled)` evaluation.
use crate::mainutils::essentials::install_cstr;
use crate::sexp::{
    object::{Sexp, SexpResult},
    owner::OwnerToken,
};
pub(super) fn prepare(
    owner: OwnerToken<'_>,
    input: Option<Sexp<'static>>,
    environment: Sexp<'static>,
) -> SexpResult<Sexp<'static>> {
    let root = |raw| owner.sexp(raw).and_then(Sexp::into_owned);
    let input = match input {
        Some(x) => x,
        None => root(crate::sexp::globals::R_False())?,
    };
    let base = root(crate::sexp::globals::R_BaseEnv())?;
    let function = |name: &std::ffi::CStr| -> SexpResult<Sexp<'static>> {
        owner.require_active()?;
        let symbol = root(install_cstr(&name))?;
        owner.require_active()?;
        let raw = unsafe { crate::sexp::envir::findFun(symbol.as_raw(), base.as_raw()) };
        owner.require_active()?;
        root(raw)
    };
    let quote = function(c"quote")?;
    let logical = function(c"as.logical")?;
    let plus = function(c"+")?;
    // Ordinary constructors retain inputs, retry one full GC at the same
    // cap, and preserve the canonical resource-limit RError payload.
    let language2 = |function: &Sexp<'_>, argument: &Sexp<'_>| -> SexpResult<Sexp<'static>> {
        owner.require_active()?;
        let raw = crate::sexp::constructors::Rf_lang2(function.as_raw(), argument.as_raw());
        owner.require_active()?;
        root(raw)
    };
    let quoted = language2(&quote, &input)?;
    let logical = language2(&logical, &quoted)?;
    let raw = crate::sexp::constructors::Rf_ScalarReal(1.0);
    owner.require_active()?;
    let one = root(raw)?;
    let raw = crate::sexp::constructors::Rf_lang3(plus.as_raw(), one.as_raw(), logical.as_raw());
    owner.require_active()?;
    let expression = root(raw)?;
    let raw = unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), environment.as_raw()) };
    owner.require_active()?;
    root(raw)
}
