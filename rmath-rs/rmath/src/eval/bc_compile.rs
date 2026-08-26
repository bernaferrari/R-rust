#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Minimal bytecode compiler — compiles simple expressions to BCODESXP.
//!
//! This provides enough of the GNU R compiler pipeline for `R_cmpfun`,
//! `R_compileExpr`, and JIT scoring to produce bytecode that `bcEval` can run.

use std::os::raw::c_int;

use super::bc_eval::opcodes;
use crate::sexp::accessors::{BODY, CAR, CDR, LENGTH, PRINTNAME, SET_BODY, TYPEOF};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::memory::with_arena_in;
use crate::sexp::protect::protect;
use crate::sexp::symbol::R_DotsSymbol;

struct BytecodeCompiler {
    consts: Vec<SEXP>,
    code: Vec<c_int>,
    stack_hint: c_int,
}

impl BytecodeCompiler {
    fn new() -> Self {
        BytecodeCompiler {
            consts: Vec::new(),
            code: Vec::new(),
            stack_hint: 8,
        }
    }

    unsafe fn add_const(&mut self, value: SEXP) -> c_int {
        // Bytecode constant operands skip slot 0, which stores the source expr.
        let idx = (self.consts.len() + 1) as c_int;
        self.consts.push(value);
        idx
    }

    fn emit(&mut self, opcode: c_int) {
        self.code.push(opcode);
    }

    fn emit_operand(&mut self, opcode: c_int, operand: c_int) {
        self.emit(opcode);
        self.code.push(operand);
    }

    unsafe fn compile_expr(&mut self, expr: SEXP) -> bool {
        unsafe {
            if expr.is_null() || expr == R_NilValue() {
                let idx = self.add_const(R_NilValue());
                self.emit_operand(opcodes::OP_PUSHCONST, idx);
                return true;
            }

            match TYPEOF(expr) {
                t if t == SEXPTYPE::LGLSXP && LENGTH(expr) == 1 => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::INTSXP && LENGTH(expr) == 1 => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::REALSXP && LENGTH(expr) == 1 => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::STRSXP && LENGTH(expr) == 1 => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::SYMSXP => {
                    // `...` must never compile to OP_GETVAR: the DOTSXP frame
                    // binding is spliced by the AST evaluator (dispatch.rs
                    // evalList/promiseArgs), not fetched as an ordinary value
                    // (mirrors GNU R, where the compiler never emits GETVAR
                    // for R_DotsSymbol). Leave the expression on the AST path.
                    if expr == R_DotsSymbol() {
                        return false;
                    }
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_GETVAR, idx);
                    true
                }
                t if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::LISTSXP => self.compile_call(expr),
                _ => false,
            }
        }
    }

    unsafe fn compile_call(&mut self, expr: SEXP) -> bool {
        unsafe {
            let fun = CAR(expr);
            if fun.is_null() {
                return false;
            }

            if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                let name = symbol_name_from_sexp(fun);
                if name.as_deref() == Some("{") {
                    return self.compile_block(expr);
                }
                if name.as_deref() == Some("if") {
                    return self.compile_if(expr);
                }
                if name.as_deref() == Some("while") {
                    return self.compile_while(expr);
                }
                if name.as_deref() == Some("for") {
                    return self.compile_for(expr);
                }
                if !name.as_deref().is_some_and(is_eager_builtin_call) {
                    return false;
                }
            } else {
                return false;
            }

            let mut arg_cells = Vec::new();
            let mut cur = CDR(expr);
            while !cur.is_null() && cur != R_NilValue() {
                arg_cells.push(cur);
                cur = CDR(cur);
            }

            for cell in &arg_cells {
                if !self.compile_expr(CAR(*cell)) {
                    return false;
                }
            }

            let fun_idx = self.add_const(fun);
            self.emit_operand(opcodes::OP_PUSHFUN, fun_idx);
            self.emit_operand(opcodes::OP_CALL, arg_cells.len() as c_int);
            true
        }
    }

    unsafe fn compile_while(&mut self, expr: SEXP) -> bool {
        unsafe {
            // while (test) body
            let test = CAR(CDR(expr));
            let body = CAR(CDR(CDR(expr)));
            let test_label = self.code.len() as c_int;
            if !self.compile_expr(test) {
                return false;
            }
            let brif_idx = self.code.len() as c_int;
            self.emit_operand(opcodes::OP_BRIFNOT, 0); // placeholder
            if !self.compile_expr(body) {
                return false;
            }
            self.emit_operand(opcodes::OP_GOTO, test_label);
            let end_label = self.code.len() as c_int;
            self.code[brif_idx as usize + 1] = end_label;
            true
        }
    }

    unsafe fn compile_for(&mut self, expr: SEXP) -> bool {
        // The bytecode VM does not yet model R's full for-loop state
        // (sequence, binding cell, reusable scalar value, break/next
        // context). Keep for-loops on the AST evaluator rather than
        // emitting bytecode that silently skips or mistypes iterations.
        let _ = expr;
        false
    }

    unsafe fn compile_if(&mut self, expr: SEXP) -> bool {
        unsafe {
            // if (test) then else ; form is lang if test then else
            let test = CAR(CDR(expr));
            let then_e = CAR(CDR(CDR(expr)));
            let else_e = if CDR(CDR(CDR(expr))).is_null() {
                R_NilValue()
            } else {
                CAR(CDR(CDR(CDR(expr))))
            };

            if !self.compile_expr(test) {
                return false;
            }
            let brif_idx = self.code.len() as c_int;
            self.emit_operand(opcodes::OP_BRIFNOT, 0); // placeholder target

            if !self.compile_expr(then_e) {
                return false;
            }
            let goto_idx = self.code.len() as c_int;
            self.emit_operand(opcodes::OP_GOTO, 0); // placeholder

            let else_label = self.code.len() as c_int;
            self.code[brif_idx as usize + 1] = else_label; // fix BRIFNOT target

            if !self.compile_expr(else_e) {
                return false;
            }
            let end_label = self.code.len() as c_int;
            self.code[goto_idx as usize + 1] = end_label; // fix GOTO target

            true
        }
    }

    unsafe fn compile_block(&mut self, expr: SEXP) -> bool {
        unsafe {
            let mut exprs = Vec::new();
            let mut cur = CDR(expr);
            while !cur.is_null() && cur != R_NilValue() {
                exprs.push(CAR(cur));
                cur = CDR(cur);
            }

            if exprs.is_empty() {
                let idx = self.add_const(R_NilValue());
                self.emit_operand(opcodes::OP_PUSHCONST, idx);
                return true;
            }

            let last = exprs.len().saturating_sub(1);
            for (index, body) in exprs.into_iter().enumerate() {
                if !self.compile_expr(body) {
                    return false;
                }
                if index != last {
                    self.emit(opcodes::OP_POP);
                }
            }
            true
        }
    }

    unsafe fn finish(&mut self, source_expr: SEXP) -> SEXP {
        unsafe {
            self.emit(opcodes::OP_RETURN);
            with_required_current_instance(|inst| unsafe {
                with_arena_in(inst, |arena| {
                    let consts =
                        arena.alloc_vector(SEXPTYPE::VECSXP, (self.consts.len() + 1) as i64);
                    let consts_data = (*consts).gengc_next_node as *mut SEXP;
                    *consts_data = source_expr;
                    for (index, value) in self.consts.iter().enumerate() {
                        *consts_data.add(index + 1) = *value;
                    }

                    let code = arena.alloc_vector(SEXPTYPE::INTSXP, self.code.len() as i64);
                    let code_data = (*code).gengc_next_node as *mut c_int;
                    for (index, instruction) in self.code.iter().enumerate() {
                        *code_data.add(index) = *instruction;
                    }

                    let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                    let stack_data = (*stack_hint).gengc_next_node as *mut c_int;
                    *stack_data = self.stack_hint.max(4);

                    let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
                    let bcode_data = (*bcode).gengc_next_node as *mut SEXP;
                    *bcode_data = code;
                    *bcode_data.add(1) = consts;
                    *bcode_data.add(2) = stack_hint;
                    bcode
                })
            })
        }
    }
}

fn is_eager_builtin_call(name: &str) -> bool {
    matches!(
        name,
        "+" | "-"
            | "*"
            | "/"
            | "^"
            | "%%"
            | "%/%"
            | "%in%"
            | "c"
            | "list"
            | "abs"
            | "sqrt"
            | "log"
            | "exp"
            | "sum"
            | "prod"
            | "min"
            | "max"
            | "length"
            | "is.null"
            | "is.na"
            | "is.numeric"
            | "is.integer"
            | "is.double"
            | "is.logical"
            | "is.character"
            | "as.integer"
            | "as.numeric"
            | "as.character"
            | "as.logical"
            | "match"
            | "paste"
            | "paste0"
            | "seq"
            | "seq_len"
            | "seq_along"
    )
}

unsafe fn symbol_name_from_sexp(sym: SEXP) -> Option<String> {
    unsafe {
        if sym.is_null() || TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return None;
        }
        let pname = PRINTNAME(sym);
        if pname.is_null() {
            return None;
        }
        let bytes = crate::sexp::accessors::CHAR(pname);
        if bytes.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(bytes)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Try to compile `expr` to bytecode. Returns `None` when the expression is too
/// complex for the minimal compiler.
pub unsafe fn compile_expr(expr: SEXP, _rho: SEXP) -> Option<SEXP> {
    unsafe {
        let mut compiler = BytecodeCompiler::new();
        if !compiler.compile_expr(expr) {
            return None;
        }
        Some(compiler.finish(expr))
    }
}

/// Try to compile a closure body and install bytecode on success.
pub unsafe fn compile_closure(fun: SEXP) -> bool {
    unsafe {
        if fun.is_null() || TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return false;
        }
        let body = BODY(fun);
        if body.is_null() || TYPEOF(body) == SEXPTYPE::BCODESXP {
            return false;
        }
        let cloenv = crate::sexp::accessors::CLOENV(fun);
        let Some(bcode) = compile_expr(body, cloenv) else {
            return false;
        };
        let _guard = protect(bcode);
        SET_BODY(fun, bcode);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::INTEGER;
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_cons};
    use crate::sexp::envir::defineVar;
    use crate::sexp::session::RSession;
    use crate::sexp::symbol::Rf_install;

    #[test]
    fn compile_constant_round_trips_through_bc_eval() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|_| unsafe { Rf_ScalarInteger(42) })
            .expect("session active");
        let env = session.global_env().expect("global env");

        unsafe {
            let bcode = compile_expr(expr, env.as_raw()).expect("constant should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.as_raw());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 42);
        }
    }

    #[test]
    fn compile_getvar_round_trips_through_bc_eval() {
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            defineVar(Rf_install(c"x".as_ptr()), Rf_ScalarInteger(9), env.as_raw());
            let sym = Rf_install(c"x".as_ptr());
            let bcode = compile_expr(sym, env.as_raw()).expect("symbol should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.as_raw());
            assert_eq!(*INTEGER(result), 9);
        }
    }

    #[test]
    fn compile_simple_call_round_trips_through_bc_eval() {
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            defineVar(Rf_install(c"x".as_ptr()), Rf_ScalarInteger(5), env.as_raw());
            let call = Rf_cons(
                Rf_install(c"+".as_ptr()),
                Rf_cons(
                    Rf_install(c"x".as_ptr()),
                    Rf_cons(Rf_ScalarInteger(1), R_NilValue()),
                ),
            );
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let bcode = compile_expr(call, env.as_raw()).expect("addition should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.as_raw());
            assert_eq!(*INTEGER(result), 6);
        }
    }

    #[test]
    fn compile_assignment_block_is_rejected() {
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            let assign = Rf_cons(
                Rf_install(c"<-".as_ptr()),
                Rf_cons(
                    Rf_install(c"x".as_ptr()),
                    Rf_cons(Rf_ScalarInteger(1), R_NilValue()),
                ),
            );
            (*assign).sxpinfo.set_type(SEXPTYPE::LANGSXP);

            let block = Rf_cons(
                Rf_install(c"{".as_ptr()),
                Rf_cons(assign, Rf_cons(Rf_install(c"x".as_ptr()), R_NilValue())),
            );
            (*block).sxpinfo.set_type(SEXPTYPE::LANGSXP);

            assert!(compile_expr(assign, env.as_raw()).is_none());
            assert!(compile_expr(block, env.as_raw()).is_none());
        }
    }

    #[test]
    fn compile_for_loop_is_rejected_until_vm_loop_state_is_complete() {
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            let for_call = Rf_cons(
                Rf_install(c"for".as_ptr()),
                Rf_cons(
                    Rf_install(c"i".as_ptr()),
                    Rf_cons(
                        Rf_ScalarInteger(1),
                        Rf_cons(Rf_install(c"i".as_ptr()), R_NilValue()),
                    ),
                ),
            );
            (*for_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);

            assert!(compile_expr(for_call, env.as_raw()).is_none());
        }
    }
}
