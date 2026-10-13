//! GNU bytecode stream validators and frame analysis — extracted verbatim from eval/bytecode.rs

use super::*;

/// Validate the integer instruction vector stored by GNU R's BCODESXP
/// serializer.  The first integer is the bytecode ABI version; each
/// subsequent opcode consumes the exact operand count from GNU R's table.
/// This validates framing and version/opcode identity, while operand meaning
/// (for example, whether a constant index is in range) remains a later
/// compiler/interpreter concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GnuFrameReject {
    Empty,
    Version {
        version: c_int,
    },
    UnknownOpcode {
        opcode: c_int,
        offset: usize,
    },
    RangeOverflow {
        opcode: c_int,
    },
    Truncated {
        opcode: c_int,
        offset: usize,
        width: usize,
    },
}

/// Next instruction boundary after the opcode at `opcode_pc`.
///
/// The advance is `1 + GNU_BC_OPERAND_WIDTHS[opcode]` when that opcode is
/// known and the operands fit. [`gnu_frame_decision`] and the GNU bytecode
/// executor both step with this function.
pub(crate) fn gnu_next_pc(
    opcode_pc: usize,
    opcode: c_int,
    len: usize,
) -> Result<usize, GnuFrameReject> {
    let Some(&width) = (opcode >= 0)
        .then(|| GNU_BC_OPERAND_WIDTHS.get(opcode as usize))
        .flatten()
    else {
        return Err(GnuFrameReject::UnknownOpcode {
            opcode,
            offset: opcode_pc,
        });
    };
    let width = width as usize;
    let Some(end) = opcode_pc
        .checked_add(1)
        .and_then(|pc| pc.checked_add(width))
    else {
        return Err(GnuFrameReject::RangeOverflow { opcode });
    };
    if end > len {
        return Err(GnuFrameReject::Truncated {
            opcode,
            offset: opcode_pc,
            width,
        });
    }
    Ok(end)
}

/// Framing decision without formatting. [`validate_gnu_bytecode_stream`] maps
/// each variant to the historical error string.
pub(crate) fn gnu_frame_decision(code: &[c_int]) -> Result<(), GnuFrameReject> {
    let Some(&version) = code.first() else {
        return Err(GnuFrameReject::Empty);
    };
    if !(GNU_BC_MIN_VERSION..=GNU_BC_MAX_VERSION).contains(&version) {
        return Err(GnuFrameReject::Version { version });
    }

    let mut pc = 1usize;
    while pc < code.len() {
        let opcode = code[pc];
        pc = gnu_next_pc(pc, opcode, code.len())?;
    }
    Ok(())
}

pub fn validate_gnu_bytecode_stream(code: &[c_int]) -> Result<(), String> {
    gnu_frame_decision(code).map_err(|reject| match reject {
        GnuFrameReject::Empty => "GNU R bytecode stream is empty".to_string(),
        GnuFrameReject::Version { version } => format!(
            "BCMISMATCH: unsupported GNU R bytecode version {version} (supported {GNU_BC_MIN_VERSION}..={GNU_BC_MAX_VERSION})"
        ),
        GnuFrameReject::UnknownOpcode { opcode, offset } => format!(
            "BCMISMATCH: unknown GNU R bytecode opcode {opcode} at stream offset {offset}"
        ),
        GnuFrameReject::RangeOverflow { opcode } => {
            format!("GNU R bytecode operand range overflows at opcode {opcode}")
        }
        GnuFrameReject::Truncated { opcode, offset, width } => format!(
            "truncated GNU R bytecode opcode {opcode} at stream offset {offset}: expected {width} operand(s)"
        ),
    })
}

#[cfg(kani)]
pub(crate) mod kani_proofs {
    use super::super::{
        GNU_BC_MAX_VERSION, GNU_BC_MIN_VERSION, GNU_BC_OPCODE_COUNT, GNU_BC_OPERAND_WIDTHS,
        gnu_frame_decision,
    };

    /// Independent framing walk. A mutant in the production validator must
    /// disagree with this check. To see that, invert `end > code.len()` in
    /// `validate_gnu_bytecode_stream` and rerun this harness; Kani reports
    /// a failure. That mutant is not part of the default build.
    fn independent_frame_ok(code: &[i32]) -> bool {
        let Some(&version) = code.first() else {
            return false;
        };
        if !(GNU_BC_MIN_VERSION..=GNU_BC_MAX_VERSION).contains(&version) {
            return false;
        }
        let mut seen = [false; 12];
        seen[0] = true;
        let mut pc = 1usize;
        while pc < code.len() {
            let opcode = code[pc];
            let Some(&width) = GNU_BC_OPERAND_WIDTHS.get(opcode as usize) else {
                return false;
            };
            let width = width as usize;
            let Some(end) = (pc + 1).checked_add(width) else {
                return false;
            };
            if end > code.len() {
                return false;
            }
            for index in pc..end {
                if seen[index] {
                    return false;
                }
                seen[index] = true;
            }
            pc = end;
        }
        pc == code.len() && seen[..code.len()].iter().all(|slot| *slot)
    }

    #[kani::proof]
    fn kani_trivial_ok() {
        assert_eq!(1 + 1, 2);
    }

    #[kani::proof]
    #[kani::unwind(13)]
    fn validate_gnu_stream_partition() {
        let mut raw = [0i32; 12];
        let len: usize = kani::any();
        kani::assume(len <= 12);
        for slot in &mut raw {
            *slot = kani::any();
        }
        let code = &raw[..len];
        let result = gnu_frame_decision(code);
        assert_eq!(result.is_ok(), independent_frame_ok(code));
        if let Ok(()) = result {
            assert!(independent_frame_ok(code));
        }
        let unknown = len >= 2
            && code[0] == GNU_BC_MAX_VERSION
            && (code[1] < 0 || code[1] as usize >= GNU_BC_OPCODE_COUNT);
        if unknown {
            assert!(result.is_err());
        }
        kani::cover(
            code.len() >= 2 && code[0] == GNU_BC_MAX_VERSION && code[1] == 0 && result.is_ok(),
            "reachable",
        );
        kani::cover(
            code.len() == 2 && code[0] == GNU_BC_MAX_VERSION && code[1] == 2 && result.is_err(),
            "reachable",
        );
        kani::cover(code.is_empty() && result.is_err(), "reachable");
    }

    /// Every opcode's step is the width table. Invert `end > len` in
    /// `gnu_next_pc` and this harness fails. That mutant is not in the default build.
    #[kani::proof]
    fn gnu_next_pc_matches_width_table() {
        let opcode: i32 = kani::any();
        kani::assume((0..GNU_BC_OPCODE_COUNT as i32).contains(&opcode));
        let opcode_pc: usize = kani::any();
        let len: usize = kani::any();
        kani::assume(opcode_pc <= 8 && len <= 16);
        let width = GNU_BC_OPERAND_WIDTHS[opcode as usize] as usize;
        let result = super::super::gnu_next_pc(opcode_pc, opcode, len);
        let summed = opcode_pc
            .checked_add(1)
            .and_then(|pc| pc.checked_add(width));
        match (result, summed) {
            (Ok(end), Some(expect)) => {
                assert_eq!(end, expect);
                assert!(end <= len);
            }
            (Err(super::super::GnuFrameReject::Truncated { .. }), Some(expect)) => {
                assert!(expect > len);
            }
            (Err(super::super::GnuFrameReject::RangeOverflow { .. }), None) => {}
            _ => assert!(false),
        }
        let unknown = super::super::gnu_next_pc(0, -1, 4);
        assert!(matches!(
            unknown,
            Err(super::super::GnuFrameReject::UnknownOpcode { .. })
        ));
        kani::cover(result.is_ok(), "advances");
        kani::cover(
            matches!(result, Err(super::super::GnuFrameReject::Truncated { .. })),
            "truncated",
        );
    }
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

/// Native deserialization boundary. Snapshot instructions and acquire the
/// original pool entries before lazy table readers can run callbacks.
/// # Safety
/// The inputs are live projections of the active interpreter on entry.
pub unsafe fn validate_gnu_adapter_with_constants(
    code: &[c_int],
    constants: SEXP,
) -> Result<bool, String> {
    let mut instructions = Vec::new();
    instructions
        .try_reserve_exact(code.len())
        .map_err(|_| "cannot reserve bytecode validation instructions")?;
    instructions.extend_from_slice(code);
    let owner =
        unsafe { crate::sexp::owner::OwnerToken::current() }.map_err(|error| error.to_string())?;
    let pin = owner.pin().map_err(|error| error.to_string())?;
    let pool = owner
        .sexp(constants)
        .and_then(Sexp::into_owned)
        .map_err(|error| error.to_string())?;
    if pool.typeof_() != SEXPTYPE::VECSXP {
        return Err("GNU bytecode has no constant pool".into());
    }
    let length = usize::try_from(pool.len()).map_err(|_| "invalid constant pool length")?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(length)
        .map_err(|_| "cannot reserve constant pool")?;
    for index in 0..length {
        entries.push(
            pool.try_vector_elt(index as i64)
                .map_err(|error| error.to_string())?,
        );
    }
    let (supported, _tables) = validate_gnu_adapter_with_owned_constants(&instructions, &entries)?;
    if let Some(pin) = &pin {
        pin.require_live().map_err(|error| error.to_string())?;
    }
    Ok(supported)
}

/// Resolve STARTFOR's end operand to the STEPFOR it owns.
///
/// A plain for-loop jumps straight to STEPFOR. When the body may eval() /
/// return / next / break, GNU wraps it as
/// STARTFOR -> STARTLOOPCNTXT; GOTO STEPFOR and
/// STEPFOR; ENDLOOPCNTXT; ENDFOR.
pub(crate) fn gnu_for_step_from_start_target(
    code: &[c_int],
    target: usize,
) -> Result<usize, String> {
    match code.get(target).copied() {
        Some(GNU_OP_STEPFOR) => Ok(target),
        Some(GNU_OP_STARTLOOPCNTXT) => {
            let isfor = code.get(target + 1).copied().unwrap_or(-1);
            let after = gnu_next_pc(target, GNU_OP_STARTLOOPCNTXT, code.len()).map_err(|_| {
                format!("GNU STARTFOR entry target {target} is not a STEPFOR instruction")
            })?;
            if isfor != 1 || code.get(after) != Some(&GNU_OP_GOTO) {
                return Err(format!(
                    "GNU STARTFOR entry target {target} is not a STEPFOR instruction"
                ));
            }
            let step = code.get(after + 1).copied().ok_or_else(|| {
                format!("GNU STARTFOR entry target {target} is not a STEPFOR instruction")
            })? as usize;
            if code.get(step) != Some(&GNU_OP_STEPFOR) {
                return Err(format!(
                    "GNU STARTFOR entry target {target} is not a STEPFOR instruction"
                ));
            }
            Ok(step)
        }
        _ => Err(format!(
            "GNU STARTFOR entry target {target} is not a STEPFOR instruction"
        )),
    }
}

pub(crate) fn gnu_stepfor_exits_through_endfor(code: &[c_int], step_pc: usize) -> bool {
    let Ok(next) = gnu_next_pc(step_pc, GNU_OP_STEPFOR, code.len()) else {
        return false;
    };
    if code.get(next) == Some(&GNU_OP_ENDFOR) {
        return true;
    }
    if code.get(next) != Some(&GNU_OP_ENDLOOPCNTXT) {
        return false;
    }
    let isfor = code.get(next + 1).copied();
    let Ok(after) = gnu_next_pc(next, GNU_OP_ENDLOOPCNTXT, code.len()) else {
        return false;
    };
    isfor == Some(1) && code.get(after) == Some(&GNU_OP_ENDFOR)
}

/// Innermost `STARTLOOPCNTXT` that syntactically contains `at`.
///
/// GNU places `DOLOOPNEXT` / `DOLOOPBREAK` between that opcode and its
/// `ENDLOOPCNTXT`. The break and next edges are already queued from
/// `STARTLOOPCNTXT`, so the jump itself does not continue this path.
pub(crate) fn gnu_enclosing_loop(
    code: &[c_int],
    at: usize,
    opcode: c_int,
) -> Result<(usize, usize), String> {
    let mut pc = 1usize;
    let mut open: Vec<(usize, usize)> = Vec::new();
    while pc < at && pc < code.len() {
        let op = code[pc];
        let next = gnu_next_pc(pc, op, code.len())
            .map_err(|_| format!("GNU bytecode control flow reaches invalid instruction {pc}"))?;
        if op == GNU_OP_STARTLOOPCNTXT {
            let break_target = code[pc + 2] as usize;
            open.push((break_target, next));
        } else if op == GNU_OP_ENDLOOPCNTXT && open.pop().is_none() {
            return Err(format!(
                "GNU ENDLOOPCNTXT at instruction {pc} has no active loop context"
            ));
        }
        pc = next;
    }
    let name = if opcode == GNU_OP_DOLOOPNEXT {
        "DOLOOPNEXT"
    } else {
        "DOLOOPBREAK"
    };
    open.last()
        .copied()
        .ok_or_else(|| format!("GNU {name} has no active loop context"))
}

pub(crate) fn validate_gnu_adapter_impl(
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
            GNU_OP_RETURN
            | GNU_OP_INVISIBLE
            | GNU_OP_LDNULL
            | GNU_OP_LDTRUE
            | GNU_OP_LDFALSE
            | GNU_OP_POP
            | GNU_OP_ENDFOR
            | GNU_OP_PUSHNULLARG
            | GNU_OP_PUSHTRUEARG
            | GNU_OP_PUSHFALSEARG
            | GNU_OP_PUSHARG
            | GNU_OP_CHECKFUN
            | GNU_OP_DUP
            | GNU_OP_PRINTVALUE
            | GNU_OP_SETLOOPVAL
            | GNU_OP_DOTSERR
            | GNU_OP_ISNULL
            | GNU_OP_ISLOGICAL
            | GNU_OP_ISINTEGER
            | GNU_OP_ISDOUBLE
            | GNU_OP_ISCOMPLEX
            | GNU_OP_ISCHARACTER
            | GNU_OP_ISSYMBOL
            | GNU_OP_ISOBJECT
            | GNU_OP_ISNUMERIC
            | GNU_OP_DOMISSING
            | GNU_OP_DFLTSUBSET
            | GNU_OP_DFLTSUBASSIGN
            | GNU_OP_DFLTSUBASSIGN2
            | GNU_OP_DFLTSUBSET2
            | GNU_OP_SWAP
            | GNU_OP_DUP2ND
            | GNU_OP_RETURNJMP
            | GNU_OP_DOLOOPNEXT
            | GNU_OP_DOLOOPBREAK => {}
            GNU_OP_STARTLOOPCNTXT => {
                let isfor = code[pc];
                let target = code[pc + 1];
                if isfor != 0 && isfor != 1 {
                    return Err(format!(
                        "GNU STARTLOOPCNTXT isfor flag {isfor} is not 0 or 1"
                    ));
                }
                if target < 0 || target as usize >= code.len() {
                    return Err(format!(
                        "GNU STARTLOOPCNTXT end target {target} is outside instruction stream length {}",
                        code.len()
                    ));
                }
                branches.push((opcode_pc, target as usize));
            }
            GNU_OP_ENDLOOPCNTXT => {
                let isfor = code[pc];
                if isfor != 0 && isfor != 1 {
                    return Err(format!("GNU ENDLOOPCNTXT isfor flag {isfor} is not 0 or 1"));
                }
            }
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
            GNU_OP_STARTASSIGN2 | GNU_OP_ENDASSIGN2 => {
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
            GNU_OP_SETTER_CALL => {
                for slot in 0..2 {
                    let index = code[pc + slot];
                    if index < 0 {
                        return Err(format!(
                            "GNU SETTER_CALL constant pool index {index} is negative"
                        ));
                    }
                    if index as usize >= constant_count {
                        return Err(format!(
                            "GNU SETTER_CALL constant pool index {index} is out of range for pool length {constant_count}"
                        ));
                    }
                }
            }
            GNU_OP_GETTER_CALL => {
                let index = code[pc];
                if index < 0 {
                    return Err(format!(
                        "GNU GETTER_CALL constant pool index {index} is negative"
                    ));
                }
                if index as usize >= constant_count {
                    return Err(format!(
                        "GNU GETTER_CALL constant pool index {index} is out of range for pool length {constant_count}"
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
            GNU_OP_STARTSUBSET
            | GNU_OP_STARTSUBSET2
            | GNU_OP_STARTSUBSET_N
            | GNU_OP_STARTSUBSET2_N
            | GNU_OP_STARTSUBASSIGN_N
            | GNU_OP_STARTSUBASSIGN2_N
            | GNU_OP_STARTSUBASSIGN
            | GNU_OP_STARTSUBASSIGN2 => {
                let call_index = code[pc];
                let target = code[pc + 1];
                let name = match opcode {
                    GNU_OP_STARTSUBSET => "STARTSUBSET",
                    GNU_OP_STARTSUBSET2 => "STARTSUBSET2",
                    GNU_OP_STARTSUBSET_N => "STARTSUBSET_N",
                    GNU_OP_STARTSUBSET2_N => "STARTSUBSET2_N",
                    GNU_OP_STARTSUBASSIGN => "STARTSUBASSIGN",
                    GNU_OP_STARTSUBASSIGN2_N => "STARTSUBASSIGN2_N",
                    GNU_OP_STARTSUBASSIGN2 => "STARTSUBASSIGN2",
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
            GNU_OP_VECSUBSET | GNU_OP_VECSUBSET2 | GNU_OP_VECSUBASSIGN | GNU_OP_MATSUBSET
            | GNU_OP_MATSUBASSIGN | GNU_OP_VECSUBASSIGN2 | GNU_OP_MATSUBSET2
            | GNU_OP_MATSUBASSIGN2 => {
                let call_index = code[pc];
                let name = match opcode {
                    GNU_OP_VECSUBSET => "VECSUBSET",
                    GNU_OP_VECSUBSET2 => "VECSUBSET2",
                    GNU_OP_MATSUBSET => "MATSUBSET",
                    GNU_OP_MATSUBASSIGN => "MATSUBASSIGN",
                    GNU_OP_VECSUBASSIGN2 => "VECSUBASSIGN2",
                    GNU_OP_MATSUBSET2 => "MATSUBSET2",
                    GNU_OP_MATSUBASSIGN2 => "MATSUBASSIGN2",
                    _ => "VECSUBASSIGN",
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_SUBSET_N | GNU_OP_SUBSET2_N => {
                let call_index = code[pc];
                let rank = code[pc + 1];
                let name = if opcode == GNU_OP_SUBSET2_N {
                    "SUBSET2_N"
                } else {
                    "SUBSET_N"
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                if rank < 0 {
                    return Err(format!("GNU {name} rank {rank} is negative"));
                }
            }
            GNU_OP_SUBASSIGN_N | GNU_OP_SUBASSIGN2_N => {
                let call_index = code[pc];
                let rank = code[pc + 1];
                let name = if opcode == GNU_OP_SUBASSIGN2_N {
                    "SUBASSIGN2_N"
                } else {
                    "SUBASSIGN_N"
                };
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU {name} expression index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                if rank < 0 {
                    return Err(format!("GNU {name} rank {rank} is negative"));
                }
            }
            GNU_OP_LDCONST
            | GNU_OP_GETVAR
            | GNU_OP_GETVAR_MISSOK
            | GNU_OP_DDVAL
            | GNU_OP_DDVAL_MISSOK
            | GNU_OP_GETFUN
            | GNU_OP_GETBUILTIN
            | GNU_OP_GETINTLBUILTIN
            | GNU_OP_GETGLOBFUN
            | GNU_OP_GETSYMFUN
            | GNU_OP_MAKEPROM
            | GNU_OP_PUSHCONSTARG
            | GNU_OP_UMINUS
            | GNU_OP_UPLUS
            | GNU_OP_ADD
            | GNU_OP_SUB
            | GNU_OP_MUL
            | GNU_OP_DIV
            | GNU_OP_EXPT
            | GNU_OP_EQ
            | GNU_OP_NE
            | GNU_OP_LT
            | GNU_OP_LE
            | GNU_OP_GE
            | GNU_OP_GT
            | GNU_OP_AND
            | GNU_OP_OR
            | GNU_OP_NOT
            | GNU_OP_SQRT
            | GNU_OP_EXP
            | GNU_OP_LOG
            | GNU_OP_LOGBASE
            | GNU_OP_SETVAR
            | GNU_OP_SETVAR2
            | GNU_OP_COLON
            | GNU_OP_SEQALONG
            | GNU_OP_SEQLEN => {
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
            GNU_OP_MATH1 => {
                let call_index = code[pc];
                let math_index = code[pc + 1];
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU MATH1 constant pool index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                // This is the fixed math1funs[] order in GNU eval.c.
                if !(0..24).contains(&math_index) {
                    return Err(format!("GNU MATH1 function index {math_index} is invalid"));
                }
            }
            GNU_OP_DOTCALL => {
                let call_index = code[pc];
                let nargs = code[pc + 1];
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU DOTCALL constant pool index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
                if nargs < 0 {
                    return Err(format!("GNU DOTCALL nargs {nargs} is negative"));
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
            GNU_OP_MAKECLOSURE => {
                let index = code[pc];
                if index < 0 || index as usize >= constant_count {
                    return Err(format!(
                        "GNU MAKECLOSURE constant index {index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_CALLSPECIAL => {
                let call_index = code[pc];
                if call_index < 0 || call_index as usize >= constant_count {
                    return Err(format!(
                        "GNU CALLSPECIAL call index {call_index} is out of range for pool length {constant_count}"
                    ));
                }
            }
            GNU_OP_DODOTS | GNU_OP_VISIBLE | GNU_OP_INCLNK | GNU_OP_DECLNK | GNU_OP_INCLNKSTK
            | GNU_OP_DECLNKSTK => {}
            GNU_OP_DECLNK_N => {
                let count = code[pc];
                if count < 0 {
                    return Err(format!("GNU DECLNK_N count {count} is negative"));
                }
            }
            _ => supported = false,
        }
        pc = gnu_next_pc(opcode_pc, opcode, code.len()).map_err(|reject| match reject {
            GnuFrameReject::UnknownOpcode { opcode, offset } => format!(
                "BCMISMATCH: unknown GNU R bytecode opcode {opcode} at stream offset {offset}"
            ),
            GnuFrameReject::RangeOverflow { opcode } => {
                format!("GNU R bytecode operand range overflows at opcode {opcode}")
            }
            GnuFrameReject::Truncated { opcode, offset, width } => format!(
                "truncated GNU R bytecode opcode {opcode} at stream offset {offset}: expected {width} operand(s)"
            ),
            GnuFrameReject::Empty | GnuFrameReject::Version { .. } => {
                "GNU R bytecode stream is empty".to_string()
            }
        })?;
    }

    for (branch_pc, target) in branches {
        if !boundaries[target] {
            return Err(format!(
                "GNU BRIFNOT jump target {target} is not an instruction boundary"
            ));
        }
        if supported
            && target <= branch_pc
            && code[branch_pc] != GNU_OP_STEPFOR
            && code[branch_pc] != GNU_OP_GOTO
        {
            return Err(format!(
                "GNU BRIFNOT backward jump from {branch_pc} to {target} is outside the bounded adapter"
            ));
        }
    }
    let mut resolved_for_steps = Vec::new();
    for entry_pc in &for_entry_targets {
        resolved_for_steps.push(gnu_for_step_from_start_target(code, *entry_pc)?);
    }
    for step_pc in for_steps {
        if !resolved_for_steps.contains(&step_pc) {
            return Err(format!(
                "GNU STEPFOR at instruction {step_pc} is not the entry target of STARTFOR"
            ));
        }
        if !gnu_stepfor_exits_through_endfor(code, step_pc) {
            return Err(format!(
                "GNU STEPFOR at instruction {step_pc} is not followed by ENDFOR"
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
                // Merge disagreement: GNU compiler can produce CFGs our
                // stack-effect model does not yet prove. Not malformed —
                // fall back to the retained source expression.
                return Ok(false);
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
        let Ok(next) = gnu_next_pc(instruction_pc, opcode, code.len()) else {
            return Ok(false);
        };
        match opcode {
            GNU_OP_RETURN | GNU_OP_RETURNJMP => {
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
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_POP | GNU_OP_PRINTVALUE | GNU_OP_SETLOOPVAL => {
                if depth == 0 || (opcode == GNU_OP_SETLOOPVAL && depth < 2) {
                    return Err(format!(
                        "GNU opcode {opcode} at instruction {instruction_pc} has stack depth {depth}"
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
                    return Ok(false);
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
            GNU_OP_STARTASSIGN2 => {
                if depth == 0 {
                    return Err(format!(
                        "GNU STARTASSIGN2 at instruction {instruction_pc} has an empty stack"
                    ));
                }
                if depth + 3 > 64 {
                    return Ok(false);
                }
                pending.push((next, depth + 3, loop_stack, call_stack.clone()));
            }
            GNU_OP_ENDASSIGN2 => {
                if depth < 3 {
                    return Err(format!(
                        "GNU ENDASSIGN2 at instruction {instruction_pc} has stack depth {depth}, requires 3"
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
            GNU_OP_STARTSUBSET | GNU_OP_STARTSUBSET2 => {
                if depth == 0 {
                    return Err(format!(
                        "GNU STARTSUBSET at instruction {instruction_pc} has an empty stack"
                    ));
                }
                if depth + 1 > 64 {
                    return Ok(false);
                }
                let mut entered = call_stack.clone();
                entered.push(depth - 1);
                pending.push((next, depth, loop_stack.clone(), entered));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth,
                    loop_stack,
                    call_stack.clone(),
                ));
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
            GNU_OP_STARTSUBASSIGN | GNU_OP_STARTSUBASSIGN2 => {
                if depth < 2 {
                    return Err(format!(
                        "GNU STARTSUBASSIGN at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                if depth + 1 > 64 {
                    return Ok(false);
                }
                let mut entered = call_stack.clone();
                entered.push(depth - 2);
                pending.push((next, depth, loop_stack.clone(), entered));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth - 1,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_DOMISSING => {
                if call_stack.is_empty() {
                    return Err(format!(
                        "GNU DOMISSING at instruction {instruction_pc} has no active call frame"
                    ));
                }
                pending.push((next, depth + 1, loop_stack, call_stack));
            }
            GNU_OP_DFLTSUBSET | GNU_OP_DFLTSUBSET2 => {
                let Some(marker) = call_stack.pop() else {
                    return Err(format!(
                        "GNU DFLTSUBSET at instruction {instruction_pc} has no active STARTSUBSET frame"
                    ));
                };
                if depth <= marker {
                    return Err(format!(
                        "GNU DFLTSUBSET at instruction {instruction_pc} has stack depth {depth}, requires object"
                    ));
                }
                pending.push((next, marker + 1, loop_stack, call_stack));
            }
            GNU_OP_STARTSUBASSIGN_N | GNU_OP_STARTSUBASSIGN2_N => {
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
            GNU_OP_DFLTSUBASSIGN | GNU_OP_DFLTSUBASSIGN2 => {
                let Some(marker) = call_stack.pop() else {
                    return Err(format!(
                        "GNU DFLTSUBASSIGN at instruction {instruction_pc} has no active STARTSUBASSIGN frame"
                    ));
                };
                if depth < marker + 2 {
                    return Err(format!(
                        "GNU DFLTSUBASSIGN at instruction {instruction_pc} has stack depth {depth}, requires lhs and rhs"
                    ));
                }
                pending.push((next, marker + 1, loop_stack, call_stack));
            }
            GNU_OP_VECSUBSET | GNU_OP_VECSUBSET2 => {
                if depth < 2 {
                    return Err(format!(
                        "GNU VECSUBSET at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_MATSUBSET => {
                if depth < 3 {
                    return Err(format!(
                        "GNU MATSUBSET at instruction {instruction_pc} has stack depth {depth}, requires 3"
                    ));
                }
                pending.push((next, depth - 2, loop_stack, call_stack.clone()));
            }
            GNU_OP_MATSUBSET2 => {
                if depth < 3 {
                    return Err(format!(
                        "GNU MATSUBSET2 at instruction {instruction_pc} has stack depth {depth}, requires 3"
                    ));
                }
                pending.push((next, depth - 2, loop_stack, call_stack.clone()));
            }
            GNU_OP_SUBSET_N | GNU_OP_SUBSET2_N => {
                let rank = code[instruction_pc + 2];
                let name = if opcode == GNU_OP_SUBSET2_N {
                    "SUBSET2_N"
                } else {
                    "SUBSET_N"
                };
                if rank < 0 {
                    return Err(format!(
                        "GNU {name} at instruction {instruction_pc} has negative rank {rank}"
                    ));
                }
                let needed = rank as usize + 1;
                if depth < needed {
                    return Err(format!(
                        "GNU {name} at instruction {instruction_pc} has stack depth {depth}, requires {needed}"
                    ));
                }
                pending.push((next, depth - rank as usize, loop_stack, call_stack.clone()));
            }
            GNU_OP_SUBASSIGN_N | GNU_OP_SUBASSIGN2_N => {
                let rank = code[instruction_pc + 2];
                let name = if opcode == GNU_OP_SUBASSIGN2_N {
                    "SUBASSIGN2_N"
                } else {
                    "SUBASSIGN_N"
                };
                if rank < 0 {
                    return Err(format!(
                        "GNU {name} at instruction {instruction_pc} has negative rank {rank}"
                    ));
                }
                let needed = rank as usize + 2;
                if depth < needed {
                    return Err(format!(
                        "GNU {name} at instruction {instruction_pc} has stack depth {depth}, requires {needed}"
                    ));
                }
                pending.push((
                    next,
                    depth - rank as usize - 1,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_VECSUBASSIGN | GNU_OP_VECSUBASSIGN2 => {
                if depth < 3 {
                    return Err(format!(
                        "GNU VECSUBASSIGN at instruction {instruction_pc} has stack depth {depth}, requires 3"
                    ));
                }
                pending.push((next, depth - 2, loop_stack, call_stack.clone()));
            }
            GNU_OP_MATSUBASSIGN => {
                if depth < 4 {
                    return Err(format!(
                        "GNU MATSUBASSIGN at instruction {instruction_pc} has stack depth {depth}, requires 4"
                    ));
                }
                pending.push((next, depth - 3, loop_stack, call_stack.clone()));
            }
            GNU_OP_MATSUBASSIGN2 => {
                if depth < 4 {
                    return Err(format!(
                        "GNU MATSUBASSIGN2 at instruction {instruction_pc} has stack depth {depth}, requires 4"
                    ));
                }
                pending.push((next, depth - 3, loop_stack, call_stack.clone()));
            }
            GNU_OP_STARTFOR => {
                if depth == 0 {
                    return Err(format!(
                        "GNU STARTFOR at instruction {instruction_pc} has an empty stack"
                    ));
                }
                let raw_target = code[instruction_pc + 3] as usize;
                let step_pc = gnu_for_step_from_start_target(code, raw_target)?;
                let mut entered = loop_stack;
                entered.push(step_pc);
                pending.push((raw_target, depth, entered, call_stack.clone()));
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
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth + 1,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_LDCONST | GNU_OP_GETVAR | GNU_OP_GETVAR_MISSOK | GNU_OP_DDVAL
            | GNU_OP_DDVAL_MISSOK | GNU_OP_LDNULL | GNU_OP_LDTRUE | GNU_OP_LDFALSE => {
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_DODOTS => {
                // Argument splicing: the number of pushed cells is only
                // known at runtime; CALL pops back to the frame marker.
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_MAKECLOSURE | GNU_OP_CALLSPECIAL => {
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_ADD | GNU_OP_SUB | GNU_OP_MUL | GNU_OP_DIV | GNU_OP_EXPT | GNU_OP_EQ
            | GNU_OP_NE | GNU_OP_LT | GNU_OP_LE | GNU_OP_GE | GNU_OP_GT | GNU_OP_AND
            | GNU_OP_OR | GNU_OP_LOGBASE | GNU_OP_COLON => {
                if depth < 2 {
                    return Err(format!(
                        "GNU binary opcode {opcode} at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_UMINUS | GNU_OP_UPLUS | GNU_OP_SQRT | GNU_OP_EXP | GNU_OP_NOT | GNU_OP_LOG
            | GNU_OP_MATH1 | GNU_OP_ISNULL | GNU_OP_ISLOGICAL | GNU_OP_ISINTEGER
            | GNU_OP_ISDOUBLE | GNU_OP_ISCOMPLEX | GNU_OP_ISCHARACTER | GNU_OP_ISSYMBOL
            | GNU_OP_ISOBJECT | GNU_OP_ISNUMERIC | GNU_OP_SEQALONG | GNU_OP_SEQLEN => {
                if depth < 1 {
                    return Err(format!(
                        "GNU unary opcode {opcode} at instruction {instruction_pc} has empty stack"
                    ));
                }
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_GETFUN
            | GNU_OP_GETBUILTIN
            | GNU_OP_GETINTLBUILTIN
            | GNU_OP_GETGLOBFUN
            | GNU_OP_GETSYMFUN
            | GNU_OP_MAKEPROM
            | GNU_OP_PUSHCONSTARG
            | GNU_OP_PUSHNULLARG
            | GNU_OP_PUSHTRUEARG
            | GNU_OP_PUSHFALSEARG => {
                if matches!(
                    opcode,
                    GNU_OP_GETFUN
                        | GNU_OP_GETBUILTIN
                        | GNU_OP_GETINTLBUILTIN
                        | GNU_OP_GETGLOBFUN
                        | GNU_OP_GETSYMFUN
                ) {
                    call_stack.push(depth);
                } else if call_stack.is_empty() {
                    return Err(format!(
                        "GNU argument opcode {opcode} has no active GETFUN call"
                    ));
                }
                pending.push((next, depth + 1, loop_stack, call_stack));
            }
            GNU_OP_DOTSERR => {
                // GNU emits DOTSERR as a tail error; no RETURN follows.
                saw_return = true;
            }
            GNU_OP_DOTCALL => {
                let nargs = code[instruction_pc + 2];
                if nargs < 0 {
                    return Err(format!("GNU DOTCALL nargs {nargs} is negative"));
                }
                let needed = nargs as usize + 1;
                if depth < needed {
                    return Err(format!(
                        "GNU DOTCALL at instruction {instruction_pc} has stack depth {depth}, requires {needed}"
                    ));
                }
                pending.push((next, depth - nargs as usize, loop_stack, call_stack.clone()));
            }
            GNU_OP_VISIBLE => {
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_INCLNK | GNU_OP_DECLNK | GNU_OP_DECLNK_N => {
                let needed = if matches!(opcode, GNU_OP_INCLNK) {
                    1
                } else {
                    2
                };
                if depth < needed {
                    return Err(format!(
                        "GNU link opcode {opcode} at instruction {instruction_pc} has stack depth {depth}, requires {needed}"
                    ));
                }
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_INCLNKSTK => {
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_DECLNKSTK => {
                if depth < 2 {
                    return Err(format!(
                        "GNU DECLNKSTK at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth - 1, loop_stack, call_stack.clone()));
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
            GNU_OP_SETTER_CALL => {
                let Some(marker) = call_stack.pop() else {
                    return Err(format!(
                        "GNU SETTER_CALL at instruction {instruction_pc} has no active GETFUN call"
                    ));
                };
                if marker < 2 {
                    return Err(format!(
                        "GNU SETTER_CALL at instruction {instruction_pc} has an empty stack"
                    ));
                }
                if depth <= marker {
                    return Err(format!(
                        "GNU SETTER_CALL at instruction {instruction_pc} has no function"
                    ));
                }
                pending.push((next, marker - 1, loop_stack, call_stack));
            }
            GNU_OP_GETTER_CALL => {
                let Some(marker) = call_stack.pop() else {
                    return Err(format!(
                        "GNU GETTER_CALL at instruction {instruction_pc} has no active GETFUN call"
                    ));
                };
                if marker < 2 {
                    return Err(format!(
                        "GNU GETTER_CALL at instruction {instruction_pc} has an empty stack"
                    ));
                }
                if depth <= marker {
                    return Err(format!(
                        "GNU GETTER_CALL at instruction {instruction_pc} has no function"
                    ));
                }
                pending.push((next, marker + 1, loop_stack, call_stack));
            }
            GNU_OP_SWAP => {
                if depth < 2 {
                    return Err(format!(
                        "GNU SWAP at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            GNU_OP_DUP2ND => {
                if depth < 2 {
                    return Err(format!(
                        "GNU DUP2ND at instruction {instruction_pc} has stack depth {depth}, requires 2"
                    ));
                }
                pending.push((next, depth + 1, loop_stack, call_stack.clone()));
            }
            GNU_OP_INVISIBLE => pending.push((next, depth, loop_stack, call_stack.clone())),
            GNU_OP_DOLOOPNEXT | GNU_OP_DOLOOPBREAK => {
                gnu_enclosing_loop(code, instruction_pc, opcode)?;
            }
            GNU_OP_STARTLOOPCNTXT => {
                pending.push((next, depth, loop_stack.clone(), call_stack.clone()));
                pending.push((
                    code[instruction_pc + 2] as usize,
                    depth,
                    loop_stack,
                    call_stack.clone(),
                ));
            }
            GNU_OP_ENDLOOPCNTXT => {
                pending.push((next, depth, loop_stack, call_stack.clone()));
            }
            _ => unreachable!(),
        }
    }
    if !saw_return {
        return Err("GNU bytecode has no reachable RETURN".into());
    }
    Ok(true)
}
