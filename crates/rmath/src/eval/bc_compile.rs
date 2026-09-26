#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Minimal bytecode compiler — compiles simple expressions to BCODESXP.
//!
//! This provides enough of the GNU R compiler pipeline for `R_cmpfun`,
//! `R_compileExpr`, and JIT scoring to produce bytecode that `bcEval` can run.

use std::os::raw::c_int;

use super::bc_eval::opcodes;
use crate::sexp::accessors::{BODY, CAR, CDR, PRINTNAME, SET_BODY, TAG, TYPEOF};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_NilValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::memory::with_arena_in;
use crate::sexp::protect::protect;
use crate::sexp::symbol::R_DotsSymbol;

struct BytecodeCompiler {
    consts: Vec<SEXP>,
    code: Vec<c_int>,
    stack_hint: c_int,
    /// Enclosing compile environment. Nested function() constants capture
    /// this so formals resolve in the call frame and base operators remain
    /// visible through the parent chain. GNU MAKECLOSURE rebinds this at
    /// runtime; the private dialect's LDCLOSURE uses the compile-time env.
    rho: SEXP,
    /// Symbols assigned from a compiled function() in this body. Calls to
    /// those locals are lowered like eager builtins; other user calls stay
    /// rejected so user_fun(x) remains unsupported compiler syntax.
    compiled_local_funs: Vec<SEXP>,
}

impl BytecodeCompiler {
    fn new(rho: SEXP) -> Self {
        BytecodeCompiler {
            consts: Vec::new(),
            code: Vec::new(),
            stack_hint: 8,
            rho,
            compiled_local_funs: Vec::new(),
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
                t if t == SEXPTYPE::LGLSXP => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::INTSXP => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::REALSXP => {
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    true
                }
                t if t == SEXPTYPE::CPLXSXP || t == SEXPTYPE::STRSXP || t == SEXPTYPE::RAWSXP => {
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
                    // A missing subscript (`x[]`) is the R_MissingArg sentinel,
                    // not a variable lookup.
                    if expr == crate::sexp::globals::R_MissingArg() {
                        let idx = self.add_const(expr);
                        self.emit_operand(opcodes::OP_PUSHCONST, idx);
                        return true;
                    }
                    let idx = self.add_const(expr);
                    self.emit_operand(opcodes::OP_GETVAR, idx);
                    true
                }
                t if t == SEXPTYPE::CLOSXP => self.compile_closure_expr(expr),
                t if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::LISTSXP => self.compile_call(expr),
                _ => false,
            }
        }
    }

    /// Lower a function expression to a closure constant.  GNU's compiler
    unsafe fn compile_closure_expr(&mut self, expr: SEXP) -> bool {
        unsafe {
            let closure = crate::mainutils::duplicate::duplicate(expr);
            if closure.is_null() || TYPEOF(closure) != SEXPTYPE::CLOSXP {
                return false;
            }
            let _closure_guard = protect(closure);
            if TYPEOF(BODY(closure)) != SEXPTYPE::BCODESXP && !compile_closure(closure) {
                return false;
            }
            let fb = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 2);
            if fb.is_null() {
                return false;
            }
            let _fb_guard = protect(fb);
            crate::sexp::accessors::SET_VECTOR_ELT(fb, 0, crate::sexp::accessors::FORMALS(closure));
            crate::sexp::accessors::SET_VECTOR_ELT(fb, 1, crate::sexp::accessors::BODY(closure));
            let idx = self.add_const(fb);
            self.emit_operand(opcodes::OP_MAKECLOSURE, idx);
            true
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
                if name.as_deref() == Some("(") {
                    // GNU inlines `(`: compile the inner expression and
                    // keep the value visible.
                    let argument = CAR(CDR(expr));
                    return if argument.is_null() || argument == R_NilValue() {
                        false
                    } else {
                        self.compile_expr(argument) && {
                            self.emit(opcodes::OP_visible);
                            true
                        }
                    };
                }
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
                if matches!(name.as_deref(), Some("<-") | Some("=")) {
                    return self.compile_assignment(expr);
                }
                if name.as_deref() == Some("<<-") {
                    return self.compile_superassignment(expr);
                }
                if name.as_deref() == Some("function") {
                    return self.compile_function_expr(expr);
                }
                let local_fun = self.is_compiled_local_fun(fun);
                if !name.as_deref().is_some_and(is_eager_builtin_call) && !local_fun {
                    return false;
                }
                let mut arg_cells = Vec::new();
                let mut cur = CDR(expr);
                while !cur.is_null() && cur != R_NilValue() {
                    arg_cells.push(cur);
                    cur = CDR(cur);
                }
                // Closure calls receive lazy promises like GNU MAKEPROM:
                // `g <- function(y) 5; g(stop("boom"))` must not evaluate
                // its arguments.  Constants stay eager values
                // (PUSHCONSTARG semantics).
                let subset = matches!(name.as_deref(), Some("[") | Some("[<-") | Some("[[") | Some("[[<-"));
                for (arg_index, cell) in arg_cells.iter().enumerate() {
                    let argument = CAR(*cell);
                    let lazy_ok = local_fun
                        && !matches!(TYPEOF(argument), 0 | 10 | 13 | 14 | 15 | 16 | 24);
                    if lazy_ok {
                        let idx = self.add_const(argument);
                        self.emit_operand(opcodes::OP_MAKEPROMISE, idx);
                    } else if !self.compile_expr(argument) {
                        return false;
                    }
                    let later_call = arg_cells.iter().skip(arg_index + 1).any(|later| {
                        let value = unsafe { CAR(*later) };
                        !value.is_null() && TYPEOF(value) == SEXPTYPE::LANGSXP
                    });
                    let is_object = subset && arg_index == 0;
                    if later_call && !is_object && TYPEOF(argument) == SEXPTYPE::SYMSXP {
                        self.emit(opcodes::OP_MARK_SHARED);
                    }
                    let tag = TAG(*cell);
                    if !tag.is_null() && tag != R_NilValue() {
                        let tag_idx = self.add_const(tag);
                        self.emit_operand(opcodes::OP_SETTAG, tag_idx);
                    }
                }
                let fun_idx = self.add_const(fun);
                self.emit_operand(opcodes::OP_PUSHFUN, fun_idx);
                self.emit_operand(opcodes::OP_CALL, arg_cells.len() as c_int);
                true
            } else {
                return false;
            }
        }
    }

    /// Lower parsed `function(formals, body)` syntax to GNU-style
    /// MAKECLOSURE: the runtime frame becomes the closure environment,
    /// exactly like eval.c OP(MAKECLOSURE) — a compile-time environment
    /// would break lexical capture of enclosing locals.
    unsafe fn compile_function_expr(&mut self, expr: SEXP) -> bool {
        unsafe {
            let formals_cell = CDR(expr);
            let body_cell = if !formals_cell.is_null() {
                CDR(formals_cell)
            } else {
                std::ptr::null_mut()
            };
            if formals_cell.is_null()
                || formals_cell == R_NilValue()
                || body_cell.is_null()
                || body_cell == R_NilValue()
            {
                return false;
            }
            // Compile through a scratch closure so the nested body shares
            // the BCODESXP pipeline, then store formals/body in the
            // constant vector MAKECLOSURE consumes.
            let scratch = crate::mainutils::dstruct::mkCLOSXP(
                CAR(formals_cell),
                CAR(body_cell),
                R_BaseEnv(),
            );
            if scratch.is_null() {
                return false;
            }
            let _scratch_guard = protect(scratch);
            if !compile_closure(scratch) {
                return false;
            }
            let fb = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 2);
            if fb.is_null() {
                return false;
            }

            let _fb_guard = protect(fb);
            crate::sexp::accessors::SET_VECTOR_ELT(fb, 0, crate::sexp::accessors::FORMALS(scratch));
            crate::sexp::accessors::SET_VECTOR_ELT(fb, 1, crate::sexp::accessors::BODY(scratch));
            let idx = self.add_const(fb);
            self.emit_operand(opcodes::OP_MAKECLOSURE, idx);
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
            self.emit(opcodes::OP_POP);
            self.emit_operand(opcodes::OP_GOTO, test_label);
            let end_label = self.code.len() as c_int;
            self.code[brif_idx as usize + 1] = end_label;
            let nil_idx = self.add_const(R_NilValue());
            self.emit_operand(opcodes::OP_PUSHCONST, nil_idx);
            // Upstream's compiler wraps while-loop results in INVISIBLE
            // (the loop's NULL result never auto-prints at top level).
            self.emit(opcodes::OPinvisible);
            true
        }
    }

    unsafe fn compile_for(&mut self, expr: SEXP) -> bool {
        unsafe {
            // for (symbol in sequence) body
            let symbol = CAR(CDR(expr));
            let sequence = CAR(CDR(CDR(expr)));
            let body = CAR(CDR(CDR(CDR(expr))));
            if TYPEOF(symbol) != SEXPTYPE::SYMSXP || !self.compile_expr(sequence) {
                return false;
            }

            let symbol_idx = self.add_const(symbol);
            let start_idx = self.code.len();
            self.emit(opcodes::OP_STARTFOR);
            self.code.push(symbol_idx);
            self.code.push(0); // end label, patched below

            let body_start = self.code.len() as c_int;
            if !self.compile_expr(body) {
                return false;
            }
            self.emit(opcodes::OP_POP);
            self.emit_operand(opcodes::OP_NEXTFOR, body_start);

            self.code[start_idx + 2] = self.code.len() as c_int;
            self.emit(opcodes::OPinvisible);
            true
        }
    }

    unsafe fn compile_assignment(&mut self, expr: SEXP) -> bool {
        unsafe {
            let lhs = CAR(CDR(expr));
            let rhs = CAR(CDR(CDR(expr)));
            if TYPEOF(lhs) == SEXPTYPE::LANGSXP {
                return self.compile_subassign(expr, false);
            }
            let binds_compiled_fun = is_function_syntax(rhs);
            if TYPEOF(lhs) != SEXPTYPE::SYMSXP || !self.compile_expr(rhs) {
                return false;
            }
            if binds_compiled_fun {
                self.compiled_local_funs.push(lhs);
            }
            let symbol_idx = self.add_const(lhs);
            self.emit_operand(opcodes::OP_SETVAR, symbol_idx);
            true
        }
    }

    /// `x[] <- rhs` is `` `<-`(`[`(x, ...), rhs) ``. The value is the
    /// modified object, which for a full replacement of a length-1 vector
    /// is the RHS, and the symbol is written back.
    unsafe fn compile_subassign(&mut self, expr: SEXP, superassign: bool) -> bool {
        unsafe {
            let lhs = CAR(CDR(expr));
            let rhs = CAR(CDR(CDR(expr)));
            if TYPEOF(lhs) != SEXPTYPE::LANGSXP {
                return false;
            }
            let double = match symbol_name_from_sexp(CAR(lhs)).as_deref() {
                Some("[") => false,
                Some("[[") => true,
                _ => return false,
            };
            let object = CAR(CDR(lhs));
            if TYPEOF(object) != SEXPTYPE::SYMSXP || !self.compile_expr(object) {
                return false;
            }
            // A subscript that runs code (`x[{x[2] <<- 3; 1}] <<- 2`) must
            // see the fetched object as shared, or the inner assign mutates
            // the value the outer update writes back. Constant indexes do not.
            let mut scan = CDR(CDR(lhs));
            let mut constant_indexes = true;
            while !scan.is_null() && scan != R_NilValue() {
                let sub = CAR(scan);
                let ty = TYPEOF(sub);
                let constant = ty == SEXPTYPE::INTSXP
                    || ty == SEXPTYPE::REALSXP
                    || ty == SEXPTYPE::LGLSXP
                    || ty == SEXPTYPE::NILSXP;
                if !constant {
                    constant_indexes = false;
                    break;
                }
                scan = CDR(scan);
            }
            if !constant_indexes {
                self.emit(opcodes::OP_BUMP_LINK);
            }
            let mut index = CDR(CDR(lhs));
            let mut n_index = 0;
            while !index.is_null() && index != R_NilValue() {
                n_index += 1;
                if !self.compile_expr(CAR(index)) {
                    return false;
                }
                index = CDR(index);
            }
            if !constant_indexes {
                self.emit_operand(opcodes::OP_DROP_LINK, n_index);
            }
            if n_index == 0 {
                let missing = crate::sexp::globals::R_MissingArg();
                let missing_idx = self.add_const(missing);
                self.emit_operand(opcodes::OP_PUSHCONST, missing_idx);
                n_index = 1;
            }
            if !self.compile_expr(rhs) {
                return false;
            }
            let op_sym = crate::sexp::symbol::Rf_install(if double {
                c"[[<-".as_ptr()
            } else {
                c"[<-".as_ptr()
            });
            let fun_idx = self.add_const(op_sym);
            self.emit_operand(opcodes::OP_PUSHFUN, fun_idx);
            self.emit_operand(opcodes::OP_CALL, n_index + 2);
            let symbol_idx = self.add_const(object);
            if superassign {
                self.emit_operand(opcodes::OP_SETVAR2, symbol_idx);
            } else {
                self.emit_operand(opcodes::OP_SETVAR, symbol_idx);
            }
            true
        }
    }


    /// `<<-`: compile the value, then store into the enclosing frame via
    /// OP_SETVAR2 (eval.c SETVAR2 semantics; the value stays on the stack).
    unsafe fn compile_superassignment(&mut self, expr: SEXP) -> bool {
        unsafe {
            let lhs = CAR(CDR(expr));
            let rhs = CAR(CDR(CDR(expr)));
            if TYPEOF(lhs) == SEXPTYPE::LANGSXP {
                return self.compile_subassign(expr, true);
            }
            if TYPEOF(lhs) != SEXPTYPE::SYMSXP || !self.compile_expr(rhs) {
                return false;
            }
            let symbol_idx = self.add_const(lhs);
            self.emit_operand(opcodes::OP_SETVAR2, symbol_idx);
            true
        }
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

    fn is_compiled_local_fun(&self, fun: SEXP) -> bool {
        self.compiled_local_funs.contains(&fun)
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
                    for (index, constant) in self.consts.iter().enumerate() {
                        *consts_data.add(index + 1) = *constant;
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
            | "!"
            | "*"
            | "/"
            | "^"
            | "%%"
            | "%/%"
            | "%in%"
            | "<"
            | "<="
            | "=="
            | "!="
            | ">="
            | ">"
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
            | "rep"
            | "round"
            | ":"
            | "["
            | "[<-"
            | "[["
            | "[[<-"
    )
}

unsafe fn is_function_syntax(expr: SEXP) -> bool {
    unsafe {
        !expr.is_null()
            && expr != R_NilValue()
            && TYPEOF(expr) == SEXPTYPE::LANGSXP
            && symbol_name_from_sexp(CAR(expr)).as_deref() == Some("function")
    }
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
        let mut compiler = BytecodeCompiler::new(_rho);
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
        let session = RSession::new();
        let expr = session.with_active(|| unsafe { Rf_ScalarInteger(42) });
        let env = session.global_env().expect("global env");

        unsafe {
            let bcode = compile_expr(expr, env.clone().as_raw()).expect("constant should compile");
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
            defineVar(
                Rf_install(c"x".as_ptr()),
                Rf_ScalarInteger(9),
                env.clone().as_raw(),
            );
            let sym = Rf_install(c"x".as_ptr());
            let bcode = compile_expr(sym, env.clone().as_raw()).expect("symbol should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.as_raw());
            assert_eq!(*INTEGER(result), 9);
        }
    }

    #[test]
    fn compile_simple_call_round_trips_through_bc_eval() {
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            defineVar(
                Rf_install(c"x".as_ptr()),
                Rf_ScalarInteger(5),
                env.clone().as_raw(),
            );
            let call = Rf_cons(
                Rf_install(c"+".as_ptr()),
                Rf_cons(
                    Rf_install(c"x".as_ptr()),
                    Rf_cons(Rf_ScalarInteger(1), R_NilValue()),
                ),
            );
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let bcode = compile_expr(call, env.clone().as_raw()).expect("addition should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.as_raw());
            assert_eq!(*INTEGER(result), 6);
        }
    }

    #[test]
    fn compile_logical_not_round_trips_through_bc_eval() {
        // Pinned GNU R oracle (`compiler::cmpfun(function(x) !x)` applied to
        // FALSE) returns TRUE; this exercises the same supported expression
        // through the portable private bytecode evaluator.
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            let call = Rf_cons(
                Rf_install(c"!".as_ptr()),
                Rf_cons(
                    crate::sexp::constructors::Rf_ScalarLogical(crate::sexp::ffi::FALSE),
                    R_NilValue(),
                ),
            );
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let bcode = compile_expr(call, env.clone().as_raw()).expect("! should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.as_raw());
            assert_eq!(
                *crate::sexp::accessors::LOGICAL(result),
                crate::sexp::ffi::TRUE
            );
        }
    }

    #[test]
    fn compile_assignment_block_round_trips_through_bc_eval() {
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

            let bcode = compile_expr(block, env.clone().as_raw()).expect("block should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.clone().as_raw());
            assert_eq!(*INTEGER(result), 1);
            assert_eq!(
                *INTEGER(crate::sexp::envir::R_findVar(
                    Rf_install(c"x".as_ptr()),
                    env.as_raw()
                )),
                1
            );
        }
    }

    #[test]
    fn compile_for_loop_updates_binding_and_returns_invisible_null() {
        let session = RSession::new();
        let env = session.global_env().expect("global env").as_raw();

        session.with_active(|| unsafe {
            let sequence = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            let values = INTEGER(sequence);
            *values = 1;
            *values.add(1) = 2;
            *values.add(2) = 3;

            let sum = Rf_install(c"sum".as_ptr());
            defineVar(sum, Rf_ScalarInteger(0), env);
            let add = Rf_cons(
                Rf_install(c"+".as_ptr()),
                Rf_cons(sum, Rf_cons(Rf_install(c"i".as_ptr()), R_NilValue())),
            );
            (*add).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let assign = Rf_cons(
                Rf_install(c"<-".as_ptr()),
                Rf_cons(sum, Rf_cons(add, R_NilValue())),
            );
            (*assign).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let for_call = Rf_cons(
                Rf_install(c"for".as_ptr()),
                Rf_cons(
                    Rf_install(c"i".as_ptr()),
                    Rf_cons(sequence, Rf_cons(assign, R_NilValue())),
                ),
            );
            (*for_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);

            let bcode = compile_expr(for_call, env).expect("for loop should compile");
            let result = super::super::bc_eval::bcEval(bcode, env);
            assert_eq!(result, R_NilValue());
            assert_eq!(*INTEGER(crate::sexp::envir::R_findVar(sum, env)), 6);

            let empty = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::INTSXP, 0);
            let empty_for = Rf_cons(
                Rf_install(c"for".as_ptr()),
                Rf_cons(
                    Rf_install(c"i".as_ptr()),
                    Rf_cons(empty, Rf_cons(R_NilValue(), R_NilValue())),
                ),
            );
            (*empty_for).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let empty_bcode = compile_expr(empty_for, env).expect("empty for loop should compile");
            assert_eq!(
                super::super::bc_eval::bcEval(empty_bcode, env),
                R_NilValue()
            );
            assert_eq!(
                crate::sexp::envir::R_findVar(Rf_install(c"i".as_ptr()), env),
                R_NilValue()
            );
        });
    }

    #[test]
    fn compile_while_loop_keeps_operand_stack_balanced() {
        let session = RSession::new();
        let env = session.global_env().expect("global env");

        unsafe {
            let counter = Rf_install(c"counter".as_ptr());
            defineVar(counter, Rf_ScalarInteger(0), env.clone().as_raw());

            let condition = Rf_cons(
                Rf_install(c"<".as_ptr()),
                Rf_cons(counter, Rf_cons(Rf_ScalarInteger(1_000), R_NilValue())),
            );
            (*condition).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let increment = Rf_cons(
                Rf_install(c"+".as_ptr()),
                Rf_cons(counter, Rf_cons(Rf_ScalarInteger(1), R_NilValue())),
            );
            (*increment).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let assign = Rf_cons(
                Rf_install(c"<-".as_ptr()),
                Rf_cons(counter, Rf_cons(increment, R_NilValue())),
            );
            (*assign).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let while_call = Rf_cons(
                Rf_install(c"while".as_ptr()),
                Rf_cons(condition, Rf_cons(assign, R_NilValue())),
            );
            (*while_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);

            let bcode =
                compile_expr(while_call, env.clone().as_raw()).expect("while loop should compile");
            let result = super::super::bc_eval::bcEval(bcode, env.clone().as_raw());
            assert_eq!(result, R_NilValue());
            assert_eq!(
                *INTEGER(crate::sexp::envir::R_findVar(counter, env.as_raw())),
                1_000
            );
        }
    }
}
