//! strsplit and chartr

use super::*;

/// R's `strsplit(x, split, fixed=FALSE, perl=FALSE, useBytes=FALSE)` — split
/// strings by matches of `split`, returning a list of token vectors.
///
/// Mirrors upstream `do_strsplit` (grep.c): every match splits — a
/// non-empty match is dropped and the text before it becomes a token; an
/// empty match consumes the current character, which itself becomes the
/// token. A trailing remainder is kept only when non-empty. The `split`
/// vector recycles across `x`; an empty pattern splits into characters; an
/// NA split token does not split; NA strings pass through as NA.
pub unsafe fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = arg_by_name_or_position(args, &["x"], 0);
        let split_arg = arg_by_name_or_position(args, &["split"], 1);

        if x_arg.is_null() || x_arg == R_NilValue() || split_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let mut perl = named_logical_arg(args, "perl").unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        let n = XLENGTH(x_arg);
        let tlen = if split_arg == R_NilValue() {
            0
        } else {
            XLENGTH(split_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            // NA input string passes through as NA.
            if TYPEOF(x_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(x_arg, i as i64) == crate::sexp::globals::R_NaString()
            {
                let na_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                if !na_vec.is_null() {
                    let _na_guard = protect(na_vec);
                    SET_STRING_ELT(na_vec, 0, crate::sexp::globals::R_NaString());
                    SET_VECTOR_ELT(result, i as i64, na_vec);
                }
                continue;
            }
            let s = elt_to_string(x_arg, i);
            // NA split token doesn't split: whole string as the only token.
            let na_split = tlen > 0
                && TYPEOF(split_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(split_arg, (i % tlen) as i64) == crate::sexp::globals::R_NaString();
            let split = if tlen == 0 {
                String::new()
            } else {
                elt_to_string(split_arg, (i % tlen) as i64)
            };

            let tokens: Vec<String> = if na_split {
                vec![s]
            } else if split.is_empty() {
                s.chars().map(|c| c.to_string()).collect()
            } else {
                // Find all matches, then cut tokens around them.
                let mut matches: Vec<(usize, usize)> = Vec::new();
                let mut offset = 0usize;
                if !s.is_empty() {
                    loop {
                        let hay = &s[offset..];
                        let found = if fixed {
                            fixed_find(hay, &split, false)
                        } else if perl {
                            crate::mainutils::grep::perl_find(&split, hay, false)
                        } else {
                            crate::mainutils::grep::ere_find(&split, hay, false)
                        };
                        let Some(m) = found else { break };
                        matches.push((offset + m.start, offset + m.end));
                        if m.end > m.start {
                            offset += m.end;
                        } else {
                            // Empty match gets the next char, so move by one.
                            offset += hay.chars().next().map_or(1, char::len_utf8);
                        }
                        if offset >= s.len() {
                            break;
                        }
                    }
                }
                let mut tokens = Vec::with_capacity(matches.len() + 1);
                let mut pos = 0usize;
                for (st, en) in &matches {
                    if en > st {
                        tokens.push(s[pos..*st].to_string());
                        pos = *en;
                    } else {
                        // Empty match: the current character is the token.
                        let clen = s[*st..].chars().next().map_or(1, char::len_utf8);
                        tokens.push(s[*st..(*st + clen)].to_string());
                        pos = *st + clen;
                    }
                }
                if pos < s.len() {
                    tokens.push(s[pos..].to_string());
                }
                tokens
            };

            let vec = Rf_allocVector3(SEXPTYPE::STRSXP, tokens.len() as R_xlen_t);
            if !vec.is_null() {
                let _vec_guard = protect(vec);
                for (j, part) in tokens.iter().enumerate() {
                    let cstr = CString::new(part.as_str()).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        SET_STRING_ELT(vec, j as i64, charsxp);
                    }
                }
            }
            SET_VECTOR_ELT(result, i as i64, vec);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Conversion: chartr, format
// ---------------------------------------------------------------------------

/// R's `chartr(old, new, x)` — character-by-character translation.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old_arg = CAR(args);
        let new_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        // SAFETY: this primitive entry point is dispatched with one active,
        // live original R owner and retains no arena or payload loan here.
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|_| chartr_original_owner_error());
        if old_arg.is_null() || new_arg.is_null() {
            return R_NilValue();
        }
        let old_str = elt_to_string(old_arg, 0);
        chartr_require_original_owner(owner);
        let new_str = elt_to_string(new_arg, 0);
        chartr_require_original_owner(owner);
        // Character/range scratch storage is admitted to the original owner's
        // ledger before each buffer allocation, including replacement peaks.
        let workspace = crate::mainutils::grep::regex_workspace();
        let old_chars = match expand_chartr_spec(&workspace, &old_str) {
            Ok(chars) => chars,
            Err(message) => crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &message,
            ),
        };
        let new_chars = match expand_chartr_spec(&workspace, &new_str) {
            Ok(chars) => chars,
            Err(message) => crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &message,
            ),
        };
        if old_chars.len() > new_chars.len() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'old' is longer than 'new'",
            );
        }
        let n = if x_arg.is_null() || x_arg == R_NilValue() {
            0
        } else {
            XLENGTH(x_arg)
        };
        chartr_require_original_owner(owner);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        chartr_require_original_owner(owner);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if as_character_element_is_na(x_arg, i) {
                chartr_require_original_owner(owner);
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            chartr_require_original_owner(owner);
            let s = elt_to_string(x_arg, i);
            chartr_require_original_owner(owner);
            let translated_len = match chartr_translated_len(&s, &old_chars, &new_chars) {
                Ok(length) => length,
                Err(message) => crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    &message,
                ),
            };
            let Ok(length) = std::ffi::c_int::try_from(translated_len) else {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "character translation result is too long",
                );
            };
            let mut translated = match workspace.try_buffer::<u8>(translated_len) {
                Ok(buffer) => buffer,
                Err(()) => crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "character translation workspace exceeds session memory limit",
                ),
            };
            for c in s.chars() {
                let mapped = old_chars
                    .iter()
                    .rposition(|&oc| oc == u32::from(c))
                    .and_then(|pos| char::from_u32(new_chars[pos]))
                    .unwrap_or(c);
                let mut encoded = [0; 4];
                for byte in mapped.encode_utf8(&mut encoded).bytes() {
                    if translated.try_push(byte).is_err() {
                        crate::mainutils::errors::errorcall_str(
                            crate::mainutils::errors::R_getCurrentCall(),
                            "character translation workspace exceeds session memory limit",
                        );
                    }
                }
            }
            let charsxp =
                crate::sexp::constructors::Rf_mkCharLen(translated.as_ptr().cast(), length);
            chartr_require_original_owner(owner);
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        result
    }
}

fn expand_chartr_spec(
    workspace: &crate::mainutils::grep::RegexWorkspace,
    spec: &str,
) -> Result<impl std::ops::Deref<Target = [u32]>, String> {
    let mut source = workspace
        .try_buffer::<u32>(spec.chars().count())
        .map_err(|()| chartr_workspace_error())?;
    for character in spec.chars() {
        source
            .try_push(u32::from(character))
            .map_err(|()| chartr_workspace_error())?;
    }
    let expanded_len = chartr_expanded_len(&source)?;
    let mut expanded = workspace
        .try_buffer::<u32>(expanded_len)
        .map_err(|()| chartr_workspace_error())?;
    let mut index = 0;
    while index + 2 < source.len() {
        if source[index + 1] == u32::from('-') {
            let first = source[index];
            let last = source[index + 2];
            if first > last {
                let first = char::from_u32(first).unwrap_or('\u{fffd}');
                let last = char::from_u32(last).unwrap_or('\u{fffd}');
                return Err(format!("decreasing range specification ('{first}-{last}')"));
            }
            for codepoint in first..=last {
                expanded
                    .try_push(codepoint)
                    .map_err(|()| chartr_workspace_error())?;
            }
            index += 3;
        } else {
            expanded
                .try_push(source[index])
                .map_err(|()| chartr_workspace_error())?;
            index += 1;
        }
    }
    for &codepoint in &source[index..] {
        expanded
            .try_push(codepoint)
            .map_err(|()| chartr_workspace_error())?;
    }
    Ok(expanded)
}

fn chartr_expanded_len(source: &[u32]) -> Result<usize, String> {
    let mut expanded_len = 0usize;
    let mut index = 0;
    while index + 2 < source.len() {
        if source[index + 1] == u32::from('-') {
            let first = source[index];
            let last = source[index + 2];
            if first > last {
                let first = char::from_u32(first).unwrap_or('\u{fffd}');
                let last = char::from_u32(last).unwrap_or('\u{fffd}');
                return Err(format!("decreasing range specification ('{first}-{last}')"));
            }
            let count = usize::try_from(last - first + 1).map_err(|_| chartr_workspace_error())?;
            expanded_len = expanded_len
                .checked_add(count)
                .ok_or_else(chartr_workspace_error)?;
            index += 3;
        } else {
            expanded_len = expanded_len
                .checked_add(1)
                .ok_or_else(chartr_workspace_error)?;
            index += 1;
        }
    }
    expanded_len = expanded_len
        .checked_add(source.len() - index)
        .ok_or_else(chartr_workspace_error)?;
    Ok(expanded_len)
}

fn chartr_translated_len(
    input: &str,
    old_chars: &[u32],
    new_chars: &[u32],
) -> Result<usize, String> {
    input.chars().try_fold(0usize, |length, character| {
        let mapped = old_chars
            .iter()
            .rposition(|&old| old == u32::from(character))
            .and_then(|position| char::from_u32(new_chars[position]))
            .unwrap_or(character);
        length
            .checked_add(mapped.len_utf8())
            .ok_or_else(chartr_workspace_error)
    })
}

fn chartr_workspace_error() -> String {
    "character translation workspace exceeds session memory limit".into()
}

fn chartr_original_owner_error() -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: "character translation lost its original session".into(),
    })
}

fn chartr_require_original_owner(owner: crate::sexp::owner::OwnerToken<'_>) {
    if owner.require_active().is_err() {
        chartr_original_owner_error();
    }
}

#[cfg(test)]
mod chartr_workspace_tests {
    use super::*;
    use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
    use crate::sexp::{memory::ArenaBudget, session::RSession};

    struct CollectingUnicode;

    impl AltrepClass for CollectingUnicode {
        fn vector_type(&self) -> crate::sexp::ffi::SEXPTYPE {
            crate::sexp::ffi::SEXPTYPE::STRSXP
        }

        fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::object::SexpResult<i64> {
            Ok(1)
        }

        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            _: i64,
        ) -> crate::sexp::object::SexpResult<AltrepElement<'s>> {
            context.gc()?;
            Ok(AltrepElement::String(context.string("α")?))
        }
    }

    struct SwitchingUnicode {
        second: std::rc::Rc<std::cell::RefCell<Option<RSession>>>,
        observed_original_workspace: std::rc::Rc<std::cell::Cell<usize>>,
    }

    struct RevokingUnicode {
        facade: std::rc::Weak<std::cell::RefCell<Option<RSession>>>,
        revoke_at_length: bool,
        length_calls: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl RevokingUnicode {
        fn revoke_original(&self) {
            let facade = self
                .facade
                .upgrade()
                .expect("original session facade is live");
            drop(facade.borrow_mut().take());
        }
    }

    impl AltrepClass for SwitchingUnicode {
        fn vector_type(&self) -> crate::sexp::ffi::SEXPTYPE {
            crate::sexp::ffi::SEXPTYPE::STRSXP
        }

        fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::object::SexpResult<i64> {
            Ok(1)
        }

        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            _: i64,
        ) -> crate::sexp::object::SexpResult<AltrepElement<'s>> {
            let second = self
                .second
                .borrow_mut()
                .take()
                .expect("second session is available to the provider");
            second.gc();
            *self.second.borrow_mut() = Some(second);

            // SAFETY: this ALTREP element callback runs under the original
            // chartr owner, after the nested second-session GC has returned.
            let current = unsafe { crate::sexp::owner::OwnerToken::current()? };
            let workspace_bytes = current.with_arena(|arena| arena.workspace_bytes())?;
            self.observed_original_workspace.set(workspace_bytes);
            Ok(AltrepElement::String(context.string("α")?))
        }
    }

    impl AltrepClass for RevokingUnicode {
        fn vector_type(&self) -> crate::sexp::ffi::SEXPTYPE {
            crate::sexp::ffi::SEXPTYPE::STRSXP
        }

        fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::object::SexpResult<i64> {
            self.length_calls.set(self.length_calls.get() + 1);
            if self.revoke_at_length && self.length_calls.get() > 1 {
                self.revoke_original();
            }
            Ok(1)
        }

        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            _: i64,
        ) -> crate::sexp::object::SexpResult<AltrepElement<'s>> {
            let character = context.string("α")?;
            if !self.revoke_at_length {
                self.revoke_original();
            }
            Ok(AltrepElement::String(character))
        }
    }

    #[test]
    fn range_workspace_denial_releases_admission_and_recovers() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let used = owner
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(used + 1024, 0)))
            .unwrap();

        // SAFETY: the session and its owner remain live for the active scope.
        unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                let workspace = crate::mainutils::grep::regex_workspace();
                assert!(expand_chartr_spec(&workspace, "Ā-\u{10ffff}").is_err());
            })
        };
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );

        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        // SAFETY: the session and its owner remain live for the active scope.
        unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                let workspace = crate::mainutils::grep::regex_workspace();
                let translated = expand_chartr_spec(&workspace, "α-γ").unwrap();
                assert_eq!(&*translated, &[0x03b1, 0x03b2, 0x03b3]);
                let moderate = expand_chartr_spec(&workspace, "Ā-࿿").unwrap();
                assert_eq!(moderate.len(), 0x0fff - 0x0100 + 1);
            })
        };
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn range_workspace_remains_charged_to_its_owner_during_owner_switch() {
        let first = RSession::new_for_gc_tests();
        let first_owner = first.owner_token().unwrap();
        let second = RSession::new_for_gc_tests();
        let second_owner = second.owner_token().unwrap();

        // SAFETY: both sessions and their owners remain live for the scopes.
        let workspace = unsafe {
            crate::sexp::session::with_instance_active(first_owner.as_ptr(), || {
                crate::mainutils::grep::regex_workspace()
            })
        };
        // SAFETY: `workspace` owns the first session's independent ledger and
        // both sessions remain live while a second owner is active.
        let expanded = unsafe {
            crate::sexp::session::with_instance_active(second_owner.as_ptr(), || {
                expand_chartr_spec(&workspace, "α-γ").unwrap()
            })
        };
        assert_eq!(&*expanded, &[0x03b1, 0x03b2, 0x03b3]);
        // SAFETY: both sessions and owners remain live for the scopes.
        let first_bytes = unsafe {
            crate::sexp::session::with_instance_active(first_owner.as_ptr(), || {
                first_owner
                    .with_arena(|arena| arena.workspace_bytes())
                    .unwrap()
            })
        };
        assert!(first_bytes > 0);
        // SAFETY: both sessions and owners remain live for the scopes.
        let second_bytes = unsafe {
            crate::sexp::session::with_instance_active(second_owner.as_ptr(), || {
                second_owner
                    .with_arena(|arena| arena.workspace_bytes())
                    .unwrap()
            })
        };
        assert_eq!(second_bytes, 0);
        drop(expanded);
        drop(workspace);
        // SAFETY: the first session and owner remain live for the scope.
        let released = unsafe {
            crate::sexp::session::with_instance_active(first_owner.as_ptr(), || {
                first_owner
                    .with_arena(|arena| arena.workspace_bytes())
                    .unwrap()
            })
        };
        assert_eq!(released, 0);
    }

    #[test]
    fn range_workspace_survives_a_collecting_altrep_string_callback() {
        let mut session = RSession::new_for_gc_tests();
        let class = session
            .register_altrep_class("chartr.collecting.unicode", CollectingUnicode)
            .unwrap();
        let input = AltrepBuilder::new(class).build().unwrap();
        assert!(session.define_var("chartr_collecting_input", input));

        let (result, _, _) = session.eval_code_with_output_capture(
            "identical(chartr('α', 'A', chartr_collecting_input), 'A')",
        );
        let result = result.expect("chartr must survive collection from ALTREP STRING_ELT");
        assert_eq!(result.logical_elt(0), Some(1));
    }

    #[test]
    fn chartr_keeps_workspace_on_original_owner_across_provider_session_switch() {
        use std::{cell::Cell, rc::Rc};

        let session = RSession::new_for_gc_tests();
        let second = Rc::new(std::cell::RefCell::new(Some(RSession::new_for_gc_tests())));
        let observed_original_workspace = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class(
                "chartr.switching.unicode",
                SwitchingUnicode {
                    second: second.clone(),
                    observed_original_workspace: observed_original_workspace.clone(),
                },
            )
            .unwrap();
        let input = AltrepBuilder::new(class).build().unwrap();
        let output = session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil();
            let old = factory.strings(&["α"]).unwrap();
            let new = factory.strings(&["A"]).unwrap();
            let x_tail = factory.pairlist_cell(&input, &nil, &nil).unwrap();
            let new_tail = factory.pairlist_cell(&new, &x_tail, &nil).unwrap();
            let args = factory.pairlist_cell(&old, &new_tail, &nil).unwrap();
            let raw = unsafe {
                do_chartr(
                    crate::sexp::globals::R_NilValue(),
                    crate::sexp::globals::R_NilValue(),
                    args.as_raw(),
                    crate::sexp::globals::R_NilValue(),
                )
            };
            session.sexp(raw).unwrap()
        });
        assert_eq!(
            output.try_string_value_elt(0).unwrap().as_deref(),
            Some("A")
        );
        assert!(
            observed_original_workspace.get() >= 8,
            "provider must observe both old and new admitted workspaces"
        );
        assert!(second.borrow().is_some());
    }

    #[test]
    fn chartr_rejects_publication_after_provider_revokes_original_owner() {
        use std::{
            cell::RefCell,
            panic::{AssertUnwindSafe, catch_unwind},
            rc::Rc,
        };

        fn run(revoke_at_length: bool, class_name: &str) {
            let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
            let original = facade
                .borrow()
                .as_ref()
                .unwrap()
                .owner_token()
                .unwrap()
                .weak_owner()
                .unwrap();
            let length_calls = Rc::new(std::cell::Cell::new(0));
            let pin = original.pin().unwrap();
            let instance = pin.as_ptr();
            let (input, args) = {
                let session = facade.borrow();
                let session = session.as_ref().unwrap();
                let class = session
                    .register_altrep_class(
                        class_name,
                        RevokingUnicode {
                            facade: Rc::downgrade(&facade),
                            revoke_at_length,
                            length_calls: length_calls.clone(),
                        },
                    )
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let input = AltrepBuilder::new(class).build().unwrap();
                let factory = original.node_factory().unwrap();
                let nil = factory.nil();
                let old = factory.strings(&["α"]).unwrap();
                let new = factory.strings(&["A"]).unwrap();
                let x_tail = factory.pairlist_cell(&input, &nil, &nil).unwrap();
                let new_tail = factory.pairlist_cell(&new, &x_tail, &nil).unwrap();
                let args = factory.pairlist_cell(&old, &new_tail, &nil).unwrap();
                (input, args)
            };

            let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
                crate::sexp::session::with_instance_active(instance, || {
                    do_chartr(
                        crate::sexp::globals::R_NilValue(),
                        crate::sexp::globals::R_NilValue(),
                        args.as_raw(),
                        crate::sexp::globals::R_NilValue(),
                    )
                })
            }));
            assert!(
                outcome.is_err(),
                "revoked owner must not publish chartr output"
            );
            if revoke_at_length {
                assert!(
                    length_calls.get() >= 2,
                    "length provider must run during chartr after ALTREP construction"
                );
            }
            assert!(!original.is_live());
            assert_eq!(
                unsafe { (*pin.as_ptr()).arena.workspace_bytes() },
                0,
                "chartr workspace must be released from the revoked original ledger"
            );
            drop(input);
            drop(args);
            drop(pin);
            assert!(facade.borrow().is_none());
        }

        run(false, "chartr.revoking.element.unicode");
    }

    #[test]
    fn chartr_uses_cached_altrep_length_without_provider_reentry() {
        use std::{cell::{Cell, RefCell}, rc::Rc};

        let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let length_calls = Rc::new(Cell::new(0));
        {
            let session = facade.borrow();
            let session = session.as_ref().unwrap();
            let owner = session.owner_token().unwrap();
            let original = owner.weak_owner().unwrap();
            let class = session
                .register_altrep_class(
                    "chartr.cached.length.unicode",
                    RevokingUnicode {
                        facade: Rc::downgrade(&facade),
                        revoke_at_length: true,
                        length_calls: length_calls.clone(),
                    },
                )
                .unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            assert_eq!(length_calls.get(), 1, "builder queries the provider once");
            session.with_active(|| {
                let factory = original.node_factory().unwrap();
                let nil = factory.nil();
                let old = factory.strings(&["α"]).unwrap();
                let new = factory.strings(&["A"]).unwrap();
                let x_tail = factory.pairlist_cell(&input, &nil, &nil).unwrap();
                let new_tail = factory.pairlist_cell(&new, &x_tail, &nil).unwrap();
                let args = factory.pairlist_cell(&old, &new_tail, &nil).unwrap();
                let raw = unsafe {
                    do_chartr(
                        crate::sexp::globals::R_NilValue(),
                        crate::sexp::globals::R_NilValue(),
                        args.as_raw(),
                        crate::sexp::globals::R_NilValue(),
                    )
                };
                let output = session.sexp(raw).unwrap();
                assert_eq!(
                    output.try_string_value_elt(0).unwrap().as_deref(),
                    Some("A")
                );
                assert_eq!(
                    length_calls.get(),
                    1,
                    "chartr's XLENGTH reads the cached vector length"
                );
                drop(output);
                assert_eq!(
                    owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                    0
                );
            })
        }
    }
}
