//! Frozen, checked branch tables for GNU SWITCH bytecode.
//! Validation and execution share copied targets and original owning names.
use crate::sexp::ffi::{NA_INTEGER, SEXPTYPE};
use crate::sexp::object::Sexp;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct OwnedSwitch {
    call: Sexp<'static>,
    names: Option<Vec<Sexp<'static>>>,
    characters: Vec<usize>,
    numeric: Vec<usize>,
}

impl OwnedSwitch {
    pub(super) fn successors(&self) -> Vec<usize> {
        self.numeric
            .iter()
            .chain(self.characters.iter())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn select(
        &self,
        value: &Sexp<'static>,
        pin: &crate::sexp::owner::OwnerPin,
    ) -> Result<usize, String> {
        pin.require_live().map_err(|error| error.to_string())?;
        let kind = value.typeof_();
        if !matches!(
            kind,
            SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP
                | SEXPTYPE::VECSXP
                | SEXPTYPE::EXPRSXP
        ) || value.len() != 1
        {
            return Err("EXPR must be a length 1 vector".into());
        }
        // Own the selected character before warning handlers can alter value.
        let selector = if kind == SEXPTYPE::STRSXP {
            Some(value.try_string_elt(0).map_err(|error| error.to_string())?)
        } else {
            None
        };
        // Native condition/coercion adapters copy results; no R payload loan
        // survives callbacks. All branch targets and names are already owned.
        pin.require_live().map_err(|error| error.to_string())?;
        let factor =
            unsafe { crate::mainutils::connections::inherits_class(value.as_raw(), "factor") };
        pin.require_live().map_err(|error| error.to_string())?;
        if factor {
            unsafe {
                crate::mainutils::errors::warningcall(self.call.as_raw(),
                c"EXPR is a \"factor\", treated as integer.\n Consider using 'switch(as.character( * ), ...)' instead.".as_ptr());
            }
            pin.require_live().map_err(|error| error.to_string())?;
        }
        if let Some(selector) = selector {
            let Some(names) = &self.names else {
                if self.numeric.len() != 1 {
                    return Err(
                        "numeric EXPR required for 'switch' without named alternatives".into(),
                    );
                }
                self.warn_empty(pin)?;
                return self
                    .numeric
                    .first()
                    .copied()
                    .ok_or_else(|| "bad numeric 'switch' offsets".into());
            };
            let mut which = names.len().checked_sub(1).ok_or("bad 'switch' names")?;
            for (index, name) in names[..names.len() - 1].iter().enumerate() {
                if unsafe {
                    crate::mainutils::match_mod::pmatch(selector.as_raw(), name.as_raw(), 1)
                } != 0
                {
                    which = index;
                    break;
                }
            }
            pin.require_live().map_err(|error| error.to_string())?;
            return self
                .characters
                .get(which)
                .copied()
                .ok_or_else(|| "bad 'switch' names or character offsets".into());
        }
        if self.numeric.len() == 1 {
            self.warn_empty(pin)?;
        }
        let index = unsafe { crate::main::coerce::asInteger(value.as_raw()) };
        pin.require_live().map_err(|error| error.to_string())?;
        let which = if index == NA_INTEGER || index < 1 || index as usize > self.numeric.len() {
            self.numeric
                .len()
                .checked_sub(1)
                .ok_or("bad numeric 'switch' offsets")?
        } else {
            index as usize - 1
        };
        self.numeric
            .get(which)
            .copied()
            .ok_or_else(|| "bad numeric 'switch' offsets".into())
    }

    fn warn_empty(&self, pin: &crate::sexp::owner::OwnerPin) -> Result<(), String> {
        pin.require_live().map_err(|error| error.to_string())?;
        unsafe {
            crate::mainutils::errors::warningcall(
                self.call.as_raw(),
                c"'switch' with no alternatives".as_ptr(),
            );
        }
        pin.require_live().map_err(|error| error.to_string())
    }
}

fn offsets(value: &Sexp<'static>, code_len: usize) -> Result<Vec<usize>, String> {
    if value.typeof_() != SEXPTYPE::INTSXP || value.len() == 0 {
        return Err("bad 'switch' offsets".into());
    }
    let length = usize::try_from(value.len()).map_err(|_| "bad 'switch' offsets")?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(length)
        .map_err(|_| "cannot reserve 'switch' offsets")?;
    for index in 0..length {
        let target = value
            .try_integer_elt(index as i64)
            .map_err(|error| error.to_string())?;
        if target < 1 || target as usize >= code_len {
            return Err("GNU SWITCH target outside instruction stream".into());
        }
        result.push(target as usize);
    }
    Ok(result)
}

pub(super) fn prepare_owned(
    code: &[i32],
    pool: &[Sexp<'static>],
) -> Result<BTreeMap<usize, OwnedSwitch>, String> {
    super::bytecode::validate_gnu_bytecode_stream(code)?;
    let mut result = BTreeMap::new();
    let mut pc = 1;
    while pc < code.len() {
        let op = code[pc];
        if op == super::bytecode::GNU_OP_SWITCH {
            let constant = |operand: usize| -> Result<Sexp<'static>, String> {
                usize::try_from(code[pc + 1 + operand])
                    .ok()
                    .and_then(|index| pool.get(index))
                    .cloned()
                    .ok_or_else(|| "GNU SWITCH constant index out of range".into())
            };
            let call = constant(0)?;
            let names = constant(1)?;
            let chars = constant(2)?;
            let ints = constant(3)?;
            let numeric = offsets(&ints, code.len())?;
            let (names, characters) = if names.is_null_value() {
                (None, Vec::new())
            } else {
                if names.typeof_() != SEXPTYPE::STRSXP
                    || names.len() == 0
                    || names.len() != chars.len()
                {
                    return Err("bad 'switch' names or character offsets".into());
                }
                let length = usize::try_from(names.len()).map_err(|_| "bad 'switch' names")?;
                let mut entries = Vec::new();
                entries
                    .try_reserve_exact(length)
                    .map_err(|_| "cannot reserve 'switch' names")?;
                for index in 0..length {
                    entries.push(
                        names
                            .try_string_elt(index as i64)
                            .map_err(|error| error.to_string())?,
                    );
                }
                let characters = offsets(&chars, code.len())?;
                if characters.len() != entries.len() {
                    return Err("bad 'switch' names or character offsets".into());
                }
                (Some(entries), characters)
            };
            result.insert(
                pc,
                OwnedSwitch {
                    call,
                    names,
                    characters,
                    numeric,
                },
            );
        }
        pc = super::bytecode::gnu_next_pc(pc, op, code.len())
            .map_err(|_| "GNU SWITCH opcode is truncated")?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::bc_stack::own_operand;
    use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector, Rf_mkChar, Rf_mkString};
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::object::SexpMut;
    use crate::sexp::session::RSession;

    fn code() -> [i32; 10] {
        [
            super::super::bytecode::GNU_BC_MAX_VERSION,
            super::super::bytecode::GNU_OP_SWITCH,
            0,
            1,
            2,
            3,
            super::super::bytecode::GNU_OP_LDNULL,
            super::super::bytecode::GNU_OP_RETURN,
            super::super::bytecode::GNU_OP_LDNULL,
            super::super::bytecode::GNU_OP_RETURN,
        ]
    }

    unsafe fn integers(values: &[i32]) -> Sexp<'static> {
        let mut value = SexpMut::try_from_checked(unsafe {
            own_operand(Rf_allocVector(
                SEXPTYPE::INTSXP,
                i32::try_from(values.len()).unwrap(),
            ))
        })
        .unwrap();
        for (index, element) in values.iter().copied().enumerate() {
            assert!(value.set_integer_elt(index as i64, element));
        }
        value.freeze()
    }

    #[test]
    fn owned_switch_tables_keep_targets_and_names_after_detachment_mutation_and_gc() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let owner = session.owner_token().unwrap();
            let pin = owner.pin().unwrap().unwrap();
            let names = own_operand(Rf_allocVector(SEXPTYPE::STRSXP, 2));
            SET_STRING_ELT(names.as_raw(), 0, Rf_mkChar(c"alpha".as_ptr()));
            SET_STRING_ELT(names.as_raw(), 1, Rf_mkChar(c"".as_ptr()));
            let chars = integers(&[8, 6]);
            let ints = integers(&[6, 8]);
            let pool = own_operand(Rf_allocVector(SEXPTYPE::VECSXP, 4));
            SET_VECTOR_ELT(pool.as_raw(), 0, R_NilValue());
            SET_VECTOR_ELT(pool.as_raw(), 1, names.as_raw());
            SET_VECTOR_ELT(pool.as_raw(), 2, chars.as_raw());
            SET_VECTOR_ELT(pool.as_raw(), 3, ints.as_raw());
            let constants: Vec<_> = (0..4)
                .map(|index| pool.try_vector_elt(index).unwrap().into_owned().unwrap())
                .collect();
            let tables = prepare_owned(&code(), &constants).unwrap();
            let table = tables.get(&1).unwrap();
            assert_eq!(table.successors(), [6, 8]);

            for index in 0..4 {
                SET_VECTOR_ELT(pool.as_raw(), index, R_NilValue());
            }
            SET_STRING_ELT(names.as_raw(), 0, Rf_mkChar(c"beta".as_ptr()));
            let mut chars = SexpMut::try_from_checked(chars).unwrap();
            let mut ints = SexpMut::try_from_checked(ints).unwrap();
            assert!(chars.set_integer_elt(0, 6));
            assert!(ints.set_integer_elt(1, 6));
            drop(chars);
            drop(ints);
            drop(names);
            drop(constants);
            owner.full_gc().unwrap();

            let alpha = own_operand(Rf_mkString(c"alpha".as_ptr()));
            let other = own_operand(Rf_mkString(c"beta".as_ptr()));
            let second = own_operand(Rf_ScalarInteger(2));
            assert_eq!(table.select(&alpha, &pin).unwrap(), 8);
            assert_eq!(table.select(&other, &pin).unwrap(), 6);
            assert_eq!(table.select(&second, &pin).unwrap(), 8);
        });
    }

    #[test]
    fn owned_switch_tables_reject_bad_shapes_targets_and_indices() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let names = own_operand(Rf_mkString(c"alpha".as_ptr()));
            let nil = own_operand(R_NilValue());
            let invalid = integers(&[10]);
            let valid = integers(&[6, 8]);
            assert!(
                prepare_owned(
                    &code(),
                    &[nil.clone(), names.clone(), valid.clone(), invalid]
                )
                .err()
                .expect("invalid switch table")
                .contains("target outside")
            );
            assert!(
                prepare_owned(&code(), &[nil.clone(), names, valid.clone(), valid.clone()])
                    .err()
                    .expect("invalid switch table")
                    .contains("names or character offsets")
            );
            let empty = integers(&[]);
            assert!(
                prepare_owned(&code(), &[nil.clone(), nil.clone(), empty.clone(), empty])
                    .err()
                    .expect("invalid switch table")
                    .contains("offsets")
            );
            let mut malformed = code();
            malformed[5] = -1;
            assert!(
                prepare_owned(
                    &malformed,
                    &[nil.clone(), nil.clone(), valid.clone(), valid]
                )
                .err()
                .expect("invalid switch table")
                .contains("constant index")
            );
            assert!(
                prepare_owned(
                    &[
                        super::super::bytecode::GNU_BC_MAX_VERSION,
                        super::super::bytecode::GNU_OP_SWITCH,
                        0
                    ],
                    &[]
                )
                .is_err()
            );
        });
    }
    #[test]
    fn owned_switch_selector_rejects_revoked_original_runtime_with_replacement_active() {
        let mut session = RSession::new_for_gc_tests();
        let (table, value, pin) = session.with_active(|| unsafe {
            let pin = session.owner_token().unwrap().pin().unwrap().unwrap();
            let value = own_operand(Rf_ScalarInteger(1));
            let table = OwnedSwitch {
                call: own_operand(R_NilValue()),
                names: None,
                characters: Vec::new(),
                numeric: vec![6],
            };
            (table, value, pin)
        });
        session.close();
        let replacement = RSession::new_for_gc_tests();
        replacement.with_active(|| {
            assert!(table.select(&value, &pin).is_err());
        });
    }
    #[test]
    #[cfg_attr(
        miri,
        ignore = "GNU serialized fixtures exercise the complete evaluator; strict Miri covers the focused table and ownership cases"
    )]
    fn owned_gnu_switch_serialized_fixtures_and_mutated_stream_round_trip() {
        fn raw_expression(bytes: &[u8]) -> String {
            let values = bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("as.raw(c({values}))")
        }
        fn assert_true(session: &mut RSession, script: &str, label: &str) {
            let (result, _, _) = session.eval_code_with_output_capture(script);
            let value = result.unwrap_or_else(|error| panic!("{label}: {error}"));
            assert_eq!(value.typeof_(), SEXPTYPE::LGLSXP, "{label}");
            assert_eq!(value.logical_elt(0), Some(1), "{label}");
        }
        let named =
            include_bytes!("../../../r-embed/tests/fixtures/gnu-bytecode-switch/switch.rds");
        let fixtures: &[(&str, &[u8], &str)] = &[
            (
                "named/default",
                named,
                "identical(c(f('a'),f('b'),f('z')),c(1L,2L,0L))",
            ),
            (
                "numeric",
                include_bytes!("../../../r-embed/tests/fixtures/gnu-bytecode-switch/numeric.rds"),
                "identical(c(f(1L),f(2L)),c(10L,20L))&&is.null(f(9L))",
            ),
            (
                "missing/default",
                include_bytes!("../../../r-embed/tests/fixtures/gnu-bytecode-switch/missing.rds"),
                "identical(f('z'),0L)&&isTRUE(tryCatch(f(character(0)),error=function(e)TRUE))",
            ),
            (
                "fallthrough",
                include_bytes!(
                    "../../../r-embed/tests/fixtures/gnu-bytecode-switch/fallthrough.rds"
                ),
                "identical(c(f('a'),f('b'),f('z')),c(2L,2L,0L))",
            ),
            (
                "visibility",
                include_bytes!(
                    "../../../r-embed/tests/fixtures/gnu-bytecode-switch/visibility.rds"
                ),
                "identical(c(withVisible(f('a'))$visible,withVisible(f('z'))$visible),c(TRUE,TRUE))",
            ),
        ];
        let mut session = RSession::new_without_default_packages();
        for (label, fixture, assertion) in fixtures {
            assert_true(
                &mut session,
                &format!("f<-unserialize({});{assertion}", raw_expression(fixture)),
                label,
            );
        }

        // The pinned GNU fixture retains source returning 1L for "a". Editing
        // only LDCONST's operand must return 2L before and after serialization.
        let stream: [i32; 20] = [
            12, 20, 1, 102, 0, 2, 6, 7, 17, 15, 1, 16, 3, 1, 16, 4, 1, 16, 5, 1,
        ];
        let encoded: Vec<_> = stream.iter().flat_map(|word| word.to_be_bytes()).collect();
        let at = named
            .windows(encoded.len())
            .position(|bytes| bytes == encoded)
            .expect("pinned GNU SWITCH stream");
        let mut changed = named.to_vec();
        changed[at + 12 * 4..at + 13 * 4].copy_from_slice(&4_i32.to_be_bytes());
        assert_true(
            &mut session,
            &format!(
                "f<-unserialize({});g<-unserialize(serialize(f,NULL));identical(f('a'),2L)&&identical(g('a'),2L)",
                raw_expression(&changed)
            ),
            "edited bytecode round trip",
        );

        // Reject malformed table operands at import, then evaluate successfully
        // in the same original session to prove error cleanup and recovery.
        for (operand, replacement) in [(5, 3_i32), (6, 3), (7, 3), (7, 999)] {
            let mut invalid = named.to_vec();
            invalid[at + operand * 4..at + (operand + 1) * 4]
                .copy_from_slice(&replacement.to_be_bytes());
            {
                let (result, _, _) = session.eval_code_with_output_capture(&format!(
                    "unserialize({})",
                    raw_expression(&invalid)
                ));
                assert!(result.is_err(), "SWITCH operand {operand}={replacement}");
            }
            assert_true(
                &mut session,
                "identical(1L+1L,2L)",
                "malformed table recovery",
            );
        }
        let targets: Vec<_> = [13_i32, 3, 11, 14, 17]
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect();
        let at = named
            .windows(targets.len())
            .position(|bytes| bytes == targets)
            .expect("pinned GNU SWITCH offset vector");
        let mut invalid = named.to_vec();
        invalid[at + 8..at + 12].copy_from_slice(&12_i32.to_be_bytes());
        {
            let (result, _, _) = session.eval_code_with_output_capture(&format!(
                "unserialize({})",
                raw_expression(&invalid)
            ));
            assert!(
                result.is_err(),
                "SWITCH target enters an instruction operand"
            );
        }
        assert_true(
            &mut session,
            "identical(1L+1L,2L)",
            "invalid target recovery",
        );
    }
}
