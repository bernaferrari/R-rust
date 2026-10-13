//! Loop machinery, control flow, getvar/dollar value operations — extracted verbatim from eval/bc_eval.rs

use super::*;

#[derive(Clone)]
pub(crate) struct ForLoopState {
    pub(crate) sequence_slot: usize,
    pub(crate) symbol: Sexp<'static>,
    pub(crate) index: c_int,
    pub(crate) length: c_int,
}

#[derive(Clone)]
pub(crate) struct GnuForLoopState {
    pub(crate) sequence_slot: usize,
    pub(crate) symbol: Sexp<'static>,
    pub(crate) index: c_int,
    pub(crate) length: c_int,
}

pub(crate) struct GnuCallFrame {
    pub(crate) marker: usize,
    // Stack slot and tag symbol for each SETTAG-ed promise. The slot is
    // resolved against the argument pairlist only when CALL rebuilds it.
    pub(crate) tags: Vec<(usize, Sexp<'static>)>,
    // STARTSUBSET/DFLTSUBSET accumulate raw arguments instead of wrapping
    // them as GETFUN promises. GNU INIT_CALL_FRAME(R_NilValue) does the same.
    // STARTSUBASSIGN/DFLTSUBASSIGN reuse that raw frame, with lhs at marker
    // and rhs in the next slot so missing indices can be pushed after it.
    pub(crate) raw_args: bool,
    pub(crate) call: Sexp<'static>,
}

/// Runtime loop context for compiled `while`/`for` loops — the port of
/// eval.c's STARTLOOPCNTXT/ENDLOOPCNTXT pair.
///
/// BEGINLOOP records where `break`/`next` continue, plus the operand-stack
/// depth and for-loop state depth at loop entry, so an exit from mid-body
/// (including a `break` unwinding out of a nested `tryCatch`) can discard the
/// loop's partial operands and sequence state before resuming at the exit.
#[derive(Clone, Copy)]
pub(crate) struct LoopContext {
    pub(crate) break_target: c_int,
    pub(crate) next_target: c_int,
    pub(crate) stack_depth: usize,
    pub(crate) for_depth: usize,
}

/// A resolved non-local loop exit produced by `break`/`next` surfacing from a
/// nested evaluation. The caller applies the depth truncations and jumps.
pub(crate) struct LoopJump {
    pub(crate) target: c_int,
    pub(crate) stack_depth: usize,
    pub(crate) for_depth: usize,
}

pub(crate) fn loop_jump_from_context(ctx: &LoopContext, is_break: bool) -> LoopJump {
    LoopJump {
        target: if is_break {
            ctx.break_target
        } else {
            ctx.next_target
        },
        stack_depth: ctx.stack_depth,
        for_depth: ctx.for_depth,
    }
}

pub(crate) fn recover_loop_jump(
    payload: Box<dyn std::any::Any + Send>,
    loop_stack: &[LoopContext],
) -> LoopJump {
    match payload.downcast::<crate::sexp::context::RSignal>() {
        Ok(signal) => {
            let is_break = matches!(*signal, crate::sexp::context::RSignal::Break);
            let is_loop_signal = is_break || matches!(*signal, crate::sexp::context::RSignal::Next);
            if !is_loop_signal {
                std::panic::panic_any(*signal);
            }
            match loop_stack.last() {
                Some(ctx) => loop_jump_from_context(ctx, is_break),
                None => std::panic::panic_any(*signal),
            }
        }
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Evaluate `call` in `rho`, routing `break`/`next` signals that escape the
/// nested evaluation to the innermost compiled-loop context.
///
/// This is the bytecode counterpart of eval.c's `findcontext(CTXT_BREAK/
/// CTXT_NEXT, ...)`: an R-level `break` inside a `tryCatch` whose body runs
/// under a compiled loop unwinds as an `RSignal` panics; the bytecode loop
/// owns the innermost loop context, so `bcEval` — not the AST evaluator —
/// must catch it here and convert it into a pc jump. Any other signal
/// (`Error`, `Return`, condition-handler jumps, ...) re-panics unchanged.
/// With no loop context on record the signal also re-panics, letting an
/// enclosing AST loop (or the top level) handle it.
pub(crate) unsafe fn eval_call_with_loop_routing(
    call: SEXP,
    rho: SEXP,
    loop_stack: &[LoopContext],
) -> Result<SEXP, LoopJump> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::eval::eval::Rf_eval(call, rho)
    }));
    match result {
        Ok(value) => Ok(value),
        Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
            Ok(signal) => {
                let is_break = matches!(*signal, crate::sexp::context::RSignal::Break);
                let is_loop_signal =
                    is_break || matches!(*signal, crate::sexp::context::RSignal::Next);
                if !is_loop_signal {
                    std::panic::panic_any(*signal);
                }
                match loop_stack.last() {
                    Some(ctx) => Err(loop_jump_from_context(ctx, is_break)),
                    None => std::panic::panic_any(*signal),
                }
            }
            Err(payload) => std::panic::resume_unwind(payload),
        },
    }
}

/// Root the residual operand stack around a nested call evaluation and route
/// escaping loop signals. Combines the GC-rooting window of
/// `with_stack_rooted` with `eval_call_with_loop_routing`.
pub(crate) unsafe fn eval_nested_call(
    call: SEXP,
    rho: SEXP,
    stack: &R_bcstack_t,
    loop_stack: &[LoopContext],
) -> Result<SEXP, LoopJump> {
    unsafe {
        with_stack_rooted(stack, call, || unsafe {
            eval_call_with_loop_routing(call, rho, loop_stack)
        })
    }
}

/// Apply a resolved loop jump: discard partial loop operands and for-loop
/// state, then continue at the loop's break/next target.
pub(crate) unsafe fn apply_loop_jump<T>(
    stack: &mut R_bcstack_t,
    for_loops: &mut Vec<T>,
    jump: LoopJump,
) -> c_int {
    unsafe {
        for_loops.truncate(jump.for_depth);
        stack.set_depth(jump.stack_depth);
        jump.target
    }
}

pub(crate) unsafe fn for_sequence_length(sequence: SEXP) -> Option<c_int> {
    unsafe {
        match TYPEOF(sequence) {
            t if t == SEXPTYPE::NILSXP => Some(0),
            t if t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
                || t == SEXPTYPE::VECSXP
                || t == SEXPTYPE::EXPRSXP
                || t == SEXPTYPE::LISTSXP
                || t == SEXPTYPE::LANGSXP =>
            {
                Some(LENGTH(sequence))
            }
            _ => None,
        }
    }
}

pub(crate) unsafe fn for_sequence_element(sequence: SEXP, index: c_int) -> SEXP {
    unsafe {
        match TYPEOF(sequence) {
            t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(*LOGICAL(sequence).add(index as usize)),
            t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(*INTEGER(sequence).add(index as usize)),
            t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(*REAL(sequence).add(index as usize)),
            t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(*COMPLEX(sequence).add(index as usize)),
            t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(sequence, index as i64)),
            t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(*RAW(sequence).add(index as usize)),
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                VECTOR_ELT(sequence, index as i64)
            }
            t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => {
                let mut node = sequence;
                for _ in 0..index {
                    node = CDR(node);
                }
                CAR(node)
            }
            _ => bc_error("invalid for() loop sequence"),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: evaluate a condition value to bool
// ---------------------------------------------------------------------------

pub(crate) unsafe fn gnu_is_number(value: SEXP) -> bool {
    unsafe {
        if value.is_null() {
            return false;
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => {
                crate::mainutils::objects::inherits2(value, c"factor".as_ptr()) == 0
            }
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::REALSXP || t == SEXPTYPE::CPLXSXP => true,
            _ => false,
        }
    }
}

/// eval.c FIXUP_SCALAR_LOGICAL: coerce the current && / || operand to a
/// length-1 logical. AND1ST/OR1ST keep the boxed value on the stack.
pub(crate) unsafe fn fixup_scalar_logical(value: SEXP, call: SEXP, arg: &str, op: &str) -> c_int {
    unsafe {
        if TYPEOF(value) == SEXPTYPE::LGLSXP
            && crate::mainutils::relop::IS_SIMPLE_SCALAR(value, SEXPTYPE::LGLSXP.into()) != 0
        {
            return crate::sexp::accessors::SCALAR_LVAL(value);
        }
        if !gnu_is_number(value) {
            crate::mainutils::coerce::errorcall(call, &format!("invalid {arg} type in 'x {op} y'"));
        }
        crate::mainutils::coerce::asLogical2(value, 1, call)
    }
}

/// Evaluate a value as a boolean condition for branching.
pub(crate) unsafe fn eval_bc_condition(val: SEXP) -> bool {
    unsafe {
        let length = if val.is_null() { 0 } else { LENGTH(val) };
        if length == 0 {
            bc_error("argument is of length zero");
        }
        if length > 1 {
            bc_error("the condition has length > 1");
        }
        let condition = crate::mainutils::coerce::asLogical(val);
        if condition == NA_LOGICAL {
            bc_error("missing value where TRUE/FALSE needed");
        }
        condition != FALSE
    }
}

/// GNU ISNULL..ISNUMERIC match eval.c DO_ISTEST/DO_ISTYPE.
/// is.integer excludes factors; is.numeric is numeric-but-not-logical.
pub(crate) unsafe fn eval_gnu_istype(opcode: c_int, value: SEXP) -> SEXP {
    unsafe {
        use super::super::bytecode::{
            GNU_OP_ISCHARACTER, GNU_OP_ISCOMPLEX, GNU_OP_ISDOUBLE, GNU_OP_ISINTEGER,
            GNU_OP_ISLOGICAL, GNU_OP_ISNULL, GNU_OP_ISNUMERIC, GNU_OP_ISOBJECT, GNU_OP_ISSYMBOL,
        };
        let test = match opcode {
            GNU_OP_ISNULL => TYPEOF(value) == SEXPTYPE::NILSXP,
            GNU_OP_ISLOGICAL => TYPEOF(value) == SEXPTYPE::LGLSXP,
            GNU_OP_ISINTEGER => {
                TYPEOF(value) == SEXPTYPE::INTSXP
                    && crate::mainutils::seq::inherits(value, c"factor".as_ptr()) == 0
            }
            GNU_OP_ISDOUBLE => TYPEOF(value) == SEXPTYPE::REALSXP,
            GNU_OP_ISCOMPLEX => TYPEOF(value) == SEXPTYPE::CPLXSXP,
            GNU_OP_ISCHARACTER => TYPEOF(value) == SEXPTYPE::STRSXP,
            GNU_OP_ISSYMBOL => TYPEOF(value) == SEXPTYPE::SYMSXP,
            GNU_OP_ISOBJECT => crate::sexp::accessors::OBJECT(value) != 0,
            GNU_OP_ISNUMERIC => {
                (TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::REALSXP)
                    && crate::mainutils::seq::inherits(value, c"factor".as_ptr()) == 0
            }
            _ => unreachable!(),
        };
        if test {
            crate::mainutils::relop::R_TrueValue()
        } else {
            crate::mainutils::relop::R_FalseValue()
        }
    }
}

/// GNU DOLLAR matches eval.c: dispatch `$` on objects, else R_subset3_dflt.
pub(crate) unsafe fn eval_gnu_dollar(call: SEXP, symbol: SEXP, x: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if crate::sexp::accessors::OBJECT(x) != 0 {
            let dollar = crate::sexp::symbol::Rf_install(c"$".as_ptr());
            let mut op = crate::mainutils::names::R_Primitive(c"$".as_ptr());
            if op.is_null() || op == R_NilValue() {
                op = crate::sexp::accessors::SYMVALUE(dollar);
            }
            let name = crate::sexp::constructors::Rf_ScalarString(PRINTNAME(symbol));
            let _name = own_operand(name);
            let args = Rf_cons(x, Rf_cons(name, R_NilValue()));
            let _args = own_operand(args);
            let ncall = crate::sexp::constructors::Rf_lang3(dollar, x, name);
            let _ncall = own_operand(ncall);
            let _ = call;
            return crate::mainutils::subset::do_subset3(ncall, op, args, rho);
        }
        crate::mainutils::subset::R_subset3_dflt(x, PRINTNAME(symbol), call)
    }
}

/// GNU DOLLARGETS matches eval.c: dispatch `$<-` on objects, else R_subassign3_dflt.
pub(crate) unsafe fn eval_gnu_dollargets(
    call: SEXP,
    symbol: SEXP,
    mut x: SEXP,
    rhs: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        if crate::sexp::accessors::NAMED(x) > 1 {
            x = crate::mainutils::duplicate::shallow_duplicate(x);
        }
        if crate::sexp::accessors::OBJECT(x) != 0
            && !call.is_null()
            && TYPEOF(call) == SEXPTYPE::LANGSXP
        {
            let ncall = crate::mainutils::duplicate::duplicate(call);
            let _ncall = own_operand(ncall);
            let name = crate::sexp::constructors::Rf_ScalarString(PRINTNAME(symbol));
            let _name = own_operand(name);
            let field_cell = crate::sexp::accessors::CDDR(ncall);
            if field_cell.is_null() || field_cell == R_NilValue() {
                let extra = Rf_cons(name, R_NilValue());
                let _extra = own_operand(extra);
                crate::sexp::accessors::SETCDR(crate::sexp::accessors::CDR(ncall), extra);
            } else {
                crate::sexp::accessors::SETCAR(field_cell, name);
            }
            let rhs_cell = crate::sexp::accessors::CDDDR(ncall);
            if !rhs_cell.is_null() && rhs_cell != R_NilValue() {
                let expr = crate::sexp::accessors::CAR(rhs_cell);
                let prom = crate::sexp::memory_ext::mkPROMSXP(expr, rho);
                crate::sexp::accessors::SET_PRVALUE(prom, rhs);
                crate::sexp::accessors::SETCAR(rhs_cell, prom);
            }
            let mut value = R_NilValue();
            if super::super::missing::tryDispatch(
                c"$<-".as_ptr() as *mut _,
                ncall,
                x,
                rho,
                &mut value,
            ) != 0
            {
                return value;
            }
        }
        crate::mainutils::subassign::R_subassign3_dflt(call, x, symbol, rhs)
    }
}

/// GNU INCREMENT_LINKS for NAMED-tracking values: raise NAMED toward the
/// maximum so in-place modification of the protected value duplicates.
pub(crate) unsafe fn increment_named_link(value: SEXP) {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return;
        }
        let current = crate::sexp::accessors::NAMED(value);
        if current < 2 {
            crate::sexp::accessors::SET_NAMED(value, current + 1);
        }
    }
}

/// GNU DECREMENT_LINKS: release one link taken by INCREMENT_LINKS.
pub(crate) unsafe fn decrement_named_link(value: SEXP) {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return;
        }
        let current = crate::sexp::accessors::NAMED(value);
        if current > 0 {
            crate::sexp::accessors::SET_NAMED(value, current - 1);
        }
    }
}

/// GNU `findVar` is unforced. `R_findVar` forces promises, which collapses a
/// supplied empty-name value into `R_MissingArg` and makes GETVAR treat
/// `recurse(formals(fn)[[i]])` as an unsupplied formal.
pub(crate) unsafe fn find_var_unforced(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let symbol = crate::sexp::object::Sexp::from_raw_unchecked(symbol);
        let rho = crate::sexp::object::Sexp::from_raw_unchecked(rho);
        match crate::sexp::envir::find_var_binding_result(symbol, rho) {
            Ok(Some(value)) => value.as_raw(),
            Ok(None) => R_UnboundValue(),
            Err(_) => R_UnboundValue(),
        }
    }
}

/// GNU GETVAR / GETVAR_MISSOK match eval.c getvar(keepmiss).
pub(crate) unsafe fn eval_gnu_getvar(
    symbol: SEXP,
    rho: SEXP,
    keep_missing: bool,
    dots: bool,
) -> SEXP {
    unsafe {
        let value = if dots
            && crate::sexp::envir::dd_val(crate::sexp::object::Sexp::from_raw_unchecked(symbol))
                .is_some()
        {
            let value = crate::sexp::envir::ddfindVar(symbol, rho);
            if value == R_UnboundValue() {
                bc_unbound_object_error(symbol, rho);
            }
            value
        } else {
            let mut value = find_var_unforced(symbol, rho);
            if value == R_UnboundValue() {
                if let Some(primitive) = super::super::eval::primitive_for_symbol(
                    crate::sexp::object::Sexp::from_raw_unchecked(symbol),
                ) {
                    value = primitive.as_raw();
                } else {
                    bc_unbound_object_error(symbol, rho);
                }
            }
            value
        };

        if value == R_MissingArg() {
            if keep_missing {
                return value;
            }
            bc_missing_arg_error(symbol);
        }
        if TYPEOF(value) == SEXPTYPE::DOTSXP {
            bc_error("'...' used in an invalid context");
        }
        if TYPEOF(value) == SEXPTYPE::PROMSXP {
            // missing() stays true for a formal that has a default. The
            // default is still the value: nmx[parametric] with
            // parametric = FALSE must see FALSE, not a missing index.
            let code = crate::sexp::accessors::PRCODE(value);
            if code == R_MissingArg() || code.is_null() {
                if keep_missing {
                    return R_MissingArg();
                }
                bc_missing_arg_error(symbol);
            }
            if keep_missing
                && TYPEOF(code) == SEXPTYPE::SYMSXP
                && crate::sexp::envir::R_missing(code, crate::sexp::accessors::PRENV(value)) != 0
            {
                return R_MissingArg();
            }
            forcePromise(value)
        } else {
            value
        }
    }
}
