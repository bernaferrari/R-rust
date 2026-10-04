#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Minimal bytecode compiler — compiles simple expressions to BCODESXP.
//!
//! This provides enough of the GNU R compiler pipeline for `R_cmpfun`,
//! `R_compileExpr`, and JIT scoring to produce bytecode that `bcEval` can run.

use std::os::raw::c_int;

use super::bc_eval::opcodes;
use super::bc_stack::own_operand;
use crate::sexp::accessors::{BODY, CAR, CDR, PRINTNAME, SET_BODY, TAG, TYPEOF};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_NilValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::memory::with_arena_in;
use crate::sexp::object::{Sexp, SexpMut};
use crate::sexp::symbol::R_DotsSymbol;

fn compiler_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

struct BytecodeCompiler {
    consts: Vec<Sexp<'static>>,
    code: Vec<c_int>,
    stack_hint: c_int,
    /// Enclosing compile environment. Nested function() constants capture
    /// this so formals resolve in the call frame and base operators remain
    /// visible through the parent chain. GNU MAKECLOSURE rebinds this at
    /// runtime; the private dialect's LDCLOSURE uses the compile-time env.
    rho: Sexp<'static>,
    /// Symbols assigned from a compiled function() in this body. Calls to
    /// those locals are lowered like eager builtins; other user calls stay
    /// rejected so user_fun(x) remains unsupported compiler syntax.
    compiled_local_funs: Vec<Sexp<'static>>,
}

impl BytecodeCompiler {
    unsafe fn new(rho: SEXP) -> Self {
        BytecodeCompiler {
            consts: Vec::new(),
            code: Vec::new(),
            stack_hint: 8,
            rho: unsafe { own_operand(rho) },
            compiled_local_funs: Vec::new(),
        }
    }

    unsafe fn add_const(&mut self, value: SEXP) -> c_int {
        // Bytecode constant operands skip slot 0, which stores the source expr.
        let idx = self
            .consts
            .len()
            .checked_add(1)
            .and_then(|n| c_int::try_from(n).ok())
            .unwrap_or_else(|| compiler_error("bytecode constant pool is too large"));
        let value = unsafe { own_operand(value) };
        self.consts
            .try_reserve(1)
            .unwrap_or_else(|_| compiler_error("cannot grow bytecode constant pool"));
        self.consts.push(value);
        idx
    }

    fn emit(&mut self, opcode: c_int) {
        if self.code.len() >= c_int::MAX as usize {
            compiler_error("bytecode instruction stream is too long");
        }
        self.code
            .try_reserve(1)
            .unwrap_or_else(|_| compiler_error("cannot grow bytecode instruction stream"));
        self.code.push(opcode);
    }

    fn emit_operand(&mut self, opcode: c_int, operand: c_int) {
        self.emit(opcode);
        self.emit(operand);
    }

    unsafe fn compile_expr(&mut self, expr: SEXP) -> bool {
        unsafe {
            let expr_owned = own_operand(if expr.is_null() { R_NilValue() } else { expr });
            let expr = expr_owned.as_raw();
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
            let _closure_guard = own_operand(closure);
            if TYPEOF(BODY(closure)) != SEXPTYPE::BCODESXP && !compile_closure(closure) {
                return false;
            }
            let fb = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 2);
            if fb.is_null() {
                return false;
            }
            let _fb_guard = own_operand(fb);
            crate::sexp::accessors::SET_VECTOR_ELT(fb, 0, crate::sexp::accessors::FORMALS(closure));
            crate::sexp::accessors::SET_VECTOR_ELT(fb, 1, crate::sexp::accessors::BODY(closure));
            let idx = self.add_const(fb);
            self.emit_operand(opcodes::OP_MAKECLOSURE, idx);
            true
        }
    }

    unsafe fn compile_call(&mut self, expr: SEXP) -> bool {
        unsafe {
            let fun_owned = own_operand(CAR(expr));

            let fun = fun_owned.as_raw();
            if fun.is_null() {
                return false;
            }
            if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                let name = symbol_name_from_sexp(fun);
                if name.as_deref() == Some("(") {
                    // GNU inlines `(`: compile the inner expression and
                    // keep the value visible.
                    let argument_owned = own_operand(CAR(CDR(expr)));

                    let argument = argument_owned.as_raw();
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
                let eager = name.as_deref().is_some_and(is_eager_builtin_call);
                let is_missing = name.as_deref() == Some("missing");
                let is_internal = name.as_deref() == Some(".Internal");
                let is_at = name.as_deref() == Some("@") || name.as_deref() == Some("$");
                let syntax_call = is_internal
                    || is_missing
                    || is_at
                    || name.as_deref().is_some_and(|name| {
                        super::primitive::fun_tab_index_by_name(name).is_some_and(|index| {
                            super::primitive::primitive_kind_for_eval(
                                crate::mainutils::names::R_FunTab[index as usize].eval,
                            ) == SEXPTYPE::SPECIALSXP
                        })
                    });
                let mut arg_cells = Vec::new();
                let mut cur = CDR(expr);
                let mut seen_cur = std::collections::HashSet::new();
                while !cur.is_null() && cur != R_NilValue() {
                    seen_cur
                        .try_reserve(1)
                        .unwrap_or_else(|_| compiler_error("cannot track bytecode source list"));
                    if !seen_cur.insert(cur.addr()) {
                        compiler_error("cyclic bytecode source list");
                    }
                    arg_cells.push(own_operand(cur));
                    cur = CDR(cur);
                }
                for cell in &arg_cells {
                    if CAR(cell.as_raw()) == R_DotsSymbol() {
                        return false;
                    }
                }
                // Closure calls receive lazy promises like GNU MAKEPROM.
                // An unsupplied argument is the R_MissingArg sentinel, not a
                // promise (a promise makes missing() false). .Internal must
                // see the call, not a promise of it.
                let subset = matches!(
                    name.as_deref(),
                    Some("[") | Some("[<-") | Some("[[") | Some("[[<-")
                );
                for (arg_index, cell) in arg_cells.iter().enumerate() {
                    let argument_owned = own_operand(CAR(cell.as_raw()));

                    let argument = argument_owned.as_raw();
                    let constant = matches!(TYPEOF(argument), 0 | 10 | 13 | 14 | 15 | 16 | 24);
                    let missing_arg = argument == crate::sexp::globals::R_MissingArg();
                    if missing_arg || syntax_call {
                        let idx = self.add_const(argument);
                        self.emit_operand(opcodes::OP_PUSHCONST, idx);
                    } else if (!eager || local_fun) && !constant {
                        let code =
                            if TYPEOF(argument) == SEXPTYPE::SYMSXP && argument != R_DotsSymbol() {
                                symbol_getvar_bcode(argument)
                            } else {
                                argument
                            };
                        let idx = self.add_const(code);
                        self.emit_operand(opcodes::OP_MAKEPROMISE, idx);
                    } else if !self.compile_expr(argument) {
                        return false;
                    }
                    let later = arg_index + 1 < arg_cells.len();
                    let is_object = subset && arg_index == 0;
                    if later && !is_object && TYPEOF(argument) == SEXPTYPE::SYMSXP {
                        self.emit(opcodes::OP_MARK_SHARED);
                    }
                    let tag = TAG(cell.as_raw());
                    if !tag.is_null() && tag != R_NilValue() {
                        let tag_idx = self.add_const(tag);
                        self.emit_operand(opcodes::OP_SETTAG, tag_idx);
                    }
                }
                let fun_idx = self.add_const(fun);
                self.emit_operand(opcodes::OP_PUSHFUN, fun_idx);
                self.emit_operand(
                    if syntax_call {
                        opcodes::OP_CALLSPECIAL
                    } else {
                        opcodes::OP_CALL
                    },
                    arg_cells.len() as c_int,
                );
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
            let scratch =
                crate::mainutils::dstruct::mkCLOSXP(CAR(formals_cell), CAR(body_cell), R_BaseEnv());
            if scratch.is_null() {
                return false;
            }
            let _scratch_guard = own_operand(scratch);
            if !compile_closure(scratch) {
                return false;
            }
            let fb = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 2);
            if fb.is_null() {
                return false;
            }

            let _fb_guard = own_operand(fb);
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
            let test_owned = own_operand(CAR(CDR(expr)));

            let test = test_owned.as_raw();
            let body_owned = own_operand(CAR(CDR(CDR(expr))));

            let body = body_owned.as_raw();
            let begin_idx = self.code.len();
            self.emit(opcodes::OP_BEGINLOOP);
            self.emit(0); // break -> ENDLOOP
            self.emit(0); // next -> test
            let test_label = self.code.len() as c_int;
            if !self.compile_expr(test) {
                return false;
            }
            let brif_idx = self.code.len() as c_int;
            self.emit_operand(opcodes::OP_BRIFNOT, 0);
            if !self.compile_expr(body) {
                return false;
            }
            self.emit(opcodes::OP_POP);
            self.emit_operand(opcodes::OP_GOTO, test_label);
            let endloop_pc = self.code.len() as c_int;
            self.emit(opcodes::OP_ENDLOOP);
            let after = self.code.len() as c_int;
            self.code[begin_idx + 1] = endloop_pc;
            self.code[begin_idx + 2] = test_label;
            self.code[brif_idx as usize + 1] = endloop_pc;
            let nil_idx = self.add_const(R_NilValue());
            self.emit_operand(opcodes::OP_PUSHCONST, nil_idx);
            self.emit(opcodes::OPinvisible);
            true
        }
    }

    unsafe fn compile_for(&mut self, expr: SEXP) -> bool {
        unsafe {
            // for (symbol in sequence) body
            let symbol_owned = own_operand(CAR(CDR(expr)));

            let symbol = symbol_owned.as_raw();
            let sequence_owned = own_operand(CAR(CDR(CDR(expr))));

            let sequence = sequence_owned.as_raw();
            let body_owned = own_operand(CAR(CDR(CDR(CDR(expr)))));

            let body = body_owned.as_raw();
            if TYPEOF(symbol) != SEXPTYPE::SYMSXP || !self.compile_expr(sequence) {
                return false;
            }

            let symbol_idx = self.add_const(symbol);
            let start_idx = self.code.len();
            self.emit(opcodes::OP_STARTFOR);
            self.emit(symbol_idx);
            self.emit(0); // empty-sequence target, patched below

            let begin_idx = self.code.len();
            self.emit(opcodes::OP_BEGINLOOP);
            self.emit(0); // break -> ENDLOOP
            self.emit(0); // next -> NEXTFOR

            let body_start = self.code.len() as c_int;
            if !self.compile_expr(body) {
                return false;
            }
            self.emit(opcodes::OP_POP);
            let next_pc = self.code.len() as c_int;
            self.emit_operand(opcodes::OP_NEXTFOR, body_start);
            let endloop_pc = self.code.len() as c_int;
            self.emit(opcodes::OP_ENDLOOP);
            let after = self.code.len() as c_int;

            self.code[start_idx + 2] = after;
            self.code[begin_idx + 1] = endloop_pc;
            self.code[begin_idx + 2] = next_pc;
            self.emit(opcodes::OPinvisible);
            true
        }
    }

    unsafe fn compile_assignment(&mut self, expr: SEXP) -> bool {
        unsafe {
            let lhs_owned = own_operand(CAR(CDR(expr)));

            let lhs = lhs_owned.as_raw();
            let rhs_owned = own_operand(CAR(CDR(CDR(expr))));

            let rhs = rhs_owned.as_raw();
            if TYPEOF(lhs) == SEXPTYPE::LANGSXP {
                return if matches!(
                    symbol_name_from_sexp(CAR(lhs)).as_deref(),
                    Some("[") | Some("[[")
                ) {
                    self.compile_subassign(expr, false)
                } else {
                    self.compile_replacement(lhs, rhs)
                };
            }
            let binds_compiled_fun = is_function_syntax(rhs);
            if TYPEOF(lhs) != SEXPTYPE::SYMSXP || !self.compile_expr(rhs) {
                return false;
            }
            if binds_compiled_fun {
                self.compiled_local_funs.push(own_operand(lhs));
            }
            let symbol_idx = self.add_const(lhs);
            self.emit_operand(opcodes::OP_SETVAR, symbol_idx);
            true
        }
    }

    /// Flat replacement assigns the modified object but returns the original
    /// RHS. Evaluate RHS first; retain other arguments as expressions/promises.
    unsafe fn compile_replacement(&mut self, lhs: SEXP, rhs: SEXP) -> bool {
        unsafe {
            let function = own_operand(CAR(lhs));
            let Some(name) = symbol_name_from_sexp(function.as_raw()) else {
                return false;
            };
            let mut cells = Vec::new();
            let mut current = CDR(lhs);
            let mut seen = std::collections::HashSet::new();
            while current != R_NilValue() && !current.is_null() {
                seen.try_reserve(1)
                    .unwrap_or_else(|_| compiler_error("cannot track replacement arguments"));
                if !seen.insert(current.addr()) {
                    compiler_error("cyclic replacement arguments");
                }
                cells
                    .try_reserve(1)
                    .unwrap_or_else(|_| compiler_error("cannot snapshot replacement arguments"));
                let cell = own_operand(current);
                let tag = TAG(current);
                cells.push((
                    own_operand(CAR(current)),
                    own_operand(if tag.is_null() { R_NilValue() } else { tag }),
                ));
                current = CDR(cell.as_raw());
            }
            let Some((object, object_tag)) = cells.first() else {
                return false;
            };
            if object.typeof_() != SEXPTYPE::SYMSXP
                || cells.iter().any(|(arg, _)| arg.as_raw() == R_DotsSymbol())
            {
                return false;
            }
            let setter_name = std::ffi::CString::new(format!("{name}<-"))
                .unwrap_or_else(|_| compiler_error("invalid replacement function name"));
            let setter = own_operand(crate::sexp::symbol::Rf_install(setter_name.as_ptr()));
            let rhs_expr_idx = self.add_const(rhs);
            if !self.compile_expr(rhs) {
                return false;
            }
            let object_idx = self.add_const(object.as_raw());
            self.emit_operand(opcodes::OP_GETVAR, object_idx);
            self.emit(opcodes::OP_MARK_SHARED);
            if object_tag.as_raw() != R_NilValue() {
                let idx = self.add_const(object_tag.as_raw());
                self.emit_operand(opcodes::OP_SETTAG, idx);
            }
            for (argument, tag) in cells.iter().skip(1) {
                let idx = self.add_const(argument.as_raw());
                self.emit_operand(
                    if argument.as_raw() == crate::sexp::globals::R_MissingArg() {
                        opcodes::OP_PUSHCONST
                    } else {
                        opcodes::OP_MAKEPROMISE
                    },
                    idx,
                );
                if tag.as_raw() != R_NilValue() {
                    let idx = self.add_const(tag.as_raw());
                    self.emit_operand(opcodes::OP_SETTAG, idx);
                }
            }
            let setter_idx = self.add_const(setter.as_raw());
            self.emit_operand(opcodes::OP_REPLACEMENT, setter_idx);
            self.emit(object_idx);
            self.emit(
                c_int::try_from(cells.len())
                    .unwrap_or_else(|_| compiler_error("too many replacement arguments")),
            );
            self.emit(rhs_expr_idx);
            true
        }
    }

    /// `x[] <- rhs` is `` `<-`(`[`(x, ...), rhs) ``. The value is the
    /// modified object, which for a full replacement of a length-1 vector
    /// is the RHS, and the symbol is written back.
    unsafe fn compile_subassign(&mut self, expr: SEXP, superassign: bool) -> bool {
        unsafe {
            let lhs_owned = own_operand(CAR(CDR(expr)));

            let lhs = lhs_owned.as_raw();
            let rhs_owned = own_operand(CAR(CDR(CDR(expr))));

            let rhs = rhs_owned.as_raw();
            if TYPEOF(lhs) != SEXPTYPE::LANGSXP {
                return false;
            }
            let double = match symbol_name_from_sexp(CAR(lhs)).as_deref() {
                Some("[") => false,
                Some("[[") => true,
                _ => return false,
            };
            let object_owned = own_operand(CAR(CDR(lhs)));

            let object = object_owned.as_raw();
            let dollar = if TYPEOF(object) == SEXPTYPE::LANGSXP
                && symbol_name_from_sexp(CAR(object)).as_deref() == Some("$")
            {
                let base_owned = own_operand(CAR(CDR(object)));

                let base = base_owned.as_raw();
                let tag_owned = own_operand(CAR(CDR(CDR(object))));

                let tag = tag_owned.as_raw();
                if TYPEOF(base) != SEXPTYPE::SYMSXP || TYPEOF(tag) != SEXPTYPE::SYMSXP {
                    return false;
                }
                if !self.compile_expr(base) {
                    return false;
                }
                let tag_idx = self.add_const(tag);
                self.emit_operand(opcodes::OP_PUSHCONST, tag_idx);
                let dollar_sym = crate::sexp::symbol::Rf_install(c"$".as_ptr());
                let dollar_idx = self.add_const(dollar_sym);
                self.emit_operand(opcodes::OP_PUSHFUN, dollar_idx);
                self.emit_operand(opcodes::OP_CALL, 2);
                Some((base, tag))
            } else if TYPEOF(object) == SEXPTYPE::SYMSXP && self.compile_expr(object) {
                None
            } else {
                return false;
            };
            // A subscript that runs code (`x[{x[2] <<- 3; 1}] <<- 2`) must
            // see the fetched object as shared, or the inner assign mutates
            // the value the outer update writes back. Constant indexes do not.
            let mut scan = CDR(CDR(lhs));
            let mut constant_indexes = true;
            let mut seen_scan = std::collections::HashSet::new();
            while !scan.is_null() && scan != R_NilValue() {
                seen_scan
                    .try_reserve(1)
                    .unwrap_or_else(|_| compiler_error("cannot track bytecode source list"));
                if !seen_scan.insert(scan.addr()) {
                    compiler_error("cyclic bytecode source list");
                }
                let sub_owned = own_operand(CAR(scan));

                let sub = sub_owned.as_raw();
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
            let mut n_index: c_int = 0;
            let mut seen_index = std::collections::HashSet::new();
            while !index.is_null() && index != R_NilValue() {
                seen_index
                    .try_reserve(1)
                    .unwrap_or_else(|_| compiler_error("cannot track bytecode source list"));
                if !seen_index.insert(index.addr()) {
                    compiler_error("cyclic bytecode source list");
                }
                let index_owned = own_operand(index);
                n_index = n_index
                    .checked_add(1)
                    .unwrap_or_else(|| compiler_error("too many bytecode subscripts"));
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
            if let Some((base, tag)) = dollar {
                let tmp = crate::sexp::symbol::Rf_install(c".Compiler.sub".as_ptr());
                let tmp_idx = self.add_const(tmp);
                self.emit_operand(opcodes::OP_SETVAR, tmp_idx);
                self.emit(opcodes::OP_POP);
                if !self.compile_expr(base) {
                    return false;
                }
                let tag_idx = self.add_const(tag);
                self.emit_operand(opcodes::OP_PUSHCONST, tag_idx);
                self.emit_operand(opcodes::OP_GETVAR, tmp_idx);
                let set_sym = crate::sexp::symbol::Rf_install(c"$<-".as_ptr());
                let set_idx = self.add_const(set_sym);
                self.emit_operand(opcodes::OP_PUSHFUN, set_idx);
                self.emit_operand(opcodes::OP_CALL, 3);
                let base_idx = self.add_const(base);
                self.emit_operand(opcodes::OP_SETVAR, base_idx);
            } else {
                let symbol_idx = self.add_const(object);
                if superassign {
                    self.emit_operand(opcodes::OP_SETVAR2, symbol_idx);
                } else {
                    self.emit_operand(opcodes::OP_SETVAR, symbol_idx);
                }
            }
            true
        }
    }

    /// `<<-`: compile the value, then store into the enclosing frame via
    /// OP_SETVAR2 (eval.c SETVAR2 semantics; the value stays on the stack).
    unsafe fn compile_superassignment(&mut self, expr: SEXP) -> bool {
        unsafe {
            let lhs_owned = own_operand(CAR(CDR(expr)));

            let lhs = lhs_owned.as_raw();
            let rhs_owned = own_operand(CAR(CDR(CDR(expr))));

            let rhs = rhs_owned.as_raw();
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
            let test_owned = own_operand(CAR(CDR(expr)));

            let test = test_owned.as_raw();
            let then_e_owned = own_operand(CAR(CDR(CDR(expr))));

            let then_e = then_e_owned.as_raw();
            let else_e = if CDR(CDR(CDR(expr))).is_null() {
                R_NilValue()
            } else {
                CAR(CDR(CDR(CDR(expr))))
            };

            let else_e_owned = own_operand(else_e);
            let else_e = else_e_owned.as_raw();
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
            let mut seen_cur = std::collections::HashSet::new();
            while !cur.is_null() && cur != R_NilValue() {
                seen_cur
                    .try_reserve(1)
                    .unwrap_or_else(|_| compiler_error("cannot track bytecode source list"));
                if !seen_cur.insert(cur.addr()) {
                    compiler_error("cyclic bytecode source list");
                }
                exprs.push(own_operand(CAR(cur)));
                cur = CDR(cur);
            }

            if exprs.is_empty() {
                let idx = self.add_const(R_NilValue());
                self.emit_operand(opcodes::OP_PUSHCONST, idx);
                return true;
            }

            let last = exprs.len().saturating_sub(1);
            for (index, body) in exprs.into_iter().enumerate() {
                if !self.compile_expr(body.as_raw()) {
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
        self.compiled_local_funs
            .iter()
            .any(|value| value.as_raw() == fun)
    }

    unsafe fn finish(&mut self, source_expr: SEXP) -> SEXP {
        unsafe {
            self.emit(opcodes::OP_RETURN);
            with_required_current_instance(|inst| unsafe {
                with_arena_in(inst, |arena| {
                    let consts =
                        arena.alloc_vector(SEXPTYPE::VECSXP, (self.consts.len() + 1) as i64);
                    let _consts_guard = own_operand(consts);
                    crate::sexp::accessors::SET_VECTOR_ELT(consts, 0, source_expr);
                    for (index, constant) in self.consts.iter().enumerate() {
                        crate::sexp::accessors::SET_VECTOR_ELT(
                            consts,
                            (index + 1) as i64,
                            constant.as_raw(),
                        );
                    }
                    let code = arena.alloc_vector(SEXPTYPE::INTSXP, self.code.len() as i64);
                    let mut code_mut =
                        SexpMut::try_from_checked(own_operand(code)).expect("fresh mutable code");
                    for (index, instruction) in self.code.iter().enumerate() {
                        assert!(code_mut.set_integer_elt(index as i64, *instruction));
                    }

                    let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                    let mut hint_mut = SexpMut::try_from_checked(own_operand(stack_hint))
                        .expect("fresh mutable stack hint");
                    assert!(hint_mut.set_integer_elt(0, self.stack_hint.max(4)));

                    let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
                    crate::sexp::accessors::SET_VECTOR_ELT(bcode, 0, code);
                    crate::sexp::accessors::SET_VECTOR_ELT(bcode, 1, consts);
                    crate::sexp::accessors::SET_VECTOR_ELT(bcode, 2, stack_hint);
                    own_operand(bcode)
                })
            })
            .as_raw()
        }
    }
}
fn symbol_getvar_bcode(sym: SEXP) -> SEXP {
    unsafe {
        let symbol_owned = own_operand(sym);
        let sym = symbol_owned.as_raw();
        with_required_current_instance(|inst| {
            with_arena_in(inst, |arena| {
                let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
                arena
                    .set_reference_element(consts, 0, sym)
                    .expect("fresh constant pool slot");
                arena
                    .set_reference_element(consts, 1, sym)
                    .expect("fresh constant pool slot");
                let code = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
                let mut code_mut =
                    SexpMut::try_from_checked(own_operand(code)).expect("fresh mutable code");
                for (index, instruction) in [opcodes::OP_GETVAR, 1, opcodes::OP_RETURN]
                    .into_iter()
                    .enumerate()
                {
                    assert!(code_mut.set_integer_elt(index as i64, instruction));
                }
                let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                let mut hint_mut = SexpMut::try_from_checked(own_operand(stack_hint))
                    .expect("fresh mutable stack hint");
                assert!(hint_mut.set_integer_elt(0, 4));
                let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
                arena
                    .set_reference_element(bcode, 0, code)
                    .expect("fresh bytecode slot");
                arena
                    .set_reference_element(bcode, 1, consts)
                    .expect("fresh bytecode slot");
                arena
                    .set_reference_element(bcode, 2, stack_hint)
                    .expect("fresh bytecode slot");
                own_operand(bcode)
            })
        })
        .as_raw()
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
            | "numeric"
            | "rm"
            | "makeActiveBinding"
            | "environment"
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
        let source_owned = own_operand(expr);
        let pin = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| compiler_error(error.to_string()))
            .pin()
            .unwrap_or_else(|error| compiler_error(error.to_string()))
            .unwrap_or_else(|| compiler_error("bytecode compilation requires a runtime owner"));
        let expr = source_owned.as_raw();
        let mut compiler = BytecodeCompiler::new(_rho);
        if !compiler.compile_expr(expr) {
            pin.require_live()
                .unwrap_or_else(|error| compiler_error(error.to_string()));
            return None;
        }
        let result = own_operand(compiler.finish(expr));
        pin.require_live()
            .unwrap_or_else(|error| compiler_error(error.to_string()));
        Some(result.as_raw())
    }
}

/// Try to compile a closure body and install bytecode on success.
pub unsafe fn compile_closure(fun: SEXP) -> bool {
    unsafe {
        if fun.is_null() {
            return false;
        }
        let fun_owned = own_operand(fun);
        let pin = fun_owned
            .pin_runtime()
            .unwrap_or_else(|error| compiler_error(error.to_string()))
            .unwrap_or_else(|| compiler_error("bytecode compilation requires a runtime owner"));
        let fun = fun_owned.as_raw();
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
        let _guard = own_operand(bcode);
        pin.require_live()
            .unwrap_or_else(|error| compiler_error(error.to_string()));
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
    fn compiled_methods_tail_vector_operations_match_source() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let env = session.global_env().unwrap();
        for script in [
            "{value<-c(a='foo',b='ANY',c='ANY',d='ANY');unspec<-value=='ANY';unspec[[4L]]}",
            "c(as.character('foo'),rep('ANY',3L))",
            "{cl<-'child';S3Class<-c('parent','oldClass');c(cl,S3Class)}",
            "list(quote(retained_symbol))",
            "list(quote(a+b),expression(a+b))",
            "list(b=1L,a=identity(2L))",
            "{i<-0L;repeat{i<-i+1L;if(i==2L)break};i}",
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                let source = factory
                    .wrap(crate::eval::eval::Rf_eval(
                        expression.as_raw(),
                        env.as_raw(),
                    ))
                    .unwrap();
                let code = compile_expr(expression.as_raw(), env.as_raw())
                    .expect("methods-tail microcase must compile");
                let code = factory.wrap(code).unwrap();
                let compiled = factory
                    .wrap(super::super::bc_eval::bcEval(code.as_raw(), env.as_raw()))
                    .unwrap();
                assert_eq!(
                    crate::mainutils::identical::R_compute_identical(
                        source.as_raw(),
                        compiled.as_raw(),
                        0
                    ),
                    1,
                    "{script}"
                );
            }));
            if let Err(payload) = result {
                let error = payload
                    .downcast_ref::<crate::sexp::context::RError>()
                    .map(|error| error.message.as_str())
                    .unwrap_or("Rust panic");
                panic!("{script}: {error}");
            }
        }
    }

    fn assert_private_matches_source(script: &str, collect: bool) {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let env = session.global_env().unwrap();
        let expression = owner
            .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
            .unwrap()
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let source = own_operand(crate::eval::eval::Rf_eval(
                expression.as_raw(),
                env.as_raw(),
            ));
            let source_visible = crate::sexp::globals::R_Visible();
            let code = own_operand(
                compile_expr(expression.as_raw(), env.as_raw()).expect("replacement must compile"),
            );
            let pool = own_operand(super::super::bc_eval::BCODE_CONSTS(code.as_raw()));
            // No expression remains for an interpreter fallback.
            crate::sexp::accessors::SET_VECTOR_ELT(pool.as_raw(), 0, R_NilValue());
            let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
            if collect {
                let captured_pool = pool.clone();
                let count = callbacks.clone();
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    for index in 0..captured_pool.len() {
                        crate::sexp::accessors::SET_VECTOR_ELT(
                            captured_pool.as_raw(),
                            index,
                            R_NilValue(),
                        );
                    }
                    crate::sexp::gengc::full_gc();
                    count.set(count.get() + 1);
                }));
                session.with_active_in(|instance| {
                    (*instance).memory_state.gc_force_gap = 1;
                    (*instance).memory_state.gc_force_wait = 1;
                });
            }
            let compiled = own_operand(super::super::bc_eval::bcEval(code.as_raw(), env.as_raw()));
            assert_eq!(
                crate::mainutils::identical::R_compute_identical(
                    source.as_raw(),
                    compiled.as_raw(),
                    0
                ),
                1,
                "{script}"
            );
            assert_eq!(
                crate::sexp::globals::R_Visible(),
                source_visible,
                "assignment visibility: {script}"
            );
            if collect {
                assert!(callbacks.get() > 0, "must actually collect");
            }
        }));
        if let Err(payload) = result {
            if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
                panic!("{script}: {}", error.message);
            }
            if let Some(signal) = payload.downcast_ref::<crate::sexp::context::RSignal>() {
                panic!("{script}: {signal:?}");
            }
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn compiled_attribute_replacement_matches_source() {
        for script in [
            "{x<-c('foo','ANY');attr(x,'package')<-c('a','b');x}",
            "{x<-c('foo','ANY');names(x)<-c('a','b');x}",
            "{x<-c('foo','ANY');y<-c('x','y');length(x)<-length(y)<-1L;list(x,y)}",
            "{x<-c('foo','ANY');rhs<-quote(retained_symbol);attr(x,'mark')<-rhs}",
            "{x<-c('foo');rhs<-quote(a+b);attr(x,'mark')<-rhs}",
            "{trace<-0L;x<-c('foo');attr(x,{trace<-trace*10L+2L;'mark'})<-{trace<-trace*10L+1L;7L};list(trace,attr(x,'mark'))}",
            "{x<-c('foo');`stamp<-`<-function(x,label,value){attr(x,label)<-value;x};stamp(x,'mark')<-quote(retained_symbol);x}",
            "{x<-1L;`stamp<-`<-function(label,object,value){attr(object,label)<-value;object};stamp(object=x,label='mark')<-7L;x}",
        ] {
            assert_private_matches_source(script, false);
        }
    }

    #[test]
    fn compiled_replacement_preserves_lazy_source_for_custom_setter() {
        // Pinned GNU oracle returns an attribute containing the symbol itself;
        // the port's interpreted replacement frontend still eagerly forces it.
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let env = session.global_env().unwrap();
        let expression=owner.with_arena(|arena|crate::eval::parser::parse("{x<-c('foo');`stamp<-`<-function(x,label,value){attr(x,'expression')<-substitute(label);x};stamp(x,unbound_label)<-7L;x}",arena,factory.domain())).unwrap().unwrap();
        unsafe {
            let code = own_operand(
                compile_expr(expression.as_raw(), env.as_raw())
                    .expect("lazy custom replacement must compile"),
            );
            crate::sexp::accessors::SET_VECTOR_ELT(
                super::super::bc_eval::BCODE_CONSTS(code.as_raw()),
                0,
                R_NilValue(),
            );
            let result = own_operand(super::super::bc_eval::bcEval(code.as_raw(), env.as_raw()));
            let symbol = crate::sexp::symbol::Rf_install(c"expression".as_ptr());
            let attribute = own_operand(crate::attrib_core::getAttrib(result.as_raw(), symbol));
            assert_eq!(attribute.typeof_(), SEXPTYPE::SYMSXP);
            assert_eq!(
                symbol_name_from_sexp(attribute.as_raw()).as_deref(),
                Some("unbound_label")
            );
            owner.full_gc().unwrap();
            assert_eq!(
                symbol_name_from_sexp(attribute.as_raw()).as_deref(),
                Some("unbound_label")
            );
        }
    }

    #[test]
    fn compiled_replacement_preserves_object_and_rhs_substitute_metadata() {
        // Independent pinned GNU expectation: *tmp* for the cached object and
        // the original quote(y) expression for the evaluated RHS.
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let env = session.global_env().unwrap();
        let script = "{x<-1L;`stamp<-`<-function(x,value){attr(x,'code')<-substitute(x);attr(x,'rhs')<-substitute(value);x};stamp(x)<-quote(y);x}";
        let expression = owner
            .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
            .unwrap()
            .unwrap();
        unsafe {
            let code = own_operand(compile_expr(expression.as_raw(), env.as_raw()).unwrap());
            crate::sexp::accessors::SET_VECTOR_ELT(
                super::super::bc_eval::BCODE_CONSTS(code.as_raw()),
                0,
                R_NilValue(),
            );
            let result = own_operand(super::super::bc_eval::bcEval(code.as_raw(), env.as_raw()));
            let symbol = crate::sexp::symbol::Rf_install(c"code".as_ptr());
            let object_expr = own_operand(crate::attrib_core::getAttrib(result.as_raw(), symbol));
            let symbol = crate::sexp::symbol::Rf_install(c"rhs".as_ptr());
            let rhs_expr = own_operand(crate::attrib_core::getAttrib(result.as_raw(), symbol));
            assert_eq!(
                symbol_name_from_sexp(object_expr.as_raw()).as_deref(),
                Some("*tmp*")
            );
            assert_eq!(
                symbol_name_from_sexp(CAR(rhs_expr.as_raw())).as_deref(),
                Some("quote")
            );
            assert_eq!(
                symbol_name_from_sexp(CAR(CDR(rhs_expr.as_raw()))).as_deref(),
                Some("y")
            );
            owner.full_gc().unwrap();
            assert_eq!(
                symbol_name_from_sexp(object_expr.as_raw()).as_deref(),
                Some("*tmp*")
            );
        }
    }

    #[test]
    fn owned_private_replacement_arguments_survive_pool_detachment_and_collecting_setter() {
        assert_private_matches_source(
            "{x<-c('foo');`stamp<-`<-function(x,label,value){gc();attr(x,label)<-value;x};stamp(x,'mark')<-quote(retained_symbol);x}",
            true,
        );
    }

    #[test]
    fn compile_constant_round_trips_through_bc_eval() {
        let session = RSession::new_without_default_packages();
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
        let session = RSession::new_without_default_packages();
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
        let session = RSession::new_without_default_packages();
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
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
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
        let session = RSession::new_without_default_packages();
        let env = session.global_env().expect("global env");

        unsafe {
            let call = Rf_cons(
                Rf_install(c"!".as_ptr()),
                Rf_cons(
                    crate::sexp::constructors::Rf_ScalarLogical(crate::sexp::ffi::FALSE),
                    R_NilValue(),
                ),
            );
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
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
        let session = RSession::new_without_default_packages();
        let env = session.global_env().expect("global env");

        unsafe {
            let assign = Rf_cons(
                Rf_install(c"<-".as_ptr()),
                Rf_cons(
                    Rf_install(c"x".as_ptr()),
                    Rf_cons(Rf_ScalarInteger(1), R_NilValue()),
                ),
            );
            crate::sexp::accessors::SET_TYPEOF(assign, SEXPTYPE::LANGSXP.as_c_int());

            let block = Rf_cons(
                Rf_install(c"{".as_ptr()),
                Rf_cons(assign, Rf_cons(Rf_install(c"x".as_ptr()), R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(block, SEXPTYPE::LANGSXP.as_c_int());

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
        let session = RSession::new_without_default_packages();
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
            crate::sexp::accessors::SET_TYPEOF(add, SEXPTYPE::LANGSXP.as_c_int());
            let assign = Rf_cons(
                Rf_install(c"<-".as_ptr()),
                Rf_cons(sum, Rf_cons(add, R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(assign, SEXPTYPE::LANGSXP.as_c_int());
            let for_call = Rf_cons(
                Rf_install(c"for".as_ptr()),
                Rf_cons(
                    Rf_install(c"i".as_ptr()),
                    Rf_cons(sequence, Rf_cons(assign, R_NilValue())),
                ),
            );
            crate::sexp::accessors::SET_TYPEOF(for_call, SEXPTYPE::LANGSXP.as_c_int());

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
            crate::sexp::accessors::SET_TYPEOF(empty_for, SEXPTYPE::LANGSXP.as_c_int());
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
        let session = RSession::new_without_default_packages();
        let env = session.global_env().expect("global env");

        unsafe {
            let counter = Rf_install(c"counter".as_ptr());
            defineVar(counter, Rf_ScalarInteger(0), env.clone().as_raw());

            let condition = Rf_cons(
                Rf_install(c"<".as_ptr()),
                Rf_cons(counter, Rf_cons(Rf_ScalarInteger(1_000), R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(condition, SEXPTYPE::LANGSXP.as_c_int());
            let increment = Rf_cons(
                Rf_install(c"+".as_ptr()),
                Rf_cons(counter, Rf_cons(Rf_ScalarInteger(1), R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(increment, SEXPTYPE::LANGSXP.as_c_int());
            let assign = Rf_cons(
                Rf_install(c"<-".as_ptr()),
                Rf_cons(counter, Rf_cons(increment, R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(assign, SEXPTYPE::LANGSXP.as_c_int());
            let while_call = Rf_cons(
                Rf_install(c"while".as_ptr()),
                Rf_cons(condition, Rf_cons(assign, R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(while_call, SEXPTYPE::LANGSXP.as_c_int());

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
    #[test]
    fn owned_bytecode_compiler_constants_survive_gc_and_collecting_publication() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let owner = session.owner_token().unwrap();
            let environment = session.global_env().unwrap();
            let mut compiler = BytecodeCompiler::new(environment.as_raw());
            let constant = Rf_ScalarInteger(73);
            let index = compiler.add_const(constant);
            compiler.emit_operand(opcodes::OP_PUSHCONST, index);
            owner.full_gc().unwrap();
            assert_eq!(compiler.consts[0].integer_elt(0), Some(73));

            let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
            let observed = callbacks.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                crate::sexp::gengc::full_gc();
                observed.set(observed.get() + 1);
            }));
            session.with_active_in(|instance| {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let bytecode = own_operand(compiler.finish(R_NilValue()));
            assert!(callbacks.get() > 0);
            drop(compiler);
            owner.full_gc().unwrap();
            let value = super::super::bc_eval::bcEval(bytecode.as_raw(), environment.as_raw());
            assert_eq!(*INTEGER(value), 73);
            drop(bytecode);
            owner.full_gc().unwrap();
            assert!(crate::sexp::memory::checked_projection(constant).is_none());
        });
    }
    #[test]
    fn owned_bytecode_compiler_block_children_survive_source_replacement_callbacks() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let function = Rf_cons(
                Rf_install(c"function".as_ptr()),
                Rf_cons(R_NilValue(), Rf_cons(Rf_ScalarInteger(1), R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(function, SEXPTYPE::LANGSXP.as_c_int());
            let final_value = Rf_ScalarInteger(37);
            let block = Rf_cons(
                Rf_install(c"{".as_ptr()),
                Rf_cons(function, Rf_cons(final_value, R_NilValue())),
            );
            crate::sexp::accessors::SET_TYPEOF(block, SEXPTYPE::LANGSXP.as_c_int());
            let source = own_operand(block);
            let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
            let observed = callbacks.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                crate::sexp::accessors::SETCDR(source.as_raw(), R_NilValue());
                crate::sexp::gengc::full_gc();
                observed.set(observed.get() + 1);
            }));
            session.with_active_in(|instance| {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let bytecode = own_operand(compile_expr(block, R_BaseEnv()).unwrap());
            assert!(callbacks.get() > 0);
            let result = own_operand(super::super::bc_eval::bcEval(
                bytecode.as_raw(),
                R_BaseEnv(),
            ));
            assert_eq!(result.integer_elt(0), Some(37));
        });
    }
}
