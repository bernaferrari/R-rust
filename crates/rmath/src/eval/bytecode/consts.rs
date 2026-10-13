//! Opcode constants and constant-pool type definitions — extracted verbatim from eval/bytecode.rs

use super::*;

pub(crate) fn format_expression_vector(
    exprs: &Sexp<'static>,
    execution: &PrivateExecution<'_>,
) -> Result<String, String> {
    let entries = snapshot_constants(Some(exprs.clone()))?;
    if entries.is_empty() {
        return Ok("expression()".into());
    }
    let mut parts = Vec::new();
    parts
        .try_reserve_exact(entries.len())
        .map_err(|_| "cannot reserve expression print buffer")?;
    for expr in &entries {
        execution.require_live()?;
        let deparsed = execution
            .native(|_| {
                execution
                    .domain
                    .wrap(unsafe { crate::mainutils::deparse::deparse1line(expr.as_raw(), false) })
            })?
            .map_err(|e| sexp_err("deparsed expression", e))?;
        parts.push(execution.native(|_| unsafe {
            crate::mainutils::essentials::elt_to_string(deparsed.as_raw(), 0)
        })?);
        execution.require_live()?;
    }
    Ok(format!("expression({})", parts.join(", ")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlFlow {
    Normal,
    Break,
    Next,
}

pub const GNU_OP_PRINTVALUE: c_int = 6;
pub const GNU_OP_SETLOOPVAL: c_int = 14;
pub const GNU_OP_GETINTLBUILTIN: c_int = 27;
pub const GNU_OP_GETGLOBFUN: c_int = 24;
pub const GNU_OP_GETSYMFUN: c_int = 25;
pub const GNU_OP_DOTSERR: c_int = 60;
pub const GNU_OP_DOTCALL: c_int = 119;
pub const GNU_OP_VISIBLE: c_int = 94;
pub const GNU_OP_INCLNK: c_int = 124;
pub const GNU_OP_DECLNK: c_int = 125;
pub const GNU_OP_DECLNK_N: c_int = 126;
pub const GNU_OP_INCLNKSTK: c_int = 127;
pub const GNU_OP_DECLNKSTK: c_int = 128;
pub const GNU_OP_DDVAL: c_int = 21;
pub const GNU_OP_DODOTS: c_int = 32;
pub const GNU_OP_CALLSPECIAL: c_int = 40;
pub const GNU_OP_MAKECLOSURE: c_int = 41;
pub const GNU_OP_DDVAL_MISSOK: c_int = 93;

pub const BCreturn: c_int = 0;
pub const BCgvar: c_int = 1;
pub const BCsvar: c_int = 2;
pub const BCint: c_int = 3;
pub const BCreal: c_int = 4;
pub const BCstring: c_int = 5;
pub const BCtrue: c_int = 6;
pub const BCfalse: c_int = 7;
pub const BCnil: c_int = 8;
pub const BCdot: c_int = 9;
pub const BCnot: c_int = 10;
pub const BCadd: c_int = 11;
pub const BCsub: c_int = 12;
pub const BCmul: c_int = 13;
pub const BCdiv: c_int = 14;
pub const BCeq: c_int = 15;
pub const BCne: c_int = 16;
pub const BClt: c_int = 17;
pub const BCle: c_int = 18;
pub const BCgt: c_int = 19;
pub const BCge: c_int = 20;
pub const BCand: c_int = 21;
pub const BCor: c_int = 22;
pub const BCcall: c_int = 23;
pub const BCpush: c_int = 24;
pub const BCpop: c_int = 25;
pub const BCdup: c_int = 26;
pub const BCprint: c_int = 27;
pub const BCbegin: c_int = 28;
pub const BCif: c_int = 29;
pub const BCjump: c_int = 30;
pub const BCfjmp: c_int = 31;
pub const BCtjmp: c_int = 32;
pub const BCfor: c_int = 33;
pub const BCwhile: c_int = 34;
pub const BCrepeat: c_int = 35;
pub const BCbreak: c_int = 36;
pub const BCnext: c_int = 37;
pub const BCclosure: c_int = 38;
pub const BCspecial: c_int = 39;
pub const BCbuiltin: c_int = 40;
pub const BCneg: c_int = 41;
pub const BCmod: c_int = 42;
pub const BCpow: c_int = 43;

/// GNU R's serialized bytecode ABI (the `eval.c` enum and `R_bcVersion`).
///
/// The local evaluator has a deliberately private opcode dialect.  GNU R
/// bytecode must therefore be validated at the serialization boundary before
/// it can reach that evaluator.  These widths are copied from the pinned
/// `r-source/src/main/eval.c` `OP(name, argc)` table; the opcode numbers are
/// the enum order, 0 through 128.
pub const GNU_BC_MIN_VERSION: c_int = 12;
pub const GNU_BC_MAX_VERSION: c_int = 12;
pub const GNU_BC_OPCODE_COUNT: usize = 129;

/// Marker stored in the owned BCODESXP payload for a GNU instruction stream.
/// GNU R does not serialize this slot; it is added only after deserialization
/// so the private opcode dialect can never be inferred from an opcode value.
pub const GNU_BC_DIALECT_MARKER: c_int = 0x4752_4e55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GnuConstantReturn {
    Pool(usize),
    Null,
    True,
    False,
}

pub const GNU_OP_RETURN: c_int = 1;
pub const GNU_OP_BRIFNOT: c_int = 3;
pub const GNU_OP_INVISIBLE: c_int = 15;
pub const GNU_OP_LDCONST: c_int = 16;
pub const GNU_OP_LDNULL: c_int = 17;
pub const GNU_OP_LDTRUE: c_int = 18;
pub const GNU_OP_LDFALSE: c_int = 19;
pub const GNU_OP_GETVAR: c_int = 20;
pub const GNU_OP_GETFUN: c_int = 23;
pub const GNU_OP_GETBUILTIN: c_int = 26;
pub const GNU_OP_CHECKFUN: c_int = 28;
pub const GNU_OP_MAKEPROM: c_int = 29;
pub const GNU_OP_PUSHCONSTARG: c_int = 34;
pub const GNU_OP_PUSHNULLARG: c_int = 35;
pub const GNU_OP_PUSHTRUEARG: c_int = 36;
pub const GNU_OP_PUSHFALSEARG: c_int = 37;
pub const GNU_OP_SETTAG: c_int = 31;
pub const GNU_OP_CALL: c_int = 38;
pub const GNU_OP_PUSHARG: c_int = 33;
pub const GNU_OP_CALLBUILTIN: c_int = 39;
pub const GNU_OP_POP: c_int = 4;
pub const GNU_OP_DUP: c_int = 5;
pub const GNU_OP_SWITCH: c_int = 102;
pub const GNU_OP_GOTO: c_int = 2;
pub const GNU_OP_STARTLOOPCNTXT: c_int = 7;
pub const GNU_OP_ENDLOOPCNTXT: c_int = 8;
pub const GNU_OP_DOLOOPNEXT: c_int = 9;
pub const GNU_OP_DOLOOPBREAK: c_int = 10;
pub const GNU_OP_STARTFOR: c_int = 11;
pub const GNU_OP_STEPFOR: c_int = 12;
pub const GNU_OP_ENDFOR: c_int = 13;
pub const GNU_OP_SETVAR: c_int = 22;
pub const GNU_OP_SETVAR2: c_int = 95;
pub const GNU_OP_SQRT: c_int = 49;
pub const GNU_OP_EXP: c_int = 50;
pub const GNU_OP_BASEGUARD: c_int = 123;
pub const GNU_OP_UMINUS: c_int = 42;
pub const GNU_OP_UPLUS: c_int = 43;
pub const GNU_OP_ADD: c_int = 44;
pub const GNU_OP_SUB: c_int = 45;
pub const GNU_OP_MUL: c_int = 46;
pub const GNU_OP_DIV: c_int = 47;
pub const GNU_OP_EXPT: c_int = 48;
pub const GNU_OP_EQ: c_int = 51;
pub const GNU_OP_NE: c_int = 52;
pub const GNU_OP_LT: c_int = 53;
pub const GNU_OP_LE: c_int = 54;
pub const GNU_OP_GE: c_int = 55;
pub const GNU_OP_GT: c_int = 56;
pub const GNU_OP_AND1ST: c_int = 88;
pub const GNU_OP_AND: c_int = 57;
pub const GNU_OP_OR: c_int = 58;
pub const GNU_OP_NOT: c_int = 59;
pub const GNU_OP_AND2ND: c_int = 89;
pub const GNU_OP_OR1ST: c_int = 90;
pub const GNU_OP_OR2ND: c_int = 91;
pub const GNU_OP_STARTASSIGN: c_int = 61;
pub const GNU_OP_ENDASSIGN: c_int = 62;
pub const GNU_OP_STARTASSIGN2: c_int = 96;
pub const GNU_OP_ENDASSIGN2: c_int = 97;
pub const GNU_OP_SETTER_CALL: c_int = 98;
pub const GNU_OP_GETTER_CALL: c_int = 99;
pub const GNU_OP_SWAP: c_int = 100;
pub const GNU_OP_DUP2ND: c_int = 101;
pub const GNU_OP_DOMISSING: c_int = 30;
pub const GNU_OP_RETURNJMP: c_int = 103;
pub const GNU_OP_STARTSUBSET: c_int = 63;
pub const GNU_OP_DFLTSUBSET: c_int = 64;
pub const GNU_OP_STARTSUBASSIGN: c_int = 65;
pub const GNU_OP_DFLTSUBASSIGN: c_int = 66;
pub const GNU_OP_STARTSUBSET2: c_int = 69;
pub const GNU_OP_DFLTSUBSET2: c_int = 70;
pub const GNU_OP_STARTSUBASSIGN2: c_int = 71;
pub const GNU_OP_DFLTSUBASSIGN2: c_int = 72;
pub const GNU_OP_DOLLAR: c_int = 73;
pub const GNU_OP_DOLLARGETS: c_int = 74;
pub const GNU_OP_ISNULL: c_int = 75;
pub const GNU_OP_ISLOGICAL: c_int = 76;
pub const GNU_OP_ISINTEGER: c_int = 77;
pub const GNU_OP_ISDOUBLE: c_int = 78;
pub const GNU_OP_ISCOMPLEX: c_int = 79;
pub const GNU_OP_ISCHARACTER: c_int = 80;
pub const GNU_OP_ISSYMBOL: c_int = 81;
pub const GNU_OP_ISOBJECT: c_int = 82;
pub const GNU_OP_ISNUMERIC: c_int = 83;
pub const GNU_OP_GETVAR_MISSOK: c_int = 92;
pub const GNU_OP_STARTSUBSET_N: c_int = 104;
pub const GNU_OP_STARTSUBASSIGN_N: c_int = 105;
pub const GNU_OP_VECSUBSET: c_int = 84;
pub const GNU_OP_MATSUBSET: c_int = 85;
pub const GNU_OP_MATSUBSET2: c_int = 107;
pub const GNU_OP_VECSUBSET2: c_int = 106;
pub const GNU_OP_VECSUBASSIGN: c_int = 86;
pub const GNU_OP_MATSUBASSIGN: c_int = 87;
pub const GNU_OP_STARTSUBSET2_N: c_int = 110;
pub const GNU_OP_SUBSET_N: c_int = 112;
pub const GNU_OP_VECSUBASSIGN2: c_int = 108;
pub const GNU_OP_STARTSUBASSIGN2_N: c_int = 111;
pub const GNU_OP_SUBASSIGN_N: c_int = 114;
pub const GNU_OP_MATSUBASSIGN2: c_int = 109;
pub const GNU_OP_SUBSET2_N: c_int = 113;
pub const GNU_OP_SUBASSIGN2_N: c_int = 115;
pub const GNU_OP_LOG: c_int = 116;
pub const GNU_OP_LOGBASE: c_int = 117;
pub const GNU_OP_MATH1: c_int = 118;
pub const GNU_OP_COLON: c_int = 120;
pub const GNU_OP_SEQALONG: c_int = 121;
pub const GNU_OP_SEQLEN: c_int = 122;
