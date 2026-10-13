//! Stack accessors and pending-argument helpers — extracted verbatim from eval/bc_eval.rs

use super::*;

/// Get the stack depth from a BCODESXP.
pub unsafe fn BCODE_STACK(x: SEXP) -> c_int {
    let body = unsafe { own_operand(x) };
    let hint = unsafe { own_operand(VECTOR_ELT(body.as_raw(), 2)) };
    if hint.typeof_() != SEXPTYPE::INTSXP || hint.len() != 1 {
        bc_error("bytecode stack hint must be a scalar integer");
    }
    let value = hint
        .integer_elt(0)
        .unwrap_or_else(|| bc_error("missing bytecode stack hint"));
    if value < 0 {
        bc_error("negative bytecode stack hint");
    }
    value
}

pub(crate) unsafe fn read_operand(
    code_ptr: &[c_int],
    pc: &mut c_int,
    code_len: c_int,
    opname: &str,
) -> c_int {
    unsafe {
        if *pc >= code_len {
            bc_error(format!("{opname} bytecode operand is truncated"));
        }
        let value = usize::try_from(*pc)
            .ok()
            .and_then(|index| code_ptr.get(index))
            .copied()
            .unwrap_or_else(|| bc_error(format!("{opname} bytecode operand is truncated")));
        *pc += 1;
        value
    }
}

pub(crate) unsafe fn read_jump_target(
    code_ptr: &[c_int],
    pc: &mut c_int,
    code_len: c_int,
    opname: &str,
) -> c_int {
    unsafe {
        let target = read_operand(code_ptr, pc, code_len, opname);
        if target < 0 || target > code_len {
            bc_error(format!(
                "{opname} bytecode jump target {target} is outside instruction stream length {code_len}"
            ));
        }
        target
    }
}

pub(crate) fn stack_pop_checked(stack: &mut R_bcstack_t, context: &str) -> Sexp<'static> {
    if stack.is_empty() {
        bc_error(format!("{context} bytecode stack underflow"));
    }
    stack.pop_owned()
}

/// GNU asReal accepts character and other atomic inputs. COLON reuses
/// do_colon, whose local asReal only reads INT/REAL/LGL, so non-numeric
/// arguments are coerced to real first.
pub(crate) unsafe fn gnu_colon_numeric_arg(value: SEXP) -> SEXP {
    unsafe {
        let ty = TYPEOF(value);
        if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::REALSXP || ty == SEXPTYPE::LGLSXP {
            value
        } else {
            crate::mainutils::coerce::coerceVector(value, SEXPTYPE::REALSXP.0)
        }
    }
}

/// Apply recorded OP_SETTAG tags to the argument cells a call opcode just
/// rebuilt. `top` is the stack depth AFTER the function was popped and
/// BEFORE the argument values were popped, so a value recorded at depth
/// `d` is the cell created on pop `top - 1 - d` (0-based, top-first).
/// Consumed entries are removed; unmatched entries (stale) are dropped.
pub(crate) unsafe fn apply_pending_arg_tags(
    pending: &mut Vec<(usize, Sexp<'static>)>,
    top: usize,
    cells: &[SEXP],
) {
    if pending.is_empty() {
        return;
    }
    let taken = std::mem::take(pending);
    for (d, tag) in taken {
        let j = top as isize - 1 - d as isize;
        if j >= 0 && (j as usize) < cells.len() {
            unsafe {
                crate::sexp::accessors::SETTAG(cells[j as usize], tag.as_raw());
            }
        }
    }
}
