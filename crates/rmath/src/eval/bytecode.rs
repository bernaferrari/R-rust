//! R bytecode evaluation.
//!
//! R compiles expressions to bytecode for faster execution.
//! The bytecode format is a vector of integers where each
//! instruction is an opcode followed by operand indices.

// Constant initializers build scalar vectors through the SexpMut mutation guard
// (sexp::object); the guard is the in-crate mutation path — a containment
// boundary (construction is crate-internal), not a uniqueness proof.
use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::{VECTOR_ELT, XLENGTH};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::memory::with_arena;
use crate::sexp::object::{Sexp, SexpError, SexpMut};

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

unsafe fn format_expression_vector(exprs: SEXP) -> String {
    unsafe {
        let n = XLENGTH(exprs);
        if n == 0 {
            return "expression()".to_string();
        }

        let mut parts = Vec::with_capacity(n as usize);
        for i in 0..n {
            let expr = VECTOR_ELT(exprs, i);
            let deparsed = crate::mainutils::deparse::deparse1line(expr, false);
            parts.push(crate::mainutils::essentials::elt_to_string(deparsed, 0));
        }
        format!("expression({})", parts.join(", "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlFlow {
    Normal,
    Break,
    Next,
}

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
pub const GNU_OP_VECSUBSET2: c_int = 106;
pub const GNU_OP_VECSUBASSIGN: c_int = 86;
pub const GNU_OP_STARTSUBSET2_N: c_int = 110;

pub(super) const GNU_BC_OPERAND_WIDTHS: [u8; GNU_BC_OPCODE_COUNT] = [
    0, 0, 1, 2, 0, 0, 0, 2, 1, 0, 0, 3, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1,
    0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 2,
    0, 2, 0, 2, 0, 2, 0, 2, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 0, 1,
    1, 1, 2, 1, 0, 0, 4, 0, 2, 2, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 1, 1, 2, 2, 1, 1, 1, 2, 0, 0, 1, 0,
    0,
];

/// Validate the integer instruction vector stored by GNU R's BCODESXP
/// serializer.  The first integer is the bytecode ABI version; each
/// subsequent opcode consumes the exact operand count from GNU R's table.
/// This validates framing and version/opcode identity, while operand meaning
/// (for example, whether a constant index is in range) remains a later
/// compiler/interpreter concern.
pub fn validate_gnu_bytecode_stream(code: &[c_int]) -> Result<(), String> {
    let Some(&version) = code.first() else {
        return Err("GNU R bytecode stream is empty".to_string());
    };
    if !(GNU_BC_MIN_VERSION..=GNU_BC_MAX_VERSION).contains(&version) {
        return Err(format!(
            "BCMISMATCH: unsupported GNU R bytecode version {version} (supported {GNU_BC_MIN_VERSION}..={GNU_BC_MAX_VERSION})"
        ));
    }

    let mut pc = 1usize;
    while pc < code.len() {
        let opcode_position = pc;
        let opcode = code[pc];
        pc += 1;
        let Some(&width) = GNU_BC_OPERAND_WIDTHS.get(opcode as usize) else {
            return Err(format!(
                "BCMISMATCH: unknown GNU R bytecode opcode {opcode} at stream offset {opcode_position}"
            ));
        };
        let width = width as usize;
        let end = pc
            .checked_add(width)
            .ok_or_else(|| format!("GNU R bytecode operand range overflows at opcode {opcode}"))?;
        if end > code.len() {
            return Err(format!(
                "truncated GNU R bytecode opcode {opcode} at stream offset {opcode_position}: expected {width} operand(s)"
            ));
        }
        pc = end;
    }
    Ok(())
}

/// Validate the pooled constant-return form of the bounded GNU adapter.
///
/// Keeping this check shared by deserialization, evaluation, and serialization
/// prevents a tagged but subsequently mutated object from escaping the narrow
/// `LDCONST; RETURN` adapter.
pub fn validate_gnu_constant_return_stream(
    code: &[c_int],
    constant_count: usize,
) -> Result<usize, String> {
    validate_gnu_bytecode_stream(code)?;
    if code.len() != 4 || code[1] != 16 || code[3] != 1 {
        return Err("unsupported GNU bytecode stream; only LDCONST+RETURN is implemented".into());
    }
    let index = code[2];
    if index < 0 {
        return Err("GNU LDCONST constant pool index is negative".into());
    }
    let index = index as usize;
    if index >= constant_count {
        return Err(format!(
            "GNU LDCONST constant pool index {index} is out of range for pool length {constant_count}"
        ));
    }
    Ok(index)
}

/// Validate the compiler's scalar-return forms which do not use the constant
/// pool.  GNU R emits these as LDNULL/LDTRUE/LDFALSE followed by RETURN.
pub fn validate_gnu_return_stream(
    code: &[c_int],
    constant_count: usize,
) -> Result<GnuConstantReturn, String> {
    validate_gnu_bytecode_stream(code)?;
    if code.len() == 3 && code[2] == 1 {
        return match code[1] {
            17 => Ok(GnuConstantReturn::Null),
            18 => Ok(GnuConstantReturn::True),
            19 => Ok(GnuConstantReturn::False),
            _ => Err(
                "unsupported GNU bytecode stream; only constant-return forms are implemented"
                    .into(),
            ),
        };
    }
    validate_gnu_constant_return_stream(code, constant_count).map(GnuConstantReturn::Pool)
}

/// Validate the deliberately bounded GNU execution adapter.
///
/// `Ok(false)` means the stream is well-framed but uses an opcode outside the
/// adapter, so deserialization may retain the source expression. `Err` means
/// an operand in an otherwise supported stream is malformed and must not be
/// hidden by source fallback.
pub fn validate_gnu_adapter_stream(code: &[c_int], constant_count: usize) -> Result<bool, String> {
    validate_gnu_adapter_impl(code, constant_count, None)
}

pub unsafe fn validate_gnu_adapter_with_constants(
    code: &[c_int],
    constants: SEXP,
) -> Result<bool, String> {
    unsafe {
        let switches = super::gnu_switch::targets(code, constants)?;
        validate_gnu_adapter_impl(code, XLENGTH(constants) as usize, Some(&switches))
    }
}

fn validate_gnu_adapter_impl(
    code: &[c_int],
    constant_count: usize,
    switches: Option<&std::collections::BTreeMap<usize, Vec<usize>>>,
) -> Result<bool, String> {
    validate_gnu_bytecode_stream(code)?;

    let mut pc = 1usize;
    let mut boundaries = vec![false; code.len()];
    let mut branches = Vec::new();
    let mut for_entry_targets = Vec::new();
    let mut for_steps = Vec::new();
    let mut supported = true;
    while pc < code.len() {
        let opcode_pc = pc;
        boundaries[opcode_pc] = true;
        let opcode = code[pc];
        pc += 1;
        match opcode {
            GNU_OP_RETURN | GNU_OP_INVISIBLE | GNU_OP_LDNULL | GNU_OP_LDTRUE | GNU_OP_LDFALSE
            | GNU_OP_POP | GNU_OP_ENDFOR | GNU_OP_PUSHNULLARG | GNU_OP_PUSHTRUEARG
            | GNU_OP_PUSHFALSEARG | GNU_OP_PUSHARG | GNU_OP_CHECKFUN | GNU_OP_DUP
            | GNU_OP_ISNULL | GNU_OP_ISLOGICAL | GNU_OP_ISINTEGER | GNU_OP_ISDOUBLE
            | GNU_OP_ISCOMPLEX | GNU_OP_ISCHARACTER | GNU_OP_ISSYMBOL | GNU_OP_ISOBJECT
            | GNU_OP_ISNUMERIC => {}
            GNU_OP_STARTASSIGN | GNU_OP_ENDASSIGN => {
                let index = code[pc];
                if index < 0 {
                    return Err(format!(
                        "GNU opcode {opcode} constant pool index {index} is negative"
                    ));
                }
                if index as usize >= constant_count {
                    return Err(format!(
                        "GNU opcode {opcode} constant pool index {index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_DOLLAR | GNU_OP_DOLLARGETS => {
                for slot in 0..2 {
                    let index = code[pc + slot];
                    if index < 0 {
                        return Err(format!(
                            "GNU opcode {opcode} constant pool index {index} is negative"
                        ));
                    }
                    if index as usize >= constant_count {
                        return Err(format!(
                            "GNU opcode {opcode} constant pool index {index} is out of range for pool length {constant_count}"
                        ));
                    }
                }
            }
            GNU_OP_STARTSUBSET_N | GNU_OP_STARTSUBSET2_N | GNU_OP_STARTSUBASSIGN_N => {
                let call_index = code[pc];
                let target = code[pc + 1];
                let name = match opcode {
                    GNU_OP_STARTSUBSET_N => "STARTSUBSET_N",
                    GNU_OP_STARTSUBSET2_N => "STARTSUBSET2_N",
                    _ => "STARTSUBASSIGN_N",
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU {name} jump target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
            }
            GNU_OP_VECSUBSET | GNU_OP_VECSUBSET2 | GNU_OP_VECSUBASSIGN => {
                let call_index = code[pc];
                let name = match opcode {
                    GNU_OP_VECSUBSET => "VECSUBSET",
                    GNU_OP_VECSUBSET2 => "VECSUBSET2",
                    _ => "VECSUBASSIGN",
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_LDCONST | GNU_OP_GETVAR | GNU_OP_GETVAR_MISSOK | GNU_OP_GETFUN | GNU_OP_GETBUILTIN
            | GNU_OP_MAKEPROM | GNU_OP_PUSHCONSTARG | GNU_OP_UMINUS | GNU_OP_UPLUS | GNU_OP_ADD
            | GNU_OP_SUB | GNU_OP_MUL | GNU_OP_DIV | GNU_OP_EXPT | GNU_OP_EQ | GNU_OP_NE
            | GNU_OP_LT | GNU_OP_LE | GNU_OP_GE | GNU_OP_GT | GNU_OP_AND | GNU_OP_OR
            | GNU_OP_NOT | GNU_OP_SQRT | GNU_OP_EXP
            | GNU_OP_SETVAR | GNU_OP_SETVAR2 => {
                let index = code[pc];
                if index < 0 {
                    return Err(format!(
                        "GNU opcode {opcode} constant pool index {index} is negative"
                    ));
                }
                if index as usize >= constant_count {
                    return Err(format!(
                        "GNU opcode {opcode} constant pool index {index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_SWITCH => {
                for index in &code[pc..pc + 4] {
                    if *index < 0 || *index as usize >= constant_count {
                        return Err("GNU SWITCH constant index out of range".into());
                    }
                }
                if let Some(targets) = switches.and_then(|all| all.get(&opcode_pc)) {
                    branches.extend(targets.iter().map(|target| (opcode_pc, *target)));
                } else {
                    supported = false;
                }
            }
            GNU_OP_BRIFNOT => {
                let call_index = code[pc];
                let target = code[pc + 1];
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU BRIFNOT expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU BRIFNOT jump target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
            }
            GNU_OP_AND1ST | GNU_OP_OR1ST => {
                let call_index = code[pc];
                let target = code[pc + 1];
                let name = if opcode == GNU_OP_AND1ST {
                    "AND1ST"
                } else {
                    "OR1ST"
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU {name} jump target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
            }
            GNU_OP_AND2ND | GNU_OP_OR2ND => {
                let call_index = code[pc];
                let name = if opcode == GNU_OP_AND2ND {
                    "AND2ND"
                } else {
                    "OR2ND"
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_CALL => {
                let call_index = code[pc];
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU CALL expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_CALLBUILTIN => {
                let call_index = code[pc];
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU CALLBUILTIN expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_SETTAG => {
                let tag_index = code[pc];
                if tag_index < 0 || tag_index as usize >= constant_count {
                    return Err(format!(
                        "GNU SETTAG constant pool index {tag_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_GOTO => {
                let target = code[pc];
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU GOTO jump target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
            }
            GNU_OP_STARTFOR => {
                let expression = code[pc];
                let symbol = code[pc + 1];
                let target = code[pc + 2];
                if expression < 0 || expression as usize >= constant_count {
                    return Err(format!(
                        "GNU STARTFOR expression index {expression} is out of range for pool length {constant_count}"
                    ));
                }
                if symbol < 0 || symbol as usize >= constant_count {
                    return Err(format!(
                        "GNU STARTFOR symbol index {symbol} is out of range for pool length {constant_count}"
                    ));
                }
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU STARTFOR end target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
                for_entry_targets.push(target as usize);
            }
            GNU_OP_STEPFOR => {
                let target = code[pc];
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU STEPFOR body target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
                for_steps.push(opcode_pc);
            }
            GNU_OP_BASEGUARD => {
                let expression = code[pc];
                let target = code[pc + 1];
                if expression < 0 || expression as usize >= constant_count {
                    return Err(format!(
                        "GNU BASEGUARD expression index {expression} is out of range for pool length {constant_count}"
                    ));
                }
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU BASEGUARD jump target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
            }
            _ => supported = false,
        }
        pc += GNU_BC_OPERAND_WIDTHS[opcode as usize] as usize;
    }

    for (branch_pc, target) in branches {
        if !boundaries[target] {
            return Err(format!(
                "GNU BRIFNOT jump target {target} is not an instruction boundary"
            ));
        }
        if supported && target <= branch_pc && code[branch_pc] != GNU_OP_STEPFOR {
            return Err(format!(
                "GNU BRIFNOT backward jump from {branch_pc} to {target} is outside the bounded adapter"
            ));
        }
    }
    for step_pc in for_steps {
        if !for_entry_targets.contains(&step_pc) {
            return Err(format!(
                "GNU STEPFOR at instruction {step_pc} is not the entry target of STARTFOR"
            ));
        }
        let next = step_pc + 1 + GNU_BC_OPERAND_WIDTHS[GNU_OP_STEPFOR as usize] as usize;
        if code.get(next) != Some(&GNU_OP_ENDFOR) {
            return Err(format!(
                "GNU STEPFOR at instruction {step_pc} is not followed by ENDFOR"
            ));
        }
    }
    for entry_pc in for_entry_targets {
        if code[entry_pc] != GNU_OP_STEPFOR {
            return Err(format!(
                "GNU STARTFOR entry target {entry_pc} is not a STEPFOR instruction"
            ));
        }
    }
    if !supported {
        return Ok(false);
    }

    let mut depths = vec![None; code.len()];
    let mut loop_stacks: Vec<Option<Vec<usize>>> = vec![None; code.len()];
    let mut pending = vec![(1usize, 0usize, Vec::new(), Vec::new())];
    let mut call_stacks: Vec<Option<Vec<usize>>> = vec![None; code.len()];
    let mut saw_return = false;
    while let Some((instruction_pc, depth, loop_stack, mut call_stack)) = pending.pop() {
        if instruction_pc >= code.len() || !boundaries[instruction_pc] {
            return Err(format!(
                "GNU bytecode control flow reaches invalid instruction {instruction_pc}"
            ));
        }
        if let Some(previous) = depths[instruction_pc] {
            if previous != depth
                || loop_stacks[instruction_pc].as_ref() != Some(&loop_stack)
                || call_stacks[instruction_pc].as_ref() != Some(&call_stack)
            {
                return Err(format!(
                    "GNU bytecode state disagrees at instruction {instruction_pc}"
                ));
            }
            continue;
        }
        if call_stack.last().is_some_and(|marker| *marker >= depth) {
            return Err("GNU bytecode consumed an active call frame".into());
        }
        call_stacks[instruction_pc] = Some(call_stack.clone());
        depths[instruction_pc] = Some(depth);
        loop_stacks[instruction_pc] = Some(loop_stack.clone());
        let opcode = code[instruction_pc];
        let next = instruction_pc + 1 + GNU_BC_OPERAND_WIDTHS[opcode as usize] as usize;
        match opcode {
            GNU_OP_RETURN => {
                if !call_stack.is_empty() {
                    return Err("GNU RETURN exits an unfinished call".into());
                }
                let required_depth = loop_stack.len() + 1;
                if depth != required_depth {
                    return Err(format!(
                        "GNU RETURN at instruction {instruction_pc} requires stack depth {required_depth}, found {depth}"
                    ));
                }
                saw_return = true;
            }
            GNU_OP_SWITCH => {
                if depth == 0 {
                    return Err("GNU SWITCH has an empty stack".into());
                }
                let targets = switches
                    .and_then(|all| all.get(&instruction_pc))
                    .ok_or("GNU SWITCH has no validated targets")?;
                for target in targets {
                    pending.push((*target, depth - 1, loop_stack.clone(), call_stack.clone()));
                }
            }
            GNU_OP_BRIFNOT => {
                if depth == 0 {
                    return Err(format!(
                        "GNU BRIFNOT at instruction {instruction_pc} has an empty stack"
                    ));
                }
                pending.push((next, depth - 1, loop_stack.clone(), call_stack.clone()));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth - 1,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_AND1ST | GNU_OP_OR1ST => {
                if depth == 0 {
                    return Err(format!(
                        "GNU short-circuit opcode {opcode} at instruction {instruction_pc} has an empty stack"
                    ));
                }
                pending.push((next, depth, loop_stack.clone(), call_stack.clone()));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_AND2ND | GNU_OP_OR2ND => {
                if depth < 2 {
                    return Err(format!(
                        "GNU short-circuit opcode {opcode} at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_GOTO => pending.push((
                code[instruction_pc + 1] as usize,
                depth,
                loop_stack,
                call_stack.clone(),
            )),
            GNU_OP_DUP => {
                if depth == 0 {
                    return Err(format!(
                        "GNU DUP at instruction {instruction_pc} has an empty stack"
                    ));
                }
                if depth >= 64 {
                    return Err("GNU bytecode exceeds the bounded adapter stack limit of 64".into());
                }
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_POP => {
                if depth == 0 {
                    return Err(format!(
                        "GNU POP at instruction {instruction_pc} has an empty stack"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_SETVAR | GNU_OP_SETVAR2 => {
                if depth == 0 {
                    return Err(format!(
                        "GNU SETVAR at instruction {instruction_pc} has an empty stack"
                    ));
                }
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_STARTASSIGN => {
                if depth == 0 {
                    return Err(format!(
                        "GNU STARTASSIGN at instruction {instruction_pc} has an empty stack"
                    ));
                }
                if depth + 3 > 64 {
                    return Err("GNU bytecode exceeds the bounded adapter stack limit of 64".into());
                }
                pending.push((next, depth + 3, loop_stack, call_stack.clone()));
            }
            GNU_OP_ENDASSIGN => {
                if depth < 3 {
                    return Err(format!(
                        "GNU ENDASSIGN at instruction {instruction_pc} has stack depth {depth}, requires 3"
                    ));
                }
                pending.push((next, depth - 2, loop_stack, call_stack.clone()));
            }
            GNU_OP_DOLLAR => {
                if depth == 0 {
                    return Err(format!(
                        "GNU DOLLAR at instruction {instruction_pc} has an empty stack"
                    ));
                }
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_DOLLARGETS => {
                if depth < 2 {
                    return Err(format!(
                        "GNU DOLLARGETS at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_STARTSUBSET_N | GNU_OP_STARTSUBSET2_N => {
                if depth == 0 {
                    return Err(format!(
                        "GNU STARTSUBSET_N at instruction {instruction_pc} has an empty stack"
                    ));
                }
                pending.push((next, depth, loop_stack.clone(), call_stack.clone()));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_STARTSUBASSIGN_N => {
                if depth < 2 {
                    return Err(format!(
                        "GNU STARTSUBASSIGN_N at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth, loop_stack.clone(), call_stack.clone()));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth - 1,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_VECSUBSET | GNU_OP_VECSUBSET2 => {
                if depth < 2 {
                    return Err(format!(
                        "GNU VECSUBSET at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_VECSUBASSIGN => {
                if depth < 3 {
                    return Err(format!(
                        "GNU VECSUBASSIGN at instruction {instruction_pc} has stack depth {depth}, requires 3"
                    ));
                }
                pending.push((next, depth - 2, loop_stack, call_stack.clone()));
            }
            GNU_OP_STARTFOR => {
                if depth == 0 {
                    return Err(format!(
                        "GNU STARTFOR at instruction {instruction_pc} has an empty stack"
                    ));
                }
                let mut entered = loop_stack;
                entered.push(code[instruction_pc + 3] as usize);
                pending.push((
                    code[instruction_pc + 3] as usize,
                    depth,
                    entered,
                    call_stack.clone(),
                ));
            }
            GNU_OP_STEPFOR => {
                if loop_stack.last() != Some(&instruction_pc) {
                    return Err(format!(
                        "GNU STEPFOR at instruction {instruction_pc} is not the active for loop"
                    ));
                }
                pending.push((next, depth, loop_stack.clone(), call_stack.clone()));
                pending.push((
                    code[instruction_pc + 1] as usize,
                    depth,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_ENDFOR => {
                let mut exited = loop_stack;
                if exited.pop().is_none() {
                    return Err(format!(
                        "GNU ENDFOR at instruction {instruction_pc} has no active for loop"
                    ));
                }
                pending.push((next, depth, exited, call_stack.clone()));
            }
            GNU_OP_BASEGUARD => {
                pending.push((next, depth, loop_stack.clone(), call_stack.clone()));
                if depth >= 64 {
                    return Err("GNU bytecode exceeds the bounded adapter stack limit of 64".into());
                }
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth + 1,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_LDCONST | GNU_OP_GETVAR | GNU_OP_GETVAR_MISSOK | GNU_OP_LDNULL | GNU_OP_LDTRUE | GNU_OP_LDFALSE => {
                if depth >= 64 {
                    return Err("GNU bytecode exceeds the bounded adapter stack limit of 64".into());
                }
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_ADD | GNU_OP_SUB | GNU_OP_MUL | GNU_OP_DIV | GNU_OP_EXPT | GNU_OP_EQ
            | GNU_OP_NE | GNU_OP_LT | GNU_OP_LE | GNU_OP_GE | GNU_OP_GT | GNU_OP_AND
            | GNU_OP_OR => {
                if depth < 2 {
                    return Err(format!(
                        "GNU binary opcode {opcode} at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_UMINUS | GNU_OP_UPLUS | GNU_OP_SQRT | GNU_OP_EXP | GNU_OP_NOT
            | GNU_OP_ISNULL | GNU_OP_ISLOGICAL | GNU_OP_ISINTEGER | GNU_OP_ISDOUBLE
            | GNU_OP_ISCOMPLEX | GNU_OP_ISCHARACTER | GNU_OP_ISSYMBOL | GNU_OP_ISOBJECT
            | GNU_OP_ISNUMERIC => {
                if depth < 1 {
                    return Err(format!(
                        "GNU unary opcode {opcode} at instruction {instruction_pc} has empty stack"
                    ));
                }
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_GETFUN | GNU_OP_GETBUILTIN | GNU_OP_MAKEPROM | GNU_OP_PUSHCONSTARG
            | GNU_OP_PUSHNULLARG | GNU_OP_PUSHTRUEARG | GNU_OP_PUSHFALSEARG => {
                if depth >= 64 {
                    return Err("GNU bytecode exceeds bounded stack limit".into());
                }
                if opcode == GNU_OP_GETFUN || opcode == GNU_OP_GETBUILTIN {
                    call_stack.push(depth);
                } else if call_stack.is_empty() {
                    return Err(format!(
                        "GNU argument opcode {opcode} has no active GETFUN call"
                    ));
                }
                pending.push((next, depth + 1, loop_stack, call_stack));
            }
            GNU_OP_PUSHARG => {
                let Some(marker) = call_stack.last() else {
                    return Err("GNU PUSHARG has no active call argument".into());
                };
                if depth <= *marker + 1 {
                    return Err("GNU PUSHARG has no preceding argument".into());
                }
                pending.push((next, depth, loop_stack, call_stack));
            }
            GNU_OP_CHECKFUN => {
                if depth == 0 {
                    return Err("GNU CHECKFUN has an empty stack".into());
                }
                let mut entered = call_stack;
                entered.push(depth - 1);
                pending.push((next, depth, loop_stack, entered));
            }
            GNU_OP_SETTAG => {
                let Some(marker) = call_stack.last() else {
                    return Err("GNU SETTAG has no active GETFUN call".into());
                };
                if depth <= *marker + 1 {
                    return Err("GNU SETTAG has no preceding call argument".into());
                }
                pending.push((next, depth, loop_stack, call_stack));
            }
            GNU_OP_CALL | GNU_OP_CALLBUILTIN => {
                let Some(marker) = call_stack.pop() else {
                    return Err("GNU CALL has no active GETFUN call".into());
                };
                pending.push((next, marker + 1, loop_stack, call_stack));
            }
            GNU_OP_INVISIBLE => pending.push((next, depth, loop_stack, call_stack.clone())),
            _ => unreachable!(),
        }
    }
    if !saw_return {
        return Err("GNU bytecode has no reachable RETURN".into());
    }
    Ok(true)
}

fn read_operand(bytecode: &[c_int], pc: &mut usize, opname: &str) -> Result<c_int, String> {
    let value = bytecode
        .get(*pc)
        .copied()
        .ok_or_else(|| format!("{opname} bytecode operand is truncated"))?;
    *pc += 1;
    Ok(value)
}

fn read_operand_index(bytecode: &[c_int], pc: &mut usize, opname: &str) -> Result<usize, String> {
    let value = read_operand(bytecode, pc, opname)?;
    if value < 0 {
        return Err(format!(
            "{opname} bytecode operand index {value} is negative"
        ));
    }
    Ok(value as usize)
}

fn read_jump_target(bytecode: &[c_int], pc: &mut usize, opname: &str) -> Result<usize, String> {
    let target = read_operand_index(bytecode, pc, opname)?;
    if target > bytecode.len() {
        return Err(format!(
            "{opname} bytecode jump target {target} is outside instruction stream length {}",
            bytecode.len()
        ));
    }
    Ok(target)
}

fn make_lgl<'a>(val: c_int) -> Result<Sexp<'a>, String> {
    let lgl = with_arena(|arena| arena.alloc_vector(SEXPTYPE::LGLSXP, 1));
    if lgl.is_null() {
        return Err("failed to allocate logical scalar".to_string());
    }
    let sexp = Sexp::from_raw(lgl).ok_or_else(|| "invalid logical scalar pointer".to_string())?;
    let mut guard = SexpMut::from_owned(sexp);
    guard
        .try_set_logical_elt(0, val)
        .map_err(|err| sexp_err("failed to initialize logical scalar", err))?;
    let sexp = guard.freeze();
    unsafe {
        (*lgl).sxpinfo.set_scalar(true);
    }
    Ok(sexp)
}

fn make_real<'a>(val: c_double) -> Result<Sexp<'a>, String> {
    let real = with_arena(|arena| arena.alloc_vector(SEXPTYPE::REALSXP, 1));
    if real.is_null() {
        return Err("failed to allocate real scalar".to_string());
    }
    let sexp = Sexp::from_raw(real).ok_or_else(|| "invalid real scalar pointer".to_string())?;
    let mut guard = SexpMut::from_owned(sexp);
    guard
        .try_set_real_elt(0, val)
        .map_err(|err| sexp_err("failed to initialize real scalar", err))?;
    let sexp = guard.freeze();
    unsafe {
        (*real).sxpinfo.set_scalar(true);
    }
    Ok(sexp)
}

fn make_int<'a>(val: c_int) -> Result<Sexp<'a>, String> {
    let int = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
    if int.is_null() {
        return Err("failed to allocate integer scalar".to_string());
    }
    let sexp = Sexp::from_raw(int).ok_or_else(|| "invalid integer scalar pointer".to_string())?;
    let mut guard = SexpMut::from_owned(sexp);
    guard
        .try_set_integer_elt(0, val)
        .map_err(|err| sexp_err("failed to initialize integer scalar", err))?;
    let sexp = guard.freeze();
    unsafe {
        (*int).sxpinfo.set_scalar(true);
    }
    Ok(sexp)
}

fn scalar_int(value: Sexp<'_>, context: &str) -> Result<c_int, String> {
    value
        .try_integer_elt(0)
        .map_err(|err| sexp_err(context, err))
}

fn scalar_real(value: Sexp<'_>, context: &str) -> Result<c_double, String> {
    value.try_real_elt(0).map_err(|err| sexp_err(context, err))
}

fn scalar_f64_or_zero(value: Sexp<'_>, context: &str) -> Result<c_double, String> {
    match value.clone().typeof_() {
        SEXPTYPE::REALSXP => scalar_real(value, context),
        SEXPTYPE::INTSXP => scalar_int(value, context).map(c_double::from),
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(0)
            .map(c_double::from)
            .map_err(|err| sexp_err(context, err)),
        _ => Ok(0.0),
    }
}

fn scalar_bool_or_false(value: Sexp<'_>, context: &str) -> Result<bool, String> {
    match value.clone().typeof_() {
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(0)
            .map(|value| value != 0)
            .map_err(|err| sexp_err(context, err)),
        SEXPTYPE::INTSXP => scalar_int(value, context).map(|value| value != 0),
        SEXPTYPE::REALSXP => scalar_real(value, context).map(|value| value != 0.0),
        _ => Ok(false),
    }
}

fn apply_binary_op<'a, FR, FI>(
    a: Sexp<'a>,
    b: Sexp<'a>,
    real_op: FR,
    int_op: FI,
) -> Result<Sexp<'a>, String>
where
    FR: Fn(c_double, c_double) -> c_double,
    FI: Fn(c_int, c_int) -> c_int,
{
    if a.clone().typeof_() == SEXPTYPE::REALSXP && b.clone().typeof_() == SEXPTYPE::REALSXP {
        let av = scalar_real(a, "left real operand")?;
        let bv = scalar_real(b, "right real operand")?;
        make_real(real_op(av, bv))
    } else if a.clone().typeof_() == SEXPTYPE::INTSXP && b.clone().typeof_() == SEXPTYPE::INTSXP {
        let av = scalar_int(a, "left integer operand")?;
        let bv = scalar_int(b, "right integer operand")?;
        make_int(int_op(av, bv))
    } else {
        let av = scalar_f64_or_zero(a, "left numeric operand")?;
        let bv = scalar_f64_or_zero(b, "right numeric operand")?;
        make_real(real_op(av, bv))
    }
}

fn apply_comparison<'a, F>(a: Sexp<'a>, b: Sexp<'a>, cmp: F) -> Result<Sexp<'a>, String>
where
    F: Fn(c_double, c_double) -> bool,
{
    let result = if a.clone().typeof_() == SEXPTYPE::REALSXP
        && b.clone().typeof_() == SEXPTYPE::REALSXP
    {
        let av = scalar_real(a, "left real comparison operand")?;
        let bv = scalar_real(b, "right real comparison operand")?;
        if cmp(av, bv) { 1 } else { 0 }
    } else if a.clone().typeof_() == SEXPTYPE::INTSXP && b.clone().typeof_() == SEXPTYPE::INTSXP {
        let av = scalar_int(a, "left integer comparison operand")? as c_double;
        let bv = scalar_int(b, "right integer comparison operand")? as c_double;
        if cmp(av, bv) { 1 } else { 0 }
    } else {
        let av = scalar_f64_or_zero(a, "left comparison operand")?;
        let bv = scalar_f64_or_zero(b, "right comparison operand")?;
        if cmp(av, bv) { 1 } else { 0 }
    };
    make_lgl(result)
}

pub fn eval_bytecode<'a>(code: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let bytecode = code
        .clone()
        .try_as_integer_slice()
        .clone()
        .map_err(|err| sexp_err("invalid bytecode vector", err))?;
    let mut pc: usize = 0;
    let mut stack: Vec<Sexp<'a>> = Vec::new();
    let constants = code.attrib();
    eval_bytecode_loop(bytecode, &mut pc, &mut stack, constants, env).map(|(sexp, _)| sexp)
}

fn eval_bytecode_loop<'a>(
    bytecode: &[c_int],
    pc: &mut usize,
    stack: &mut Vec<Sexp<'a>>,
    constants: Option<Sexp<'a>>,
    env: Sexp<'a>,
) -> Result<(Sexp<'a>, ControlFlow), String> {
    while *pc < bytecode.len() {
        let opcode = bytecode[*pc] as c_int;
        *pc += 1;

        match opcode {
            BCreturn => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on return".to_string())?;
                return Ok((val, ControlFlow::Normal));
            }
            BCgvar | BCsvar => {
                let idx = read_operand_index(bytecode, pc, "variable")?;
                let sym = get_constant(constants.clone(), idx)?;
                let val = crate::eval::eval::find_var_result(sym, env.clone())?
                    .ok_or_else(|| "variable not found".to_string())?;
                stack.push(val);
            }
            BCint | BCreal | BCstring => {
                let idx = read_operand_index(bytecode, pc, "constant")?;
                let val = get_constant(constants.clone(), idx)?;
                stack.push(val);
            }
            BCtrue => {
                stack.push(unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_True()) });
            }
            BCfalse => {
                stack.push(unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_False()) });
            }
            BCnil => {
                stack.push(unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) });
            }
            BCdot => {
                return Err("'...' used in incorrect context".to_string());
            }
            BCnot => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on not".to_string())?;
                let v = if scalar_bool_or_false(val, "bytecode not operand")? {
                    1
                } else {
                    0
                };
                stack.push(make_lgl(if v != 0 { 0 } else { 1 })?);
            }
            BCadd => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on add".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on add".to_string())?;
                stack.push(apply_binary_op(
                    a,
                    b,
                    |x, y| x + y,
                    |x, y| x.wrapping_add(y),
                )?);
            }
            BCsub => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on sub".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on sub".to_string())?;
                stack.push(apply_binary_op(
                    a,
                    b,
                    |x, y| x - y,
                    |x, y| x.wrapping_sub(y),
                )?);
            }
            BCmul => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mul".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mul".to_string())?;
                stack.push(apply_binary_op(
                    a,
                    b,
                    |x, y| x * y,
                    |x, y| x.wrapping_mul(y),
                )?);
            }
            BCdiv => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                stack.push(apply_binary_op(a, b, |x, y| x / y, |x, y| x / y)?);
            }
            BCmod => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                if a.clone().typeof_() == SEXPTYPE::REALSXP
                    && b.clone().typeof_() == SEXPTYPE::REALSXP
                {
                    stack.push(make_real(
                        scalar_real(a, "left real modulo operand")?
                            % scalar_real(b, "right real modulo operand")?,
                    )?);
                } else if a.clone().typeof_() == SEXPTYPE::INTSXP
                    && b.clone().typeof_() == SEXPTYPE::INTSXP
                {
                    let bv = scalar_int(b, "right integer modulo operand")?;
                    if bv != 0 {
                        stack.push(make_int(
                            scalar_int(a, "left integer modulo operand")? % bv,
                        )?);
                    } else {
                        stack.push(make_real(f64::NAN)?);
                    }
                } else {
                    stack.push(make_real(
                        scalar_f64_or_zero(a, "left modulo operand")?
                            % scalar_f64_or_zero(b, "right modulo operand")?,
                    )?);
                }
            }
            BCpow => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                stack.push(make_real(
                    scalar_f64_or_zero(a, "left power operand")?
                        .powf(scalar_f64_or_zero(b, "right power operand")?),
                )?);
            }
            BCeq => {
                let b = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x == y)?);
            }
            BCne => {
                let b = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x != y)?);
            }
            BClt => {
                let b = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x < y)?);
            }
            BCle => {
                let b = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x <= y)?);
            }
            BCgt => {
                let b = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x > y)?);
            }
            BCge => {
                let b = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x >= y)?);
            }
            BCand => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                stack.push(make_lgl(
                    if scalar_bool_or_false(a, "left and operand")?
                        && scalar_bool_or_false(b, "right and operand")?
                    {
                        1
                    } else {
                        0
                    },
                )?);
            }
            BCor => {
                let b = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                stack.push(make_lgl(
                    if scalar_bool_or_false(a, "left or operand")?
                        || scalar_bool_or_false(b, "right or operand")?
                    {
                        1
                    } else {
                        0
                    },
                )?);
            }
            BCcall => {
                let idx = read_operand_index(bytecode, pc, "call function")?;
                let nargs = read_operand_index(bytecode, pc, "call argument count")?;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();
                let fun = get_constant(constants.clone(), idx)?;

                if fun.clone().typeof_() == SEXPTYPE::CLOSXP {
                    let mut arg_list =
                        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) };
                    for arg in args_vec.into_iter().rev() {
                        let cell = with_arena(|arena| {
                            arena.cons(
                                arg.as_raw(),
                                arg_list.clone().as_raw(),
                                std::ptr::null_mut(),
                            )
                        });
                        arg_list = Sexp::from_raw(cell).unwrap_or(arg_list);
                    }
                    let result =
                        crate::eval::closure::apply_closure_safe(fun, arg_list, env.clone())
                            .map_err(|e| format!("closure call failed: {e}"))?;
                    stack.push(result);
                } else {
                    stack.push(fun);
                }
            }
            BCpush => {
                let idx = read_operand_index(bytecode, pc, "push")?;
                stack.push(get_constant(constants.clone(), idx)?);
            }
            BCpop => {
                stack.pop();
            }
            BCdup => {
                if let Some(top) = stack.last() {
                    stack.push(top.clone());
                }
            }
            BCprint => {
                if let Some(top) = stack.last() {
                    let output = if top.clone().typeof_() == SEXPTYPE::EXPRSXP {
                        unsafe { format_expression_vector(top.clone().as_raw()) }
                    } else {
                        let type_name = match top.clone().typeof_() {
                            SEXPTYPE::NILSXP => "NULL",
                            SEXPTYPE::INTSXP => "integer",
                            SEXPTYPE::REALSXP => "double",
                            SEXPTYPE::LGLSXP => "logical",
                            SEXPTYPE::STRSXP => "character",
                            SEXPTYPE::VECSXP => "list",
                            SEXPTYPE::RAWSXP => "raw",
                            SEXPTYPE::CPLXSXP => "complex",
                            SEXPTYPE::SYMSXP => "symbol",
                            SEXPTYPE::CLOSXP => "closure",
                            SEXPTYPE::ENVSXP => "environment",
                            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                            SEXPTYPE::CHARSXP => "charsxp",
                            SEXPTYPE::PROMSXP => "promise",
                            SEXPTYPE::DOTSXP => "...",
                            SEXPTYPE::SPECIALSXP => "special",
                            SEXPTYPE::BUILTINSXP => "builtin",
                            SEXPTYPE::EXTPTRSXP => "externalptr",
                            SEXPTYPE::WEAKREFSXP => "weakref",
                            SEXPTYPE::BCODESXP => "bytecode",
                            SEXPTYPE::OBJSXP => "object",
                            _ => "unknown",
                        };
                        format!("[{}; length={}]", type_name, top.clone().len())
                    };
                    if crate::sexp::output::is_capturing() {
                        crate::sexp::output::capture_stdout(&output);
                        crate::sexp::output::capture_stdout("\n");
                    } else {
                        println!("{}", output);
                    }
                }
            }
            BCbegin => {}
            BCif => {
                let cond = stack.pop().ok_or_else(|| "empty stack on if".to_string())?;
                let true_offset = read_jump_target(bytecode, pc, "if true")?;
                let false_offset = read_jump_target(bytecode, pc, "if false")?;
                if scalar_bool_or_false(cond, "if condition")? {
                    *pc = true_offset;
                } else {
                    *pc = false_offset;
                }
            }
            BCjump => {
                let offset = read_jump_target(bytecode, pc, "jump")?;
                *pc = offset;
            }
            BCfjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on fjmp".to_string())?;
                let offset = read_jump_target(bytecode, pc, "false jump")?;
                if !scalar_bool_or_false(cond, "false jump condition")? {
                    *pc = offset;
                }
            }
            BCtjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on tjmp".to_string())?;
                let offset = read_jump_target(bytecode, pc, "true jump")?;
                if scalar_bool_or_false(cond, "true jump condition")? {
                    *pc = offset;
                }
            }
            BCfor => {
                let var_idx = read_operand_index(bytecode, pc, "for variable")?;
                let seq_idx = read_operand_index(bytecode, pc, "for sequence")?;
                let body_offset = read_jump_target(bytecode, pc, "for body")?;
                let end_offset = read_jump_target(bytecode, pc, "for end")?;

                let var_sym = get_constant(constants.clone(), var_idx)?;
                let seq_val = get_constant(constants.clone(), seq_idx)?;
                let len = seq_val.clone().len();

                for i in 0..len as usize {
                    let idx_val = if seq_val.clone().typeof_() == SEXPTYPE::INTSXP {
                        make_int(
                            seq_val
                                .clone()
                                .try_integer_elt(i as i64)
                                .map_err(|err| sexp_err("for-loop integer sequence", err))?,
                        )?
                    } else if seq_val.clone().typeof_() == SEXPTYPE::REALSXP {
                        make_real(
                            seq_val
                                .clone()
                                .try_real_elt(i as i64)
                                .map_err(|err| sexp_err("for-loop real sequence", err))?,
                        )?
                    } else {
                        make_int(i as c_int)?
                    };

                    unsafe {
                        crate::sexp::envir::defineVar(
                            var_sym.clone().as_raw(),
                            idx_val.clone().as_raw(),
                            env.clone().as_raw(),
                        );
                    }
                    stack.push(idx_val);

                    let mut loop_pc = body_offset;
                    let (_, control) = eval_bytecode_loop(
                        bytecode,
                        &mut loop_pc,
                        stack,
                        constants.clone(),
                        env.clone(),
                    )?;

                    if control == ControlFlow::Break {
                        *pc = end_offset;
                        return Ok((make_lgl(0)?, ControlFlow::Normal));
                    }
                }
                *pc = end_offset;
            }
            BCwhile => {
                let cond_offset = read_jump_target(bytecode, pc, "while condition")?;
                let body_offset = read_jump_target(bytecode, pc, "while body")?;
                let end_offset = read_jump_target(bytecode, pc, "while end")?;

                loop {
                    let mut cond_pc = cond_offset;
                    let (cond_result, cond_control) = eval_bytecode_loop(
                        bytecode,
                        &mut cond_pc,
                        stack,
                        constants.clone(),
                        env.clone(),
                    )?;
                    if cond_control != ControlFlow::Normal {
                        return Ok((cond_result, cond_control));
                    }

                    if !scalar_bool_or_false(cond_result, "while condition")? {
                        *pc = end_offset;
                        return Ok((make_lgl(0)?, ControlFlow::Normal));
                    }

                    let mut body_pc = body_offset;
                    let (body_result, body_control) = eval_bytecode_loop(
                        bytecode,
                        &mut body_pc,
                        stack,
                        constants.clone(),
                        env.clone(),
                    )?;

                    if body_control == ControlFlow::Break {
                        *pc = end_offset;
                        return Ok((body_result, ControlFlow::Normal));
                    }
                }
            }
            BCrepeat => {
                let body_offset = read_jump_target(bytecode, pc, "repeat body")?;
                let end_offset = read_jump_target(bytecode, pc, "repeat end")?;

                loop {
                    let mut body_pc = body_offset;
                    let (body_result, body_control) = eval_bytecode_loop(
                        bytecode,
                        &mut body_pc,
                        stack,
                        constants.clone(),
                        env.clone(),
                    )?;

                    if body_control == ControlFlow::Break {
                        *pc = end_offset;
                        return Ok((body_result, ControlFlow::Normal));
                    }
                }
            }
            BCbreak => {
                return Ok((make_lgl(0)?, ControlFlow::Break));
            }
            BCnext => {
                return Ok((make_lgl(0)?, ControlFlow::Next));
            }
            BCspecial => {
                let idx = read_operand_index(bytecode, pc, "special function")?;
                let nargs = read_operand_index(bytecode, pc, "special argument count")?;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();
                let fun = get_constant(constants.clone(), idx)?;

                if fun.clone().typeof_() == SEXPTYPE::SPECIALSXP
                    || fun.clone().typeof_() == SEXPTYPE::BUILTINSXP
                {
                    let mut arg_list =
                        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) };
                    for arg in args_vec.into_iter().rev() {
                        let cell = with_arena(|arena| {
                            arena.cons(
                                arg.as_raw(),
                                arg_list.clone().as_raw(),
                                std::ptr::null_mut(),
                            )
                        });
                        arg_list = Sexp::from_raw(cell).unwrap_or(arg_list);
                    }

                    let call = with_arena(|arena| {
                        arena.cons(fun.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                    });
                    let call_sexp = unsafe { Sexp::from_raw_unchecked(call) };

                    let result = crate::eval::eval::eval_lang_safe(call_sexp, env.clone())
                        .map_err(|e| format!("special call failed: {e}"))?;
                    stack.push(result);
                } else {
                    stack.push(fun);
                }
            }
            BCbuiltin => {
                let idx = read_operand_index(bytecode, pc, "builtin function")?;
                let nargs = read_operand_index(bytecode, pc, "builtin argument count")?;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();
                let fun = get_constant(constants.clone(), idx)?;

                if fun.clone().typeof_() == SEXPTYPE::BUILTINSXP
                    || fun.clone().typeof_() == SEXPTYPE::SPECIALSXP
                {
                    let mut arg_list =
                        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) };
                    for arg in args_vec.into_iter().rev() {
                        let cell = with_arena(|arena| {
                            arena.cons(
                                arg.as_raw(),
                                arg_list.clone().as_raw(),
                                std::ptr::null_mut(),
                            )
                        });
                        arg_list = Sexp::from_raw(cell).unwrap_or(arg_list);
                    }

                    let call = with_arena(|arena| {
                        arena.cons(fun.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                    });
                    let call_sexp = unsafe { Sexp::from_raw_unchecked(call) };

                    let result = crate::eval::eval::eval_lang_safe(call_sexp, env.clone())
                        .map_err(|e| format!("builtin call failed: {e}"))?;
                    stack.push(result);
                } else {
                    stack.push(fun);
                }
            }
            BCneg => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on neg".to_string())?;
                if val.clone().typeof_() == SEXPTYPE::REALSXP {
                    stack.push(make_real(-scalar_real(val, "real negation operand")?)?);
                } else if val.clone().typeof_() == SEXPTYPE::INTSXP {
                    stack.push(make_int(-scalar_int(val, "integer negation operand")?)?);
                } else {
                    stack.push(val);
                }
            }
            BCclosure => {
                let idx = read_operand_index(bytecode, pc, "closure")?;
                stack.push(get_constant(constants.clone(), idx)?);
            }
            _ => {
                return Err(format!("unknown bytecode opcode: {opcode}"));
            }
        }
    }

    let val = stack
        .pop()
        .ok_or_else(|| "empty stack at end of bytecode".to_string())?;
    Ok((val, ControlFlow::Normal))
}

fn get_constant(constants: Option<Sexp<'_>>, idx: usize) -> Result<Sexp<'_>, String> {
    match constants {
        Some(c) => c
            .try_vector_elt(idx as i64)
            .map_err(|err| sexp_err(&format!("constant index {idx}"), err)),
        None => Err("no constants available".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    fn nil_sexp<'a>() -> Sexp<'a> {
        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) }
    }

    #[test]
    fn eval_bytecode_rejects_truncated_operands() {
        let _session = RSession::new();
        let mut pc = 0;
        let mut stack = Vec::new();
        let err = eval_bytecode_loop(&[BCpush], &mut pc, &mut stack, None, nil_sexp())
            .expect_err("truncated operand should return an error");
        assert!(err.contains("push bytecode operand is truncated"));
    }

    #[test]
    fn eval_bytecode_rejects_negative_constant_indices() {
        let _session = RSession::new();
        let mut pc = 0;
        let mut stack = Vec::new();
        let err = eval_bytecode_loop(&[BCpush, -1], &mut pc, &mut stack, None, nil_sexp())
            .expect_err("negative operand should return an error");
        assert!(err.contains("push bytecode operand index -1 is negative"));
    }

    #[test]
    fn eval_bytecode_rejects_out_of_range_jump_targets() {
        let _session = RSession::new();
        let mut pc = 0;
        let mut stack = Vec::new();
        let err = eval_bytecode_loop(&[BCjump, 99], &mut pc, &mut stack, None, nil_sexp())
            .expect_err("invalid jump should return an error");
        assert!(err.contains("jump bytecode jump target 99 is outside"));
    }

    #[test]
    fn validates_every_pinned_gnu_opcode_width() {
        // Independent copy of the pinned `OP(name, argc)` table in
        // r-source/src/main/eval.c.  Keeping this fixture separate catches a
        // drift in the implementation table instead of inferring expected
        // widths by calling the validator under test.
        let expected: Vec<u8> = include_str!("../../tests/fixtures/gnu-bytecode-opcodes.tsv")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| line.split('\t').nth(2).unwrap().parse().unwrap())
            .collect();
        assert_eq!(expected.len(), GNU_BC_OPCODE_COUNT);
        assert_eq!(GNU_BC_OPERAND_WIDTHS.as_slice(), expected.as_slice());

        for (opcode, &width) in expected.iter().enumerate() {
            // Zero is a valid framing value for every operand slot; semantic
            // indices are intentionally outside this wire-level validator's
            // scope.
            let mut stream = vec![GNU_BC_MAX_VERSION, opcode as c_int];
            stream.extend(std::iter::repeat_n(0, usize::from(width)));
            assert!(
                validate_gnu_bytecode_stream(&stream).is_ok(),
                "GNU opcode {opcode} should accept its pinned operand width"
            );
        }
    }

    #[test]
    fn rejects_gnu_bytecode_version_opcode_and_truncation() {
        assert!(validate_gnu_bytecode_stream(&[]).is_err());
        let bad_ver = validate_gnu_bytecode_stream(&[GNU_BC_MIN_VERSION - 1]).unwrap_err();
        assert!(bad_ver.contains("BCMISMATCH"));
        assert!(bad_ver.contains("unsupported GNU R bytecode version"));
        let bad_ver_hi = validate_gnu_bytecode_stream(&[GNU_BC_MAX_VERSION + 1]).unwrap_err();
        assert!(bad_ver_hi.contains("BCMISMATCH"));
        let bad_op =
            validate_gnu_bytecode_stream(&[GNU_BC_MAX_VERSION, GNU_BC_OPCODE_COUNT as c_int])
                .unwrap_err();
        assert!(bad_op.contains("BCMISMATCH"));
        assert!(bad_op.contains("unknown GNU R bytecode opcode"));
        // GNU GOTO (opcode 2) has one operand.
        let err = validate_gnu_bytecode_stream(&[GNU_BC_MAX_VERSION, 2]).unwrap_err();
        assert!(err.contains("truncated GNU R bytecode opcode 2"));
    }

    #[test]
    fn constant_return_adapter_rejects_shape_and_pool_index_mutations() {
        let valid = [GNU_BC_MAX_VERSION, 16, 1, 1];
        assert_eq!(validate_gnu_constant_return_stream(&valid, 2), Ok(1));
        assert!(
            validate_gnu_constant_return_stream(&valid, 1)
                .unwrap_err()
                .contains("out of range")
        );
        assert!(
            validate_gnu_constant_return_stream(&[GNU_BC_MAX_VERSION, 16, -1, 1], 2)
                .unwrap_err()
                .contains("negative")
        );
        assert!(
            validate_gnu_constant_return_stream(&[GNU_BC_MAX_VERSION, 16, 0, 0], 2)
                .unwrap_err()
                .contains("only LDCONST+RETURN")
        );
    }

    #[test]
    fn scalar_return_adapter_accepts_only_pinned_gnu_shapes() {
        assert_eq!(
            validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 17, 1], 2),
            Ok(GnuConstantReturn::Null)
        );
        assert_eq!(
            validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 18, 1], 2),
            Ok(GnuConstantReturn::True)
        );
        assert_eq!(
            validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 19, 1], 2),
            Ok(GnuConstantReturn::False)
        );
        assert!(validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 17, 0], 2).is_err());
        // A private-dialect opcode with the same length is not accepted as a
        // scalar form merely because it happens to fit the shape.
        assert!(validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 20, 1], 2).is_err());
    }

    #[test]
    fn gnu_arithmetic_and_comparison_streams_have_binary_stack_effects() {
        let add = [
            GNU_BC_MAX_VERSION,
            GNU_OP_LDCONST,
            0,
            GNU_OP_LDCONST,
            1,
            GNU_OP_ADD,
            0,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&add, 2).unwrap());
        let cmp = [
            GNU_BC_MAX_VERSION,
            GNU_OP_LDCONST,
            0,
            GNU_OP_LDCONST,
            1,
            GNU_OP_LT,
            0,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&cmp, 2).unwrap());
        let underflow = [
            GNU_BC_MAX_VERSION,
            GNU_OP_LDCONST,
            0,
            GNU_OP_ADD,
            0,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&underflow, 1).is_err());
        let bad_operand = [
            GNU_BC_MAX_VERSION,
            GNU_OP_LDCONST,
            0,
            GNU_OP_LDCONST,
            1,
            GNU_OP_ADD,
            2,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&bad_operand, 2).is_err());
        let unary = [
            GNU_BC_MAX_VERSION,
            GNU_OP_LDCONST,
            0,
            GNU_OP_UMINUS,
            1,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&unary, 2).unwrap());
        let unary_underflow = [GNU_BC_MAX_VERSION, GNU_OP_UMINUS, 0, GNU_OP_RETURN];
        assert!(validate_gnu_adapter_stream(&unary_underflow, 1).is_err());
        let power = [
            GNU_BC_MAX_VERSION,
            GNU_OP_LDCONST,
            0,
            GNU_OP_LDCONST,
            1,
            GNU_OP_EXPT,
            0,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&power, 2).unwrap());
    }

    #[test]
    fn bounded_gnu_adapter_proves_branch_targets_and_stack_shape() {
        assert!(validate_gnu_adapter_stream(&[12], 0).is_err());
        let branch = [12, 20, 1, 3, 0, 9, 16, 2, 1, 16, 3, 1];
        assert_eq!(validate_gnu_adapter_stream(&branch, 4), Ok(true));

        let mut non_boundary = branch;
        non_boundary[5] = 7;
        assert!(
            validate_gnu_adapter_stream(&non_boundary, 4)
                .unwrap_err()
                .contains("instruction boundary")
        );

        assert!(
            validate_gnu_adapter_stream(&[12, 1], 0)
                .unwrap_err()
                .contains("stack depth 1")
        );
        assert!(
            validate_gnu_adapter_stream(&[12, 18, 19, 1], 0)
                .unwrap_err()
                .contains("stack depth 1")
        );
        assert!(
            validate_gnu_adapter_stream(&[12, 18], 0)
                .unwrap_err()
                .contains("invalid instruction")
        );

        // ADD is well-framed, but remains source-fallback territory.
        assert_eq!(validate_gnu_adapter_stream(&[12, 66, 0, 1], 1), Ok(false));
    }

    #[test]
    fn bounded_gnu_adapter_validates_superassignment_stack_and_pool() {
        assert_eq!(
            validate_gnu_adapter_stream(&[12, 20, 0, 95, 1, 1], 2),
            Ok(true)
        );
        assert!(validate_gnu_adapter_stream(&[12, 95, 0, 1], 1).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 0, 95, 2, 1], 2).is_err());
    }

    #[test]
    fn bounded_gnu_adapter_validates_settag_call_frames() {
        let valid = [12, 23, 0, 29, 1, 31, 2, 38, 3, 1];
        assert_eq!(validate_gnu_adapter_stream(&valid, 4), Ok(true));

        let no_frame = [12, 31, 0, 1];
        assert!(
            validate_gnu_adapter_stream(&no_frame, 1)
                .unwrap_err()
                .contains("SETTAG has no active GETFUN call")
        );

        let no_argument = [12, 23, 0, 31, 1, 38, 2, 1];
        assert!(
            validate_gnu_adapter_stream(&no_argument, 3)
                .unwrap_err()
                .contains("SETTAG has no preceding call argument")
        );

        let bad_tag = [12, 23, 0, 29, 1, 31, 3, 38, 2, 1];
        assert!(
            validate_gnu_adapter_stream(&bad_tag, 3)
                .unwrap_err()
                .contains("SETTAG constant pool index 3")
        );
    }

    #[test]
    fn bounded_gnu_adapter_validates_builtin_call_frames() {
        let valid = [12, 123, 0, 11, 26, 1, 20, 2, 33, 39, 0, 1];
        assert_eq!(validate_gnu_adapter_stream(&valid, 3), Ok(true));
        assert!(
            validate_gnu_adapter_stream(&[12, 33, 39, 0, 1], 1)
                .unwrap_err()
                .contains("PUSHARG has no active call")
        );
        assert!(
            validate_gnu_adapter_stream(&[12, 39, 0, 1], 1)
                .unwrap_err()
                .contains("CALL has no active GETFUN call")
        );
        assert!(validate_gnu_adapter_stream(&[12, 26, 2, 1], 2).is_err());
    }

    #[test]
    fn bounded_gnu_adapter_validates_checkfun_stack() {
        assert_eq!(
            validate_gnu_adapter_stream(&[12, 16, 0, 28, 29, 0, 38, 1, 1], 2),
            Ok(true)
        );
        assert!(
            validate_gnu_adapter_stream(&[12, 28, 1], 1)
                .unwrap_err()
                .contains("CHECKFUN has an empty stack")
        );
    }

    #[test]
    fn bounded_gnu_adapter_rejects_malformed_for_control_flow() {
        // LDCONST; STARTFOR -> STEPFOR; LDNULL/POP body; STEPFOR -> body;
        // ENDFOR; RETURN.
        let valid = [12, 16, 0, 11, 1, 2, 9, 17, 4, 12, 7, 13, 1];
        assert_eq!(validate_gnu_adapter_stream(&valid, 3), Ok(true));

        let mut start_to_body = valid;
        start_to_body[6] = 7;
        assert!(validate_gnu_adapter_stream(&start_to_body, 3).is_err());

        let mut stray_step = valid;
        stray_step[6] = 12;
        assert!(validate_gnu_adapter_stream(&stray_step, 3).is_err());

        let stray_end = [12, 17, 13, 1];
        assert!(
            validate_gnu_adapter_stream(&stray_end, 0)
                .unwrap_err()
                .contains("no active for loop")
        );

        let mut non_boundary = valid;
        non_boundary[10] = 10;
        assert!(
            validate_gnu_adapter_stream(&non_boundary, 3)
                .unwrap_err()
                .contains("instruction boundary")
        );

        let early_return = [12, 16, 0, 11, 1, 2, 11, 16, 0, 1, 4, 12, 7, 13, 1];
        assert_eq!(validate_gnu_adapter_stream(&early_return, 3), Ok(true));
    }

    #[test]
    fn bounded_gnu_calls_validate_frame_lifetime_and_preserve_outer_operands() {
        for words in [
            vec![12, 38, 0, 1],        // CALL without GETFUN
            vec![12, 29, 0, 1],        // MAKEPROM without GETFUN
            vec![12, 23, 0, 4, 17, 1], // POP destroys the active function
            vec![12, 23, 0, 1],        // unfinished call at RETURN
        ] {
            assert!(validate_gnu_adapter_stream(&words, 1).is_err(), "{words:?}");
        }
        // An argument call in the right operand of ADD must retain the left.
        assert_eq!(
            validate_gnu_adapter_stream(&[12, 16, 0, 23, 1, 29, 2, 38, 3, 44, 4, 1], 5),
            Ok(true)
        );
    }

    #[test]
    fn gnu_constant_arguments_validate_pool_indices_and_call_frames() {
        for words in [
            vec![12, 34, 0, 1],
            vec![12, 35, 1],
            vec![12, 36, 1],
            vec![12, 37, 1],
            vec![12, 23, 0, 34, -1, 38, 0, 1],
            vec![12, 23, 0, 34, 1, 38, 0, 1],
        ] {
            assert!(validate_gnu_adapter_stream(&words, 1).is_err(), "{words:?}");
        }
        for opcode in [35, 36, 37] {
            assert_eq!(
                validate_gnu_adapter_stream(&[12, 23, 0, opcode, 38, 0, 1], 1),
                Ok(true)
            );
        }
        assert_eq!(
            validate_gnu_adapter_stream(&[12, 23, 0, 34, 0, 38, 0, 1], 1),
            Ok(true)
        );
    }

    #[test]
    fn accepts_pinned_compiler_fixture_stream() {
        // `compiler:::disassemble(compiler::cmpfun(function(x)
        // if (x) x + 1 else 0))` under the pinned GNU R oracle reports this
        // exact integer stream (version 12, GETVAR, BRIFNOT, LDCONST, ADD,
        // RETURN).  The opcode names and operands are therefore sourced from
        // an actual compiler-produced fixture, not the private dialect.
        let fixture = [12, 20, 1, 3, 0, 13, 20, 1, 16, 2, 44, 3, 1, 16, 4, 17];
        assert!(validate_gnu_bytecode_stream(&fixture).is_ok());
    }

    #[test]
    fn gnu_dup_validator_rejects_underflow_and_stack_growth() {
        assert!(
            validate_gnu_adapter_stream(&[12, 5, 1], 0)
                .unwrap_err()
                .contains("empty stack")
        );
        let mut words = vec![12, 17];
        words.extend(std::iter::repeat_n(5, 64));
        words.push(1);
        assert!(
            validate_gnu_adapter_stream(&words, 0)
                .unwrap_err()
                .contains("stack limit")
        );
        assert!(validate_gnu_adapter_stream(&[12, 17, 5, 4, 1], 0).unwrap());
    }

    #[test]
    fn gnu_short_circuit_validator_accepts_and_or_cfg() {
        // compiler:::disassemble(cmpfun(function(x, y) x && y))
        let and_stream = [12, 20, 1, 88, 0, 10, 20, 2, 89, 0, 1];
        assert_eq!(validate_gnu_adapter_stream(&and_stream, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x, y) x || y))
        let or_stream = [12, 20, 1, 90, 0, 10, 20, 2, 91, 0, 1];
        assert_eq!(validate_gnu_adapter_stream(&or_stream, 3), Ok(true));
        let and_const = [12, 20, 1, 88, 0, 9, 18, 89, 0, 1];
        assert_eq!(validate_gnu_adapter_stream(&and_const, 2), Ok(true));

        let mut non_boundary = and_stream;
        non_boundary[5] = 9;
        assert!(
            validate_gnu_adapter_stream(&non_boundary, 3)
                .unwrap_err()
                .contains("instruction boundary")
        );
        assert!(validate_gnu_adapter_stream(&[12, 88, 0, 4, 1], 1).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 18, 89, 0, 1], 1).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 1, 88, 3, 10, 20, 2, 89, 0, 1], 3).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 1, 90, 0, 99, 20, 2, 91, 0, 1], 3).is_err());
    }

    #[test]
    fn gnu_vector_logic_validator_accepts_and_or_not() {
        // compiler:::disassemble(cmpfun(function(x, y) x & y))
        let and_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_GETVAR,
            2,
            GNU_OP_AND,
            0,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&and_stream, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x, y) x | y))
        let or_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_GETVAR,
            2,
            GNU_OP_OR,
            0,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&or_stream, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x) !x))
        let not_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_NOT,
            0,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&not_stream, 2), Ok(true));

        let underflow = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_AND,
            0,
            GNU_OP_RETURN,
        ];
        assert!(validate_gnu_adapter_stream(&underflow, 2).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 59, 0, 1], 1).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 1, 20, 2, 57, 3, 1], 3).is_err());
    }

    #[test]
    fn gnu_istype_validator_accepts_isnull_family() {
        // compiler:::disassemble(cmpfun(function(x) is.null(x), options=list(optimize=3)))
        let null_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_ISNULL,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&null_stream, 2), Ok(true));
        let integer_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_ISINTEGER,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&integer_stream, 2), Ok(true));
        let object_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_ISOBJECT,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&object_stream, 2), Ok(true));
        // ISNUMERIC is emitted only by older/hand-built streams; still accept it.
        let numeric_stream = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_ISNUMERIC,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&numeric_stream, 2), Ok(true));
        assert!(validate_gnu_adapter_stream(&[12, 75, 1], 1).is_err());
    }

    #[test]
    fn gnu_dollar_validator_accepts_get_and_assign_streams() {
        // compiler:::disassemble(cmpfun(function(x) x$a, options=list(optimize=3)))
        let dollar = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_DOLLAR,
            0,
            2,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&dollar, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x,v){x$a<-v;x}, options=list(optimize=3)))
        let dollargets = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_STARTASSIGN,
            2,
            GNU_OP_DOLLARGETS,
            4,
            5,
            GNU_OP_ENDASSIGN,
            2,
            GNU_OP_POP,
            GNU_OP_GETVAR,
            2,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&dollargets, 7), Ok(true));
        assert!(validate_gnu_adapter_stream(&[12, 73, 0, 1, 1], 2).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 0, 73, 2, 1, 1], 2).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 0, 61, 1, 1], 2).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 0, 74, 0, 1, 1], 2).is_err());
    }

    #[test]
    fn gnu_subset_n_validator_accepts_vector_get_and_assign_streams() {
        // compiler:::disassemble(cmpfun(function(x,i) x[i], options=list(optimize=3)))
        let subset = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_STARTSUBSET_N,
            0,
            10,
            GNU_OP_GETVAR_MISSOK,
            2,
            GNU_OP_VECSUBSET,
            0,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&subset, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x) x[1L], options=list(optimize=3)))
        let const_index = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_STARTSUBSET_N,
            0,
            10,
            GNU_OP_LDCONST,
            2,
            GNU_OP_VECSUBSET,
            0,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&const_index, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x,i) x[[i]], options=list(optimize=3)))
        let subset2 = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_STARTSUBSET2_N,
            0,
            10,
            GNU_OP_GETVAR_MISSOK,
            2,
            GNU_OP_VECSUBSET2,
            0,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&subset2, 3), Ok(true));
        // compiler:::disassemble(cmpfun(function(x,i,v){x[i]<-v;x}, options=list(optimize=3)))
        let assign = [
            GNU_BC_MAX_VERSION,
            GNU_OP_GETVAR,
            1,
            GNU_OP_STARTASSIGN,
            2,
            GNU_OP_STARTSUBASSIGN_N,
            4,
            12,
            GNU_OP_GETVAR_MISSOK,
            6,
            GNU_OP_VECSUBASSIGN,
            4,
            GNU_OP_ENDASSIGN,
            2,
            GNU_OP_POP,
            GNU_OP_GETVAR,
            2,
            GNU_OP_RETURN,
        ];
        assert_eq!(validate_gnu_adapter_stream(&assign, 7), Ok(true));
        assert!(validate_gnu_adapter_stream(&[12, 104, 0, 1, 1], 1).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 0, 84, 0, 1], 1).is_err());
        assert!(validate_gnu_adapter_stream(&[12, 20, 0, 104, 2, 6, 1], 1).is_err());
    }
}
