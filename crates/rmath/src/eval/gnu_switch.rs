//! Constant-pool validation and selection for GNU SWITCH bytecode.
use crate::sexp::accessors::{INTEGER, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH};
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use std::collections::BTreeMap;

/// Extract all possible successors before the stack/control-flow verifier runs.
/// The instruction framing is checked before any operands are indexed.
pub(super) unsafe fn targets(
    code: &[i32],
    pool: SEXP,
) -> Result<BTreeMap<usize, Vec<usize>>, String> {
    unsafe {
        super::bytecode::validate_gnu_bytecode_stream(code)?;
        if TYPEOF(pool) != SEXPTYPE::VECSXP {
            return Err("GNU bytecode has no constant pool".into());
        }
        let mut result = BTreeMap::new();
        let mut pc = 1;
        while pc < code.len() {
            let op = code[pc];
            if op == super::bytecode::GNU_OP_SWITCH {
                let mut args = [R_NilValue(); 4];
                for (i, arg) in args.iter_mut().enumerate() {
                    let index = code[pc + 1 + i];
                    if index < 0 || index as i64 >= XLENGTH(pool) {
                        return Err("GNU SWITCH constant index out of range".into());
                    }
                    *arg = VECTOR_ELT(pool, index as i64);
                }
                let names = args[1];
                if TYPEOF(args[3]) != SEXPTYPE::INTSXP || XLENGTH(args[3]) == 0 {
                    return Err("bad numeric 'switch' offsets".into());
                }
                if names != R_NilValue()
                    && (TYPEOF(names) != SEXPTYPE::STRSXP
                        || TYPEOF(args[2]) != SEXPTYPE::INTSXP
                        || XLENGTH(names) == 0
                        || XLENGTH(names) != XLENGTH(args[2]))
                {
                    return Err("bad 'switch' names or character offsets".into());
                }
                let mut successors = std::collections::BTreeSet::new();
                for offsets in [
                    args[3],
                    if names == R_NilValue() {
                        R_NilValue()
                    } else {
                        args[2]
                    },
                ] {
                    if offsets == R_NilValue() {
                        continue;
                    }
                    for i in 0..XLENGTH(offsets) {
                        let target = *INTEGER(offsets).add(i as usize);
                        if target < 1 || target as usize >= code.len() {
                            return Err("GNU SWITCH target outside instruction stream".into());
                        }
                        successors.insert(target as usize);
                    }
                }
                result.insert(pc, successors.into_iter().collect());
            }
            pc += 1 + super::bytecode::GNU_BC_OPERAND_WIDTHS[op as usize] as usize;
        }
        Ok(result)
    }
}

/// Constants have passed `targets`; the selector remains rooted by the caller.
pub(super) unsafe fn select(
    value: SEXP,
    call: SEXP,
    names: SEXP,
    chars: SEXP,
    ints: SEXP,
) -> Result<usize, String> {
    unsafe {
        if crate::sexp::constructors::Rf_isVector(value) == 0 || XLENGTH(value) != 1 {
            return Err("EXPR must be a length 1 vector".into());
        }
        if crate::mainutils::connections::inherits_class(value, "factor") {
            crate::mainutils::errors::warningcall(call, c"EXPR is a \"factor\", treated as integer.\n Consider using 'switch(as.character( * ), ...)' instead.".as_ptr());
        }
        let numeric_len = XLENGTH(ints);
        if TYPEOF(value) == SEXPTYPE::STRSXP {
            if names == R_NilValue() {
                if numeric_len != 1 {
                    return Err(
                        "numeric EXPR required for 'switch' without named alternatives".into(),
                    );
                }
                crate::mainutils::errors::warningcall(
                    call,
                    c"'switch' with no alternatives".as_ptr(),
                );
                return Ok(*INTEGER(ints) as usize);
            } else {
                let n = XLENGTH(names);
                let selector = STRING_ELT(value, 0);
                let mut which = n - 1;
                for i in 0..n - 1 {
                    if crate::mainutils::match_mod::pmatch(selector, STRING_ELT(names, i), 1) != 0 {
                        which = i;
                        break;
                    }
                }
                return Ok(*INTEGER(chars).add(which as usize) as usize);
            }
        }
        if numeric_len == 1 {
            crate::mainutils::errors::warningcall(call, c"'switch' with no alternatives".as_ptr());
        }
        let index = crate::main::coerce::asInteger(value);
        let which = if index == NA_INTEGER || index < 1 || index as i64 > numeric_len {
            numeric_len - 1
        } else {
            index as i64 - 1
        };
        Ok(*INTEGER(ints).add(which as usize) as usize)
    }
}
