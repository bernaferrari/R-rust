#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

//! Port of R's src/main/dotcode.c — foreign function interface for .Call, .C, .Fortran, .External.
//!
//! Provides the dispatch machinery for calling native C/Fortran routines from R code.
//! Faithfully ports the argument marshaling, symbol resolution, and error checking from
//! the C implementation while using idiomatic Rust patterns to collapse the repetitive
//! function-pointer typedefs and switch dispatches.

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::mainutils::memory_main::{R_ExternalPtrAddr, sexptype2char};
use crate::mainutils::rdynload::{
    R_FindSymbol as R_lookupLoadedSymbol, R_dlsym, R_findDllByHandle,
};
use crate::mainutils::relop::PRIMVAL;
use crate::sexp::accessors::{
    CAR, CDR, COMPLEX, INTEGER, LENGTH, PRINTNAME, RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT,
    SETCDR, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH, translateChar,
};
use crate::sexp::attrib_core::{getAttrib, setAttrib};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_length, Rf_mkChar};
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::memory_ext::{R_alloc, vmaxget, vmaxset};
use crate::unix::dynload::DL_FUNC;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum entry-point name length including nul terminator.
const MAX_SYMBOL_BYTES: usize = 1024;

/// Maximum number of arguments to .C, .Fortran and .Call.
const MAX_ARGS: usize = 65;

/// Maximum DLL / path name length.
const R_PATH_MAX: usize = 4096;

/// Guard byte pattern and count for bounds checking in .C/.Fortran.
const FILL: u8 = 0xee;
const NG: usize = 64;

#[derive(Default)]
pub(crate) struct DotcodeRuntimeState {
    pub retval_check: Option<bool>,
}

// ---------------------------------------------------------------------------
// Native symbol type constants
// ---------------------------------------------------------------------------

const R_ANY_SYM: c_int = -1;
const R_C_SYM: c_int = 1;
const R_FORTRAN_SYM: c_int = 2;
const R_CALL_SYM: c_int = 3;
const R_EXTERNAL_SYM: c_int = 4;

// ---------------------------------------------------------------------------
// DllReference — identifies which DLL to resolve symbols in
// ---------------------------------------------------------------------------

/// Discriminant for how the DLL was specified.
const NOT_DEFINED: i32 = 0;
const FILENAME: i32 = 1;
const DLL_HANDLE: i32 = 2;
const R_OBJECT: i32 = 3;

struct DllReference {
    dll_name: [u8; R_PATH_MAX],
    dll: *mut c_void,
    obj: SEXP,
    ref_type: i32,
}

impl DllReference {
    fn new() -> Self {
        let mut dll_name = [0u8; R_PATH_MAX];
        dll_name[0] = 0;
        DllReference {
            dll_name,
            dll: ptr::null_mut(),
            obj: ptr::null_mut(),
            ref_type: NOT_DEFINED,
        }
    }
}

// Resolution retains owned declarations and the loaded library, rather than
// retaining pointers into a mutable foreign registration table.
struct R_RegisteredNativeSymbol {
    type_: c_int,
    resolved: Option<crate::mainutils::rdynload::ResolvedForeignRoutine>,
}
impl R_RegisteredNativeSymbol {
    fn new(type_: c_int) -> Self {
        Self {
            type_,
            resolved: None,
        }
    }
    fn validate(&self, count: usize) -> Result<(), String> {
        self.resolved
            .as_ref()
            .map_or(Ok(()), |routine| routine.validate(self.type_, count))
    }
}

// ---------------------------------------------------------------------------
// Local error / warning helpers
// ---------------------------------------------------------------------------

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

fn errorcall(_call: SEXP, msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

/// Adopt a raw SEXP under the active owner for checked element access.
fn checked_sexp(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    crate::mainutils::essentials::owner_sexp(crate::mainutils::essentials::active_owner(), x)
}

fn native_extension_policy_error(call: SEXP, entrypoint: &str) -> ! {
    {
        errorcall(
            call,
            &format!(
                "{entrypoint} calls foreign native code, which cannot exist in the pure-Rust runtime; package authors should use the bundled Rust-ported routines"
            ),
        )
    }
}

fn native_extension_policy_enabled() -> bool {
    !crate::mainutils::rdynload::native_extensions_enabled()
}

fn ported_call_name(op: SEXP) -> Option<String> {
    if TYPEOF(op) == SEXPTYPE::STRSXP && LENGTH(op) >= 1 {
        return checked_sexp(op).try_string_value_elt(0).ok().flatten();
    }
    if TYPEOF(op) == SEXPTYPE::SYMSXP {
        return checked_sexp(op)
            .try_printname()
            .ok()
            .and_then(|name| name.try_as_string().ok());
    }
    if TYPEOF(op) == SEXPTYPE::VECSXP && XLENGTH(op) >= 1 {
        return ported_call_name(VECTOR_ELT(op, 0));
    }
    None
}

fn warning(msg: &str) {
    eprintln!("WARNING: {}", msg);
}

fn warningcall(_call: SEXP, msg: &str) {
    eprintln!("WARNING: {}", msg);
}

fn dotcode_retval_check_enabled() -> bool {
    std::env::var("_R_CHECK_DOTCODE_RETVAL_")
        .map(|p| p == "TRUE" || p == "true" || p == "1" || p == "yes")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Session-local symbols
// ---------------------------------------------------------------------------

fn NaokSymbol() -> SEXP {
    crate::sexp::symbol::install_str("NAOK")
}

fn DupSymbol() -> SEXP {
    crate::sexp::symbol::install_str("DUP")
}

fn PkgSymbol() -> SEXP {
    crate::sexp::symbol::install_str("PACKAGE")
}

fn EncSymbol() -> SEXP {
    crate::sexp::symbol::install_str("ENCODING")
}

fn CSingSymbol() -> SEXP {
    crate::sexp::symbol::install_str("Csingle")
}

// ---------------------------------------------------------------------------
// Helper: isValidString
// ---------------------------------------------------------------------------

/// Check that `s` is a length-1 character string that is not NA.
fn isValidString(s: SEXP) -> bool {
    TYPEOF(s) == SEXPTYPE::STRSXP && LENGTH(s) > 0
}

fn isValidStringF(s: SEXP) -> bool {
    if !isValidString(s) {
        return false;
    }
    let elt = STRING_ELT(s, 0);
    // Check it's not NA_STRING (NA_STRING has type == CHARSXP and is marked)
    !elt.is_null() && TYPEOF(elt) == SEXPTYPE::CHARSXP
}

// ---------------------------------------------------------------------------
// Helper: isNativeSymbolInfo
// ---------------------------------------------------------------------------

/// Structural check that replaces inherits(op, "NativeSymbolInfo").
fn isNativeSymbolInfo(op: SEXP) -> bool {
    TYPEOF(op) == SEXPTYPE::VECSXP
        && LENGTH(op) >= 2
        && TYPEOF(VECTOR_ELT(op, 1)) == SEXPTYPE::EXTPTRSXP
}

// ---------------------------------------------------------------------------
// check1arg2
// ---------------------------------------------------------------------------

fn check1arg2(arg: SEXP, call: SEXP, _formal: &str) {
    {
        if TAG(arg).is_null() || TAG(arg).is_null() {
            // Untagged — fine, it's positional
            return;
        }
        if TAG(arg) == R_NilValue() {
            return;
        }
        errorcall(call, "the first argument should not be named");
    }
}

// ---------------------------------------------------------------------------
// checkValidSymbolId
// ---------------------------------------------------------------------------

/// Validates and resolves a .NAME argument. May set `fun` and `symbol` if
/// the argument is an external pointer or NativeSymbolInfo.
unsafe fn checkValidSymbolId(
    op: SEXP,
    call: SEXP,
    fun: &mut DL_FUNC,
    _symbol: &mut R_RegisteredNativeSymbol,
    buf: *mut u8,
) {
    unsafe {
        if isValidStringF(op) {
            if !buf.is_null() {
                let name = translateChar(STRING_ELT(op, 0));
                let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                if bytes.len() >= MAX_SYMBOL_BYTES {
                    errorcall(call, "symbol name is too long");
                }
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(bytes.len()) = 0;
            }
            return;
        }

        if TYPEOF(op) == SEXPTYPE::EXTPTRSXP {
            let _native_sym = crate::sexp::symbol::install_str("native symbol");
            let _reg_native_sym = crate::sexp::symbol::install_str("registered native symbol");

            if fun.is_none() {
                errorcall(call, "NULL value passed as symbol address");
            }
            return;
        }

        if isNativeSymbolInfo(op) {
            checkValidSymbolId(VECTOR_ELT(op, 1), call, fun, _symbol, buf);
            return;
        }

        errorcall(
            call,
            "first argument must be a string (of length 1) or native symbol reference",
        );
    }
}

// ---------------------------------------------------------------------------
// check_retval
// ---------------------------------------------------------------------------

fn check_retval(call: SEXP, val: SEXP) -> SEXP {
    let do_check = instance::with_required_current_instance(|inst| {
        // SAFETY: short-lived raw place access, no &mut held across calls.
        let slot = unsafe { &mut (*inst).dotcode_state.retval_check }
            .get_or_insert_with(dotcode_retval_check_enabled);
        *slot
    });

    if do_check {
        if (val as usize) < 16 {
            errorcall(call, &format!("WEIRD RETURN VALUE: {:?}", val));
        }
    } else if val.is_null() {
        warningcall(call, "converting NULL pointer to R NULL");
        return R_NilValue();
    }

    val
}

// ---------------------------------------------------------------------------
// do_External — .External and .External2
// ---------------------------------------------------------------------------

/// Actual owning payloads captured before reconstruction or native callbacks.
/// Control arguments select resolution; they never become callable payloads.
struct NativeOperands {
    // Retain the original name graph and any authenticated address child before
    // a control provider can mutate the container or collect detached values.
    name: crate::sexp::object::Sexp<'static>,
    lookup_child: crate::sexp::object::Sexp<'static>,
    symbol_address: Option<crate::sexp::object::Sexp<'static>>,
    payload: Vec<(
        crate::sexp::object::Sexp<'static>,
        crate::sexp::object::Sexp<'static>,
    )>,
    package: Option<(
        String,
        crate::sexp::object::Sexp<'static>,
        crate::sexp::object::Sexp<'static>,
    )>,
}

fn native_admission_error(message: impl Into<String>) -> crate::sexp::object::SexpError {
    crate::sexp::object::SexpError::EvaluationFailed {
        message: message.into(),
    }
}

impl NativeOperands {
    fn capture(
        arguments: crate::sexp::object::Sexp<'static>,
        interface: crate::mainutils::native_routines::NativeInterface,
        access: &crate::sexp::owner::RuntimeAccess,
    ) -> crate::sexp::object::SexpResult<Self> {
        if arguments.is_nil() {
            return Err(native_admission_error("'.NAME' is missing"));
        }
        if !arguments.try_tag()?.is_nil() {
            return Err(native_admission_error(
                "the first argument should not be named",
            ));
        }
        let name = arguments.try_car()?.into_owned()?;
        let mut snapshots = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = arguments.try_cdr()?;
        while !cursor.is_nil() {
            let identity = cursor
                .allocation()?
                .link()
                .ok_or(crate::sexp::object::SexpError::StaleAllocation)?;
            if !seen.insert(identity) {
                return Err(native_admission_error("cyclic native argument list"));
            }
            let value = cursor.try_car()?.into_owned()?;
            let tag = cursor.try_tag()?.into_owned()?;
            snapshots.push((value, tag));
            cursor = cursor.try_cdr()?;
        }
        // Capture every payload/tag and the actual symbol authority child
        // before PACKAGE providers can change the original name container.
        let symbol_address = crate::mainutils::native_symbols::address(&name)?;
        let lookup_child = name.clone();
        access.require_active()?;
        let mut payload = Vec::new();
        let mut package = None;
        // Every source cell and edge has been captured before character/ALTREP
        // access can invoke a provider and detach the original argument graph.
        for (value, tag) in snapshots {
            let control = !tag.is_nil() && tag.try_printname()?.try_as_string()? == "PACKAGE";
            if control {
                if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
                    return Err(native_admission_error(
                        "PACKAGE argument must be a single character string",
                    ));
                }
                let text = value.try_string_elt(0)?.try_as_string()?;
                access.require_active()?;
                if package.is_some() {
                    eprintln!("WARNING: 'PACKAGE' used more than once");
                }
                package = Some((
                    text.strip_prefix("package:").unwrap_or(&text).to_owned(),
                    value,
                    tag,
                ));
            } else {
                if interface == crate::mainutils::native_routines::NativeInterface::Call
                    && payload.len() == MAX_ARGS
                {
                    return Err(native_admission_error(
                        "too many arguments in foreign function call",
                    ));
                }
                payload.push((value, tag));
            }
        }
        Ok(Self {
            name,
            lookup_child,
            symbol_address,
            payload,
            package,
        })
    }

    fn lookup_name(&self) -> crate::sexp::object::SexpResult<Option<String>> {
        let name = &self.lookup_child;
        match name.typeof_() {
            SEXPTYPE::STRSXP if name.len() == 1 => {
                Ok(Some(name.try_string_elt(0)?.try_as_string()?))
            }
            SEXPTYPE::SYMSXP => Ok(Some(name.try_printname()?.try_as_string()?)),
            _ => Ok(None),
        }
    }

    fn argument_list(
        &self,
        allocator: &crate::sexp::object::NodeAllocator<'_, 'static>,
        include_package: bool,
        nil: &crate::sexp::object::Sexp<'static>,
    ) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
        let mut result = nil.clone();
        if include_package {
            if let Some((_, value, tag)) = &self.package {
                result = allocator.pairlist_cell(value, &result, tag)?;
            }
        }
        for (value, tag) in self.payload.iter().rev() {
            result = allocator.pairlist_cell(value, &result, tag)?;
        }
        allocator.pairlist_cell(&self.name, &result, nil)
    }
}

fn lookup_bundled_native(
    name: &str,
    package: Option<&str>,
) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    crate::mainutils::native_symbols::lookup(name, package)
}

/// This translated entry retains original values and execution authority for
/// the full callback, including reconstruction and result validation.
fn invoke_native_handler(
    call: SEXP,
    operator: SEXP,
    arguments: SEXP,
    environment: SEXP,
    interface: crate::mainutils::native_routines::NativeInterface,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    use crate::mainutils::native_routines::NativeInterface;
    use crate::sexp::{
        object::SexpError,
        owner::{OwnerToken, with_runtime},
    };
    OwnerToken::with_current(|owner| {
        let managed = owner.weak_owner().ok_or(SexpError::RootUnavailable)?;
        // Primitive projections may be immortal native table entries. Capture
        // their immutable identity before allocating a local owning operator.
        let operator_identity =
            unsafe { crate::eval::primitive::PrimitiveDescriptor::from_raw(operator) }
                .map(|descriptor| (descriptor.name, descriptor.op.typeof_()));
        with_runtime(&managed, |access| {
            let domain = access.domain();
            let call = domain.wrap(call)?.into_owned()?;
            let environment = domain.wrap(environment)?.into_owned()?;
            let operands =
                NativeOperands::capture(domain.wrap(arguments)?.into_owned()?, interface, access)?;
            access.require_active()?;
            let name = operands.lookup_name()?;
            access.require_active()?;
            let package = operands.package.as_ref().map(|(name, _, _)| name.as_str());
            let handle = operands
                .symbol_address
                .as_ref()
                .map(|address| {
                    access.with_native(|owner| {
                        crate::mainutils::native_symbols::symbol(owner, address)
                    })
                })
                .transpose()?;
            let routine = if let Some(handle) = &handle {
                Some(handle.routine()?)
            } else {
                name.as_deref()
                    .and_then(|name| lookup_bundled_native(name, package))
            };
            // Rejection happens before any argument-list construction, wrapper
            // allocation, or native callback. The pointer descriptor is definitive.
            if let Some(routine) = routine {
                routine
                    .validate_request(interface, operands.payload.len())
                    .map_err(|error| native_admission_error(error.to_string()))?;
            } else {
                // Foreign native code can never exist in the pure-Rust
                // runtime: unregistered calls are unconditionally rejected.
                native_extension_policy_error(call.as_raw(), &interface.to_string());
            }
            let result = if let Some(routine) = routine {
                match interface {
                    NativeInterface::Call => {
                        let pointers: Vec<_> = operands
                            .payload
                            .iter()
                            .map(|(value, _)| value.as_raw())
                            .collect();
                        access.require_active()?;
                        if let Some(handle) = &handle {
                            handle.routine()?;
                        }
                        unsafe { routine.invoke_call(&pointers) }
                            .map_err(|error| native_admission_error(error.to_string()))?
                    }
                    NativeInterface::External | NativeInterface::External2 => {
                        let allocator = access.allocator(&domain)?;
                        let list = operands.argument_list(&allocator, false, &domain.nil())?;
                        if interface == NativeInterface::External {
                            access.require_active()?;
                            if let Some(handle) = &handle {
                                handle.routine()?;
                            }
                            unsafe {
                                routine.invoke_external1(list.as_raw(), operands.payload.len())
                            }
                        } else {
                            let operator = match operator_identity {
                                Some((name, kind)) => access.with_native(|owner| {
                                    let pointer = unsafe {
                                        crate::eval::primitive::make_primitive_binding(name, kind)
                                    };
                                    owner.sexp(pointer)?.into_owned()
                                })?,
                                None => domain.nil(),
                            };
                            access.require_active()?;
                            if let Some(handle) = &handle {
                                handle.routine()?;
                            }
                            unsafe {
                                routine.invoke_external2(
                                    call.as_raw(),
                                    operator.as_raw(),
                                    list.as_raw(),
                                    environment.as_raw(),
                                    operands.payload.len(),
                                )
                            }
                        }
                        .map_err(|error| native_admission_error(error.to_string()))?
                    }
                }
            } else {
                // Unregistered foreign calls are unconditionally rejected
                // above; this branch cannot be reached.
                unreachable!("foreign native code cannot exist in the pure-Rust runtime")
            };
            access.require_active()?;
            let result = check_retval(call.as_raw(), result);
            domain.wrap(result)?.into_owned()
        })?
    })?
}

/// .External / .External2 handler.
pub fn do_External(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let interface = if PRIMVAL(op) == 1 {
        crate::mainutils::native_routines::NativeInterface::External2
    } else {
        crate::mainutils::native_routines::NativeInterface::External
    };
    invoke_native_handler(call, op, args, env, interface)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// .Call handler.
pub fn do_dotcall(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    invoke_native_handler(
        call,
        op,
        args,
        env,
        crate::mainutils::native_routines::NativeInterface::Call,
    )
    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    .as_raw()
}

// ---------------------------------------------------------------------------
// do_dotCode — .C() and .Fortran() handler
// ---------------------------------------------------------------------------

mod buffer_dispatch;

#[cfg(test)]
mod native_inventory;

#[cfg(test)]
mod lookup_snapshot_tests;

pub fn do_dotCode(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    use crate::mainutils::native_routines::buffers::BufferInterface;
    let interface = if PRIMVAL(op) == 0 {
        BufferInterface::C
    } else {
        BufferInterface::Fortran
    };
    unsafe { buffer_dispatch::invoke(call, op, args, env, interface) }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}


// ---------------------------------------------------------------------------
// do_isloaded
// ---------------------------------------------------------------------------

/// Check if a native symbol is available.
pub unsafe fn do_isloaded(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let nargs = Rf_length(args);
        if nargs < 1 {
            error("no arguments supplied");
        }
        if nargs > 3 {
            error("too many arguments");
        }

        if !isValidStringF(CAR(args)) {
            error("invalid 'symbol' argument");
        }

        let sym_name = translateChar(STRING_ELT(CAR(args), 0));

        let pkg_ptr: *const c_char;
        if nargs >= 2 {
            let pkg_arg = CAR(CDR(args));
            if pkg_arg.is_null() || pkg_arg == R_NilValue() {
                pkg_ptr = b"\0".as_ptr() as *const c_char;
            } else {
                if !isValidStringF(pkg_arg) {
                    error("invalid 'PACKAGE' argument");
                }
                pkg_ptr = translateChar(STRING_ELT(pkg_arg, 0));
            }
        } else {
            pkg_ptr = b"\0".as_ptr() as *const c_char;
        }

        let sym_type: c_int;
        if nargs >= 3 {
            let type_arg = CAR(CDR(CDR(args)));
            if type_arg.is_null() || type_arg == R_NilValue() {
                sym_type = R_ANY_SYM;
            } else {
                if !isValidStringF(type_arg) {
                    error("invalid 'type' argument");
                }
                sym_type = match std::ffi::CStr::from_ptr(translateChar(STRING_ELT(type_arg, 0)))
                    .to_str()
                    .unwrap_or("")
                {
                    "" => R_ANY_SYM,
                    "Fortran" => R_FORTRAN_SYM,
                    "Call" => R_CALL_SYM,
                    "External" => R_EXTERNAL_SYM,
                    _ => error("invalid 'type' argument"),
                };
            }
        } else {
            sym_type = R_ANY_SYM;
        }

        let found = R_lookupLoadedSymbol(sym_name, pkg_ptr, sym_type);
        Rf_ScalarLogical(if found.is_some() { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// do_Externalgr / do_dotcallgr — graphics variants
// ---------------------------------------------------------------------------

/// .External.graphics handler — simplified for headless environment.
pub fn do_Externalgr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_External(call, op, args, env)
}

/// .Call.graphics handler — simplified for headless environment.
pub fn do_dotcallgr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_dotcall(call, op, args, env)
}

// ---------------------------------------------------------------------------
// Rf_getCallingDLL / R_FindNativeSymbolFromDLL
// ---------------------------------------------------------------------------

/// Find the DLL that the calling function was loaded from.
pub fn Rf_getCallingDLL() -> SEXP {
    {
        // Stub: return R_NilValue — namespace/DLL tracking not yet implemented
        R_NilValue()
    }
}

/// Find a native symbol from a specific DLL.
fn R_FindNativeSymbolFromDLL(
    name: &[u8],
    dll: &mut DllReference,
    symbol: &mut R_RegisteredNativeSymbol,
    _env: SEXP,
) -> DL_FUNC {
    unsafe {
        if name.is_empty() {
            return None;
        }

        let pkg_ptr = if dll.dll_name[0] == 0 {
            b"\0".as_ptr() as *const c_char
        } else {
            dll.dll_name.as_ptr() as *const c_char
        };

        let looked_up = if dll.ref_type == DLL_HANDLE && !dll.dll.is_null() {
            let loaded = R_findDllByHandle(dll.dll);
            if loaded.is_null() {
                None
            } else {
                R_dlsym(loaded, name.as_ptr() as *const c_char, symbol.type_)
            }
        } else {
            R_lookupLoadedSymbol(name.as_ptr() as *const c_char, pkg_ptr, symbol.type_)
        };

        looked_up
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::RSession;

    #[test]
    fn test_check_valid_symbol_id_copies_name() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let op = crate::sexp::constructors::mk_string_str("registered");
            let mut fun: DL_FUNC = None;
            let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
            let mut buf = [0u8; MAX_SYMBOL_BYTES];

            checkValidSymbolId(op, R_NilValue(), &mut fun, &mut symbol, buf.as_mut_ptr());

            let copied = std::ffi::CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("");
            assert_eq!(copied, "registered");
            assert!(fun.is_none());
        });
    }

    #[test]
    fn test_isloaded_missing_symbol_is_false() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let args = crate::sexp::constructors::Rf_cons(
                crate::sexp::constructors::mk_string_str("missing"),
                R_NilValue(),
            );
            let out = do_isloaded(R_NilValue(), R_NilValue(), args, R_NilValue());
            assert_eq!(*crate::sexp::accessors::INTEGER(out), FALSE as c_int);
        });
    }

    #[test]
    fn test_find_native_symbol_from_dll_empty() {
        let session = RSession::new();
        session.with_protected(|| {
            let mut dll = DllReference::new();
            let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
            let found =
                R_FindNativeSymbolFromDLL(b"missing\0", &mut dll, &mut symbol, R_NilValue());
            assert!(found.is_none());
        });
    }

    #[test]
    fn test_dotcode_symbols_are_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let left_naok = left.with_protected(NaokSymbol);
        let right_naok = right.with_protected(NaokSymbol);
        let left_naok_again = left.with_protected(NaokSymbol);

        assert_eq!(left_naok, left_naok_again);
        assert_ne!(left_naok, right_naok);
    }
}

#[cfg(test)]
mod typed_native_handler_tests {
    use super::*;
    use crate::{
        mainutils::native_routines::NativeInterface,
        sexp::{
            object::{SessionNodeFactory, Sexp},
            session::RSession,
        },
    };
    use std::{cell::Cell, rc::Rc};

    fn make_arguments(
        factory: &SessionNodeFactory<'_>,
        name: &str,
        values: &[(Sexp<'static>, Sexp<'static>)],
    ) -> Sexp<'static> {
        let name = factory.strings(&[name]).unwrap().into_owned().unwrap();
        make_arguments_value(factory, &name, values)
    }

    fn make_arguments_value(
        factory: &SessionNodeFactory<'_>,
        name: &Sexp<'static>,
        values: &[(Sexp<'static>, Sexp<'static>)],
    ) -> Sexp<'static> {
        let mut list = factory.nil().into_owned().unwrap();
        for (value, tag) in values.iter().rev() {
            list = factory
                .pairlist_cell(value, &list, tag)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        factory
            .pairlist_cell(name, &list, &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap()
    }

    fn operator(session: &RSession, interface: NativeInterface) -> Sexp<'static> {
        let name = match interface {
            NativeInterface::Call => ".Call",
            NativeInterface::External => ".External",
            NativeInterface::External2 => ".External2",
        };
        unsafe {
            let raw = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            session
                .owner_token()
                .unwrap()
                .sexp(raw)
                .unwrap()
                .into_owned()
                .unwrap()
        }
    }

    fn rejection(operation: impl FnOnce() -> SEXP) -> String {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        let payload = result.expect_err("incompatible native request must be rejected");
        payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("native rejection is an R error")
            .message
            .clone()
    }

    #[test]
    fn owning_native_handlers_reject_wrong_interfaces_before_allocation() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            for (name, interface) in [
                ("C_par", NativeInterface::External),
                ("C_nlm", NativeInterface::External),
                ("C_termsform", NativeInterface::External2),
                ("C_doTabExpand", NativeInterface::External),
                ("C_nonASCII", NativeInterface::External2),
                ("C_typeconvert", NativeInterface::Call),
                ("C_contourDef", NativeInterface::External),
                ("C_signrank_free", NativeInterface::Call),
            ] {
                let arguments = make_arguments(&factory, name, &[]);
                let operator = operator(&session, interface);
                let before =
                    session.with_active_in(|instance| unsafe { (*instance).arena.node_count() });
                let message = rejection(|| {
                    if interface == NativeInterface::Call {
                        do_dotcall(
                            factory.nil().as_raw(),
                            operator.as_raw(),
                            arguments.as_raw(),
                            factory.nil().as_raw(),
                        )
                    } else {
                        do_External(
                            factory.nil().as_raw(),
                            operator.as_raw(),
                            arguments.as_raw(),
                            factory.nil().as_raw(),
                        )
                    }
                });
                assert!(
                    message.contains("routine called through"),
                    "{name}: {message}"
                );
                assert_eq!(
                    session.with_active_in(|instance| unsafe { (*instance).arena.node_count() }),
                    before
                );
            }
        });
    }

    #[test]
    fn owning_native_handlers_reject_all_registered_call_arity_mismatches() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let operator = operator(&session, NativeInterface::Call);
            let nil = factory.nil().into_owned().unwrap();
            for (name, expected) in [
                ("C_R_identC", 2usize),
                ("C_spline_basis", 4),
                ("C_pretty2", 2),
                ("C_rpois", 2),
                ("C_tzcode_type", 0),
            ] {
                for actual in [expected.saturating_sub(1), expected + 1] {
                    if actual == expected {
                        continue;
                    }
                    let values = vec![(nil.clone(), nil.clone()); actual];
                    let arguments = make_arguments(&factory, name, &values);
                    let message = rejection(|| {
                        do_dotcall(
                            nil.as_raw(),
                            operator.as_raw(),
                            arguments.as_raw(),
                            nil.as_raw(),
                        )
                    });
                    assert!(
                        message.contains(&format!("expected {expected}, received {actual}")),
                        "{name}: {message}"
                    );
                }
            }
        });
    }

    #[test]
    fn owning_native_call_honors_package_and_excludes_control_from_payload() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let operator = operator(&session, NativeInterface::Call);
            let value = factory.strings(&["same"]).unwrap().into_owned().unwrap();
            let invalid_package = {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::constructors::Rf_ScalarInteger(42))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::symbol::install_str("PACKAGE"))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let package = factory.strings(&["methods"]).unwrap().into_owned().unwrap();
            for position in 0..=2 {
                let mut payload = vec![(value.clone(), nil.clone()), (value.clone(), nil.clone())];
                payload.insert(position, (package.clone(), tag.clone()));
                let arguments = make_arguments(&factory, "C_R_identC", &payload);
                let result = {
                    do_dotcall(
                        nil.as_raw(),
                        operator.as_raw(),
                        arguments.as_raw(),
                        nil.as_raw(),
                    )
                };
                assert_eq!(
                    factory.wrap(result).unwrap().try_logical_elt(0).unwrap(),
                    TRUE
                );
            }
            let wrong_package = factory.strings(&["tools"]).unwrap().into_owned().unwrap();
            let arguments = make_arguments(
                &factory,
                "C_R_identC",
                &[
                    (value.clone(), nil.clone()),
                    (wrong_package, tag.clone()),
                    (value.clone(), nil.clone()),
                ],
            );
            let message = rejection(|| {
                do_dotcall(
                    nil.as_raw(),
                    operator.as_raw(),
                    arguments.as_raw(),
                    nil.as_raw(),
                )
            });
            assert!(
                message.contains("disabled"),
                "explicit wrong package must not fall back globally: {message}"
            );
            let invalid = make_arguments(&factory, "C_R_identC", &[(invalid_package, tag)]);
            let message = rejection(|| {
                do_dotcall(
                    nil.as_raw(),
                    operator.as_raw(),
                    invalid.as_raw(),
                    nil.as_raw(),
                )
            });
            assert!(message.contains("PACKAGE argument must be a single character string"));
        });
    }

    #[test]
    fn owning_native_external_retains_detached_controls_across_real_gc() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let operator = operator(&session, NativeInterface::External);
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::symbol::install_str("PACKAGE"))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let package = factory
                .strings(&["grDevices"])
                .unwrap()
                .into_owned()
                .unwrap();
            let package_token = package.allocation().unwrap().clone();
            let arguments = make_arguments(&factory, "C_devcur", &[(package, tag)]);
            let original = arguments.as_raw();
            let name_token = arguments.try_car().unwrap().allocation().unwrap().clone();
            let calls = Rc::new(Cell::new(0));
            let notifications = calls.clone();
            let callback_owner = session.owner_token().unwrap().weak_owner().unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if notifications.get() != 0 {
                    return;
                }
                notifications.set(1);
                unsafe {
                    let pin = callback_owner.pin().unwrap();
                    let instance = pin.as_ptr();
                    crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 0, None);
                    crate::sexp::accessors::SETCAR(original, R_NilValue());
                    SETCDR(original, R_NilValue());
                }
                crate::sexp::gengc::full_gc();
                assert!(name_token.is_live());
                assert!(
                    package_token.is_live(),
                    "the removed control has only the operation snapshot as a root"
                );
            }));
            session.with_active_in(|instance| unsafe {
                crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 1, Some(1));
            });
            let result = {
                do_External(
                    nil.as_raw(),
                    operator.as_raw(),
                    arguments.as_raw(),
                    nil.as_raw(),
                )
            };
            assert_eq!(factory.wrap(result).unwrap().try_integer_elt(0).unwrap(), 1);
            assert_eq!(calls.get(), 1);
        });
    }

    #[test]
    fn owning_native_external_denies_publication_after_callback_close() {
        // No RefCell loan or RSession reference is held across the callback.
        let session = RSession::new_for_gc_tests();
        let factory = crate::sexp::owner::OwnerToken::current()
            .unwrap()
            .node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let operator = operator(&session, NativeInterface::External);
        let arguments = make_arguments(&factory, "C_devcur", &[]);
        let callback_owner = session.owner_token().unwrap().weak_owner().unwrap();
        let sessions = Rc::new(std::cell::RefCell::new(Some(session)));
        let weak = Rc::downgrade(&sessions);
        let calls = Rc::new(Cell::new(0));
        let notifications = calls.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if notifications.get() != 0 {
                return;
            }
            notifications.set(1);
            unsafe {
                let pin = callback_owner.pin().unwrap();
                let instance = pin.as_ptr();
                crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 0, None);
            }
            weak.upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }));
        unsafe {
            let instance =
                crate::sexp::owner::OwnerToken::with_current(|owner| owner.as_ptr()).unwrap();
            crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 1, Some(1));
        }
        let message = rejection(|| {
            do_External(
                nil.as_raw(),
                operator.as_raw(),
                arguments.as_raw(),
                nil.as_raw(),
            )
        });
        assert!(
            message.contains("owner could not retain its root"),
            "{message}"
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn owning_native_altrep_name_and_package_collect_and_reject_revoked_runtime() {
        use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
        use crate::sexp::object::SexpResult;
        use std::cell::RefCell;

        struct CollectingText {
            text: &'static str,
            calls: Rc<Cell<usize>>,
            close: bool,
            session: std::rc::Weak<RefCell<Option<RSession>>>,
        }
        impl AltrepClass for CollectingText {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::STRSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
                Ok(1)
            }
            fn element<'s>(
                &self,
                context: &AltrepContext<'s>,
                _: R_xlen_t,
            ) -> SexpResult<AltrepElement<'s>> {
                self.calls.set(self.calls.get() + 1);
                let text = context.string(self.text)?;
                context.gc()?;
                if self.close {
                    self.session
                        .upgrade()
                        .unwrap()
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .close();
                }
                Ok(AltrepElement::String(text))
            }
        }

        for package_provider in [false, true] {
            for close in [false, true] {
                let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
                let calls = Rc::new(Cell::new(0));
                let collections = Rc::new(Cell::new(0));
                let observed = collections.clone();
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    observed.set(observed.get() + 1);
                }));
                // Release all RSession and RefCell loans before provider entry.
                let (factory, nil, operator, arguments) = {
                    let borrowed = sessions.borrow();
                    let session = borrowed.as_ref().unwrap();
                    let factory = crate::sexp::owner::OwnerToken::current()
                        .unwrap()
                        .node_factory();
                    let nil = factory.nil().into_owned().unwrap();
                    let operator = operator(session, NativeInterface::Call);
                    let class = session
                        .register_altrep_class(
                            "native_lookup_collecting_text",
                            CollectingText {
                                text: if package_provider {
                                    "methods"
                                } else {
                                    "C_R_identC"
                                },
                                calls: calls.clone(),
                                close,
                                session: Rc::downgrade(&sessions),
                            },
                        )
                        .unwrap()
                        .into_owned()
                        .unwrap();
                    let provider = AltrepBuilder::new(class)
                        .build()
                        .unwrap()
                        .into_owned()
                        .unwrap();
                    let name = if package_provider {
                        factory
                            .strings(&["C_R_identC"])
                            .unwrap()
                            .into_owned()
                            .unwrap()
                    } else {
                        provider.clone()
                    };
                    let package = if package_provider {
                        provider
                    } else {
                        factory.strings(&["methods"]).unwrap().into_owned().unwrap()
                    };
                    let tag = unsafe {
                        session
                            .owner_token()
                            .unwrap()
                            .sexp(crate::sexp::symbol::install_str("PACKAGE"))
                            .unwrap()
                            .into_owned()
                            .unwrap()
                    };
                    let value = factory.strings(&["same"]).unwrap().into_owned().unwrap();
                    let arguments = make_arguments_value(
                        &factory,
                        &name,
                        &[
                            (value.clone(), nil.clone()),
                            (package, tag),
                            (value, nil.clone()),
                        ],
                    );
                    (factory, nil, operator, arguments)
                };
                let invoke = || {
                    do_dotcall(
                        nil.as_raw(),
                        operator.as_raw(),
                        arguments.as_raw(),
                        nil.as_raw(),
                    )
                };
                if close {
                    let message = rejection(invoke);
                    assert!(
                        message.contains("owner could not retain its root"),
                        "{message}"
                    );
                } else {
                    let result = invoke();
                    assert_eq!(
                        factory.wrap(result).unwrap().try_logical_elt(0).unwrap(),
                        TRUE
                    );
                }
                assert_eq!(calls.get(), 1, "the actual lookup provider must run once");
                assert!(
                    collections.get() > 0,
                    "the provider must execute an actual collection"
                );
            }
        }
    }

    #[test]
    fn owning_native_external_payload_metadata_matches_independent_gnu_registry() {
        use crate::mainutils::native_routines::PayloadArity;
        // GNU R devel 4.7.0 r90451, commit bac583951b728e97b9786804d3b4081f0fe18df5.
        // Independent getDLLRegisteredRoutines inventory, not an implementation table.
        // The 107 rows include unsupported routines; only resolved bundled entries
        // are checked, without treating absent functionality as a passing call.
        const REGISTERED: &str = r"stats	compcases	-1
stats	doD	2
stats	deriv	5
stats	modelframe	8
stats	modelmatrix	2
stats	termsform	5
stats	do_fmin	4
stats	nlm	11
stats	zeroin2	7
stats	optim	7
stats	optimhess	4
stats	call_dqags	7
stats	call_dqagi	7
stats	signrank_free	0
stats	wilcox_free	0
tools	parseLatex	6
tools	parseRd	9
grDevices	PicTeX	6
grDevices	PostScript	19
grDevices	PDF	23
grDevices	devCairo	12
grDevices	devcap	1
grDevices	devcapture	1
grDevices	devcontrol	1
grDevices	devcopy	1
grDevices	devcur	0
grDevices	devdisplaylist	0
grDevices	devholdflush	1
grDevices	devnext	1
grDevices	devoff	1
grDevices	devprev	1
grDevices	devset	1
grDevices	devsize	0
grDevices	contourLines	4
grDevices	getSnapshot	0
grDevices	playSnapshot	1
grDevices	getGraphicsEvent	1
grDevices	getGraphicsEventEnv	1
grDevices	setGraphicsEventEnv	2
grDevices	setPattern	1
grDevices	setClipPath	2
grDevices	setMask	2
grDevices	defineGroup	3
grDevices	useGroup	2
grDevices	devUp	0
grDevices	devAskNewPage	1
grDevices	savePlot	3
grDevices	Quartz	11
grDevices	X11	18
graphics	C_contour	-1
graphics	C_filledcontour	5
graphics	C_image	4
graphics	C_persp	-1
graphics	C_abline	-1
graphics	C_axis	-1
graphics	C_arrows	-1
graphics	C_box	-1
graphics	C_clip	-1
graphics	C_convertX	3
graphics	C_convertY	3
graphics	C_dend	-1
graphics	C_dendwindow	-1
graphics	C_erase	-1
graphics	C_layout	-1
graphics	C_mtext	-1
graphics	C_par	-1
graphics	C_path	-1
graphics	C_plotXY	-1
graphics	C_plot_window	-1
graphics	C_polygon	-1
graphics	C_raster	-1
graphics	C_rect	-1
graphics	C_segments	-1
graphics	C_strHeight	-1
graphics	C_strWidth	-1
graphics	C_symbols	-1
graphics	C_text	-1
graphics	C_title	-1
graphics	C_xspline	-1
graphics	C_plot_new	0
graphics	C_locator	-1
graphics	C_identify	-1
utils	download	6
utils	unzip	7
utils	Rprof	10
utils	Rprofmem	3
utils	countfields	6
utils	readtablehead	7
utils	typeconvert	6
utils	writetable	11
utils	addhistory	1
utils	loadhistory	1
utils	savehistory	1
utils	dataentry	2
utils	dataviewer	2
utils	edit	4
utils	fileedit	3
utils	selectlist	4
utils	hashtab_Ext	2
utils	gethash_Ext	3
utils	sethash_Ext	3
utils	remhash_Ext	2
utils	numhash_Ext	1
utils	typhash_Ext	1
utils	maphash_Ext	2
utils	clrhash_Ext	1
utils	ishashtab_Ext	1";
        let mut covered = 0;
        assert_eq!(REGISTERED.lines().count(), 107);
        for line in REGISTERED.lines() {
            let mut fields = line.split('\t');
            let package = fields.next().unwrap();
            let name = fields.next().unwrap();
            let count: i32 = fields.next().unwrap().parse().unwrap();
            let Some(routine) = lookup_bundled_native(name, Some(package)) else {
                continue;
            };
            assert_ne!(
                routine.interface(),
                NativeInterface::Call,
                "{package}::{name}"
            );
            let expected = if count == -1 {
                PayloadArity::Variadic
            } else {
                PayloadArity::Fixed(count as usize)
            };
            assert_eq!(routine.payload_arity(), expected, "{package}::{name}");
            covered += 1;
        }
        // 68 since the PostScript device entry point landed (2026-10-11).
        assert_eq!(
            covered, 68,
            "static registration coverage, not handler functionality"
        );
        assert_eq!(
            lookup_bundled_native("parseRdText", Some("tools"))
                .unwrap()
                .payload_arity(),
            PayloadArity::Fixed(9)
        );
        assert_eq!(
            lookup_bundled_native("plot_xy", Some("graphics"))
                .unwrap()
                .payload_arity(),
            PayloadArity::Variadic
        );
    }

    #[test]
    fn owning_native_external_rejects_fixed_payload_counts_before_allocation_or_gc() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::symbol::install_str("PACKAGE"))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let mut requests = Vec::new();
            for (name, package, expected, interface) in [
                ("C_devcur", "grDevices", 0usize, NativeInterface::External),
                ("C_devnext", "grDevices", 1, NativeInterface::External),
                ("C_PDF", "grDevices", 23, NativeInterface::External),
                ("C_typeconvert", "utils", 6, NativeInterface::External2),
                ("C_parseRd", "tools", 9, NativeInterface::External2),
                ("C_nlm", "stats", 11, NativeInterface::External2),
                ("C_do_fmin", "stats", 4, NativeInterface::External2),
                ("C_image", "graphics", 4, NativeInterface::External),
                ("C_signrank_free", "stats", 0, NativeInterface::External),
            ] {
                let package = factory.strings(&[package]).unwrap().into_owned().unwrap();
                for actual in [expected.saturating_sub(1), expected + 1] {
                    if actual == expected {
                        continue;
                    }
                    let mut payload = vec![(nil.clone(), nil.clone()); actual];
                    payload.insert(actual / 2, (package.clone(), tag.clone()));
                    requests.push((
                        name,
                        expected,
                        actual,
                        interface,
                        operator(&session, interface),
                        make_arguments(&factory, name, &payload),
                    ));
                }
            }
            let callbacks = Rc::new(Cell::new(0));
            let observed = callbacks.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1)
            }));
            session.with_active_in(|instance| unsafe {
                crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 1, Some(1));
            });
            let before =
                session.with_active_in(|instance| unsafe { (*instance).arena.node_count() });
            for (name, expected, actual, _, operator, arguments) in requests {
                let message = rejection(|| {
                    do_External(
                        nil.as_raw(),
                        operator.as_raw(),
                        arguments.as_raw(),
                        nil.as_raw(),
                    )
                });
                assert!(
                    message.contains(&format!("expected {expected}, received {actual}")),
                    "{name}: {message}"
                );
                assert_eq!(
                    session.with_active_in(|instance| unsafe { (*instance).arena.node_count() }),
                    before
                );
                assert_eq!(
                    callbacks.get(),
                    0,
                    "rejection must precede wrapper allocations and collection"
                );
            }
            session.with_active_in(|instance| unsafe {
                crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 0, None);
            });
        });
    }

    #[test]
    fn owning_native_external_fixed_and_variadic_neighbors_execute_genuine_handlers() {
        let mut session = RSession::new_for_gc_tests();
        let function = session
            .eval_code_with_output_capture("function(x) (x - 2)^2")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::symbol::install_str("PACKAGE"))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let scalar_int = |value| {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::constructors::Rf_ScalarInteger(value))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let scalar_real = |value| {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::constructors::Rf_ScalarReal(value))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let invoke = |name, package, interface, values: Vec<Sexp<'static>>| {
                let package = factory.strings(&[package]).unwrap().into_owned().unwrap();
                let mut payload: Vec<_> = values.into_iter().map(|v| (v, nil.clone())).collect();
                payload.insert(payload.len() / 2, (package, tag.clone()));
                let arguments = make_arguments(&factory, name, &payload);
                let op = operator(&session, interface);
                let result =
                    { do_External(nil.as_raw(), op.as_raw(), arguments.as_raw(), nil.as_raw()) };
                factory.wrap(result).unwrap().into_owned().unwrap()
            };
            assert_eq!(
                invoke("C_devcur", "grDevices", NativeInterface::External, vec![])
                    .try_integer_elt(0)
                    .unwrap(),
                1
            );
            assert_eq!(
                invoke(
                    "C_devnext",
                    "grDevices",
                    NativeInterface::External,
                    vec![scalar_int(1)]
                )
                .try_integer_elt(0)
                .unwrap(),
                1
            );
            let optimum = invoke(
                "C_do_fmin",
                "stats",
                NativeInterface::External2,
                vec![
                    function,
                    scalar_real(0.0),
                    scalar_real(4.0),
                    scalar_real(0.01),
                ],
            )
            .try_real_elt(0)
            .unwrap();
            assert!((optimum - 2.0).abs() < 0.01);
            let integer_vector = |values: &[i32]| {
                let value = factory
                    .allocate(|arena| {
                        arena
                            .alloc_vector_sexp(SEXPTYPE::INTSXP, values.len() as R_xlen_t)
                            .map(|value| value.as_raw())
                    })
                    .unwrap();
                let mut value = crate::sexp::object::SexpMut::try_from_checked(value).unwrap();
                for (index, scalar) in values.iter().enumerate() {
                    value
                        .try_set_integer_elt(index as R_xlen_t, *scalar)
                        .unwrap();
                }
                value.freeze().into_owned().unwrap()
            };
            let x = integer_vector(&[1, NA_INTEGER]);
            let y = integer_vector(&[2, 3]);
            let complete = invoke(
                "C_compcases",
                "stats",
                NativeInterface::External,
                vec![x, y],
            );
            assert_eq!(complete.len(), 2);
            assert_eq!(complete.try_logical_elt(0).unwrap(), TRUE);
            assert_eq!(complete.try_logical_elt(1).unwrap(), FALSE);
            // External passes one rooted list pointer, not Call's bounded
            // pointer array. GNU accepts this genuine 66-payload variadic call.
            let x = integer_vector(&[1, NA_INTEGER]);
            let complete = invoke(
                "C_compcases",
                "stats",
                NativeInterface::External,
                vec![x; 66],
            );
            assert_eq!(complete.len(), 2);
            assert_eq!(complete.try_logical_elt(0).unwrap(), TRUE);
            assert_eq!(complete.try_logical_elt(1).unwrap(), FALSE);
        });
    }
}
