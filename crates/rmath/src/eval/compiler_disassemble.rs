//! Decode the pinned GNU bytecode ABI for compiler::disassemble.
use super::compiler::compiler_error;
use super::{bc_eval, bytecode};
use crate::sexp::globals::R_NilValue;
use crate::sexp::{
    accessors::*,
    constructors::*,
    ffi::{SEXP, SEXPTYPE},
    protect::protect,
    symbol::Rf_install,
};

// Enum order from the pinned r-source/src/main/eval.c, shared with the width table.
const GNU_NAMES: [&str; 129] = [
    "BCMISMATCH.OP",
    "RETURN.OP",
    "GOTO.OP",
    "BRIFNOT.OP",
    "POP.OP",
    "DUP.OP",
    "PRINTVALUE.OP",
    "STARTLOOPCNTXT.OP",
    "ENDLOOPCNTXT.OP",
    "DOLOOPNEXT.OP",
    "DOLOOPBREAK.OP",
    "STARTFOR.OP",
    "STEPFOR.OP",
    "ENDFOR.OP",
    "SETLOOPVAL.OP",
    "INVISIBLE.OP",
    "LDCONST.OP",
    "LDNULL.OP",
    "LDTRUE.OP",
    "LDFALSE.OP",
    "GETVAR.OP",
    "DDVAL.OP",
    "SETVAR.OP",
    "GETFUN.OP",
    "GETGLOBFUN.OP",
    "GETSYMFUN.OP",
    "GETBUILTIN.OP",
    "GETINTLBUILTIN.OP",
    "CHECKFUN.OP",
    "MAKEPROM.OP",
    "DOMISSING.OP",
    "SETTAG.OP",
    "DODOTS.OP",
    "PUSHARG.OP",
    "PUSHCONSTARG.OP",
    "PUSHNULLARG.OP",
    "PUSHTRUEARG.OP",
    "PUSHFALSEARG.OP",
    "CALL.OP",
    "CALLBUILTIN.OP",
    "CALLSPECIAL.OP",
    "MAKECLOSURE.OP",
    "UMINUS.OP",
    "UPLUS.OP",
    "ADD.OP",
    "SUB.OP",
    "MUL.OP",
    "DIV.OP",
    "EXPT.OP",
    "SQRT.OP",
    "EXP.OP",
    "EQ.OP",
    "NE.OP",
    "LT.OP",
    "LE.OP",
    "GE.OP",
    "GT.OP",
    "AND.OP",
    "OR.OP",
    "NOT.OP",
    "DOTSERR.OP",
    "STARTASSIGN.OP",
    "ENDASSIGN.OP",
    "STARTSUBSET.OP",
    "DFLTSUBSET.OP",
    "STARTSUBASSIGN.OP",
    "DFLTSUBASSIGN.OP",
    "STARTC.OP",
    "DFLTC.OP",
    "STARTSUBSET2.OP",
    "DFLTSUBSET2.OP",
    "STARTSUBASSIGN2.OP",
    "DFLTSUBASSIGN2.OP",
    "DOLLAR.OP",
    "DOLLARGETS.OP",
    "ISNULL.OP",
    "ISLOGICAL.OP",
    "ISINTEGER.OP",
    "ISDOUBLE.OP",
    "ISCOMPLEX.OP",
    "ISCHARACTER.OP",
    "ISSYMBOL.OP",
    "ISOBJECT.OP",
    "ISNUMERIC.OP",
    "VECSUBSET.OP",
    "MATSUBSET.OP",
    "VECSUBASSIGN.OP",
    "MATSUBASSIGN.OP",
    "AND1ST.OP",
    "AND2ND.OP",
    "OR1ST.OP",
    "OR2ND.OP",
    "GETVAR_MISSOK.OP",
    "DDVAL_MISSOK.OP",
    "VISIBLE.OP",
    "SETVAR2.OP",
    "STARTASSIGN2.OP",
    "ENDASSIGN2.OP",
    "SETTER_CALL.OP",
    "GETTER_CALL.OP",
    "SWAP.OP",
    "DUP2ND.OP",
    "SWITCH.OP",
    "RETURNJMP.OP",
    "STARTSUBSET_N.OP",
    "STARTSUBASSIGN_N.OP",
    "VECSUBSET2.OP",
    "MATSUBSET2.OP",
    "VECSUBASSIGN2.OP",
    "MATSUBASSIGN2.OP",
    "STARTSUBSET2_N.OP",
    "STARTSUBASSIGN2_N.OP",
    "SUBSET_N.OP",
    "SUBSET2_N.OP",
    "SUBASSIGN_N.OP",
    "SUBASSIGN2_N.OP",
    "LOG.OP",
    "LOGBASE.OP",
    "MATH1.OP",
    "DOTCALL.OP",
    "COLON.OP",
    "SEQALONG.OP",
    "SEQLEN.OP",
    "BASEGUARD.OP",
    "INCLNK.OP",
    "DECLNK.OP",
    "DECLNK_N.OP",
    "INCLNKSTK.OP",
    "DECLNKSTK.OP",
];

unsafe fn expand(code: SEXP, depth: usize) -> SEXP {
    unsafe {
        if depth > 128 {
            compiler_error("bytecode disassembly nesting exceeds limit");
        }
        if !bc_eval::BCODE_IS_GNU(code) {
            compiler_error("disassembly of the private compiler dialect is not yet supported");
        }
        let instructions = VECTOR_ELT(code, 0);
        let constants = bc_eval::BCODE_CONSTS(code);
        if TYPEOF(instructions) != SEXPTYPE::INTSXP || TYPEOF(constants) != SEXPTYPE::VECSXP {
            compiler_error("invalid bytecode object");
        }
        let words =
            std::slice::from_raw_parts(INTEGER(instructions), LENGTH(instructions) as usize);
        bytecode::validate_gnu_bytecode_stream(words).unwrap_or_else(|e| compiler_error(e));
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _result_root = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_install(c".Code".as_ptr()));
        let decoded = Rf_allocVector3(SEXPTYPE::VECSXP, words.len() as _);
        let _decoded_root = protect(decoded);
        SET_VECTOR_ELT(result, 1, decoded);
        SET_VECTOR_ELT(decoded, 0, Rf_ScalarInteger(words[0]));
        let mut pc = 1;
        while pc < words.len() {
            let opcode = words[pc] as usize;
            let name = std::ffi::CString::new(GNU_NAMES[opcode]).unwrap();
            SET_VECTOR_ELT(decoded, pc as _, Rf_install(name.as_ptr()));
            let width = bytecode::GNU_BC_OPERAND_WIDTHS[opcode] as usize;
            for offset in 1..=width {
                SET_VECTOR_ELT(
                    decoded,
                    (pc + offset) as _,
                    Rf_ScalarInteger(words[pc + offset]),
                );
            }
            pc += 1 + width;
        }
        let pool = Rf_allocVector3(SEXPTYPE::VECSXP, LENGTH(constants) as _);
        let _pool_root = protect(pool);
        SET_VECTOR_ELT(result, 2, pool);
        for i in 0..LENGTH(constants) {
            let value = VECTOR_ELT(constants, i as _);
            let decoded = if TYPEOF(value) == SEXPTYPE::BCODESXP {
                expand(value, depth + 1)
            } else {
                value
            };
            SET_VECTOR_ELT(pool, i as _, decoded);
        }
        result
    }
}

pub unsafe fn do_disassemble(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            compiler_error("argument 'code' is missing, with no default");
        }
        if CDR(args) != R_NilValue() {
            compiler_error("unused argument (...)");
        }
        let input = CAR(args);
        let code = if TYPEOF(input) == SEXPTYPE::CLOSXP {
            BODY(input)
        } else {
            input
        };
        if TYPEOF(code) != SEXPTYPE::BCODESXP {
            compiler_error(if TYPEOF(input) == SEXPTYPE::CLOSXP {
                "function is not compiled"
            } else {
                "argument is not byte code"
            });
        }
        let _code_root = protect(code);
        let result = expand(code, 0);
        let _result_root = protect(result);
        let dump_args = Rf_cons(result, R_NilValue());
        let _args_root = protect(dump_args);
        crate::mainutils::essentials::do_dput(call, op, dump_args, rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}
