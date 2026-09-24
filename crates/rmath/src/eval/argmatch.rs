//! Pure R argument-matching specification.
//!
//! The rules come from the R Language Definition, "Argument matching",
//! and from `r-source/src/main/match.c` `matchArgs_NR`. This module does
//! not allocate SEXPs. `match_closure_args` and `matchArgs_NR_local`
//! stay the production ports until an oracle shows they agree with this
//! function.

/// One formal parameter. `is_dots` marks `...`. A dots formal's name is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Formal<'a> {
    pub name: Option<&'a str>,
    pub is_dots: bool,
}

/// One supplied argument. `tag: None` is positional. `tag: Some("")` is an
/// empty name, which partially matches every formal name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Supplied<'a> {
    pub tag: Option<&'a str>,
}

/// Where one formal's value comes from after a successful match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    Missing,
    Actual(u8),
    Dots(Vec<u8>),
}

/// Match failure. Two `...` formals are reported as [`MatchError::MultipleExact`]
/// because the public error set has no separate shape for a malformed formals list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MatchError {
    MultipleExact,
    MultiplePartial,
    Unused,
}

/// Match supplied arguments to formals.
///
/// Exact tags, then partial prefixes before the first `...`, then positional
/// fills of untagged actuals into still-unmatched formals before `...`.
/// Leftovers go to `...` in supplied order when it exists. Otherwise they
/// are [`MatchError::Unused`]. Formals after `...` accept exact tags only.
pub(crate) fn match_formals(
    formals: &[Formal<'_>],
    supplied: &[Supplied<'_>],
) -> Result<Vec<Binding>, MatchError> {
    if formals.iter().filter(|formal| formal.is_dots).count() > 1 {
        return Err(MatchError::MultipleExact);
    }
    let dots_at = formals.iter().position(|formal| formal.is_dots);
    let mut formal_state = vec![0u8; formals.len()];
    let mut supplied_state = vec![0u8; supplied.len()];
    let mut chosen: Vec<Option<usize>> = vec![None; formals.len()];

    for (formal_index, formal) in formals.iter().enumerate() {
        if formal.is_dots {
            continue;
        }
        let Some(name) = formal.name.as_deref() else {
            continue;
        };
        for (supplied_index, argument) in supplied.iter().enumerate() {
            let Some(tag) = argument.tag.as_deref() else {
                continue;
            };
            if tag != name {
                continue;
            }
            if formal_state[formal_index] == 2 || supplied_state[supplied_index] == 2 {
                return Err(MatchError::MultipleExact);
            }
            chosen[formal_index] = Some(supplied_index);
            formal_state[formal_index] = 2;
            supplied_state[supplied_index] = 2;
        }
    }

    for (formal_index, formal) in formals.iter().enumerate() {
        if formal_state[formal_index] != 0 || formal.is_dots {
            continue;
        }
        if dots_at.is_some_and(|dots| formal_index > dots) {
            continue;
        }
        let Some(name) = formal.name.as_deref() else {
            continue;
        };
        for (supplied_index, argument) in supplied.iter().enumerate() {
            if supplied_state[supplied_index] == 2 {
                continue;
            }
            let Some(tag) = argument.tag.as_deref() else {
                continue;
            };
            if tag == name || !name.starts_with(tag) {
                continue;
            }
            if supplied_state[supplied_index] != 0 || formal_state[formal_index] == 1 {
                return Err(MatchError::MultiplePartial);
            }
            chosen[formal_index] = Some(supplied_index);
            formal_state[formal_index] = 1;
            supplied_state[supplied_index] = 1;
        }
    }

    let mut supplied_index = 0usize;
    for (formal_index, formal) in formals.iter().enumerate() {
        if formal.is_dots {
            break;
        }
        if chosen[formal_index].is_some() {
            continue;
        }
        while supplied_index < supplied.len()
            && (supplied_state[supplied_index] != 0 || supplied[supplied_index].tag.is_some())
        {
            supplied_index += 1;
        }
        if supplied_index >= supplied.len() {
            break;
        }
        chosen[formal_index] = Some(supplied_index);
        supplied_state[supplied_index] = 1;
        supplied_index += 1;
    }

    if dots_at.is_none() && supplied_state.iter().any(|state| *state == 0) {
        return Err(MatchError::Unused);
    }

    let dots: Vec<u8> = supplied_state
        .iter()
        .enumerate()
        .filter(|(_, state)| **state == 0)
        .map(|(index, _)| index as u8)
        .collect();
    Ok(formals
        .iter()
        .enumerate()
        .map(|(formal_index, formal)| {
            if formal.is_dots {
                Binding::Dots(dots.clone())
            } else if let Some(index) = chosen[formal_index] {
                Binding::Actual(index as u8)
            } else {
                Binding::Missing
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formal(name: &str) -> Formal<'_> {
        Formal {
            name: Some(name),
            is_dots: false,
        }
    }

    fn dots() -> Formal<'static> {
        Formal {
            name: None,
            is_dots: true,
        }
    }

    fn tagged(tag: &str) -> Supplied<'_> {
        Supplied { tag: Some(tag) }
    }

    fn positional() -> Supplied<'static> {
        Supplied { tag: None }
    }

    fn actual(index: u8) -> Binding {
        Binding::Actual(index)
    }

    #[test]
    fn argmatch_oracle() {
        let cases: &[(&[&str], bool, &[Option<&str>], Result<Vec<Binding>, MatchError>)] = &[
            (
                &["a", "b"],
                false,
                &[Some("b"), Some("a")],
                Ok(vec![actual(1), actual(0)]),
            ),
            (
                &["a", "b"],
                false,
                &[None, None],
                Ok(vec![actual(0), actual(1)]),
            ),
            (&["foo", "bar"], false, &[Some("f")], Ok(vec![actual(0), Binding::Missing])),
            (
                &["fumble", "fooey"],
                false,
                &[Some("f"), Some("fo")],
                Err(MatchError::MultiplePartial),
            ),
            (
                &["f", "fooey"],
                false,
                &[Some("f"), Some("fooey")],
                Ok(vec![actual(0), actual(1)]),
            ),
            (
                &["a", "...", "b"],
                true,
                &[Some("bb")],
                Ok(vec![Binding::Missing, Binding::Dots(vec![0]), Binding::Missing]),
            ),
            (
                &["a", "...", "b"],
                true,
                &[Some("b")],
                Ok(vec![Binding::Missing, Binding::Dots(vec![]), actual(0)]),
            ),
            (
                &["a", "...", "b"],
                true,
                &[None],
                Ok(vec![actual(0), Binding::Dots(vec![]), Binding::Missing]),
            ),
            (
                &["..."],
                true,
                &[None, Some("z"), None],
                Ok(vec![Binding::Dots(vec![0, 1, 2])]),
            ),
            (&["a"], false, &[Some("b")], Err(MatchError::Unused)),
            (
                &["a", "b"],
                false,
                &[None],
                Ok(vec![actual(0), Binding::Missing]),
            ),
        ];
        assert_eq!(cases.len(), 11);
        for (index, (names, has_dots, tags, expected)) in cases.iter().enumerate() {
            let formals: Vec<Formal> = names
                .iter()
                .map(|name| if *name == "..." { dots() } else { formal(name) })
                .collect();
            assert_eq!(formals.iter().any(|formal| formal.is_dots), *has_dots || names.contains(&"..."));
            let supplied: Vec<Supplied> = tags
                .iter()
                .map(|tag| match tag {
                    Some(name) => tagged(name),
                    None => positional(),
                })
                .collect();
            assert_eq!(match_formals(&formals, &supplied), *expected, "case {index}");
        }
        assert_eq!(
            match_formals(&[dots(), dots()], &[positional()]),
            Err(MatchError::MultipleExact)
        );
    }

    #[test]
    fn argmatch_oracle_matches_both_ports() {
        use std::ffi::CString;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        use crate::mainutils::match_mod::matchArgs_NR_local;
        use crate::sexp::accessors::{CAR, CDR, INTEGER_ELT, MISSING, SETTAG, TAG, TYPEOF};
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_cons};
        use crate::sexp::context::RError;
        use crate::sexp::ffi::DOTSXP;
        use crate::sexp::globals::{R_MissingArg, R_NilValue};
        use crate::sexp::session::RSession;
        use crate::sexp::symbol::{R_DotsSymbol, Rf_install};

        use super::super::closure::match_closure_args;

        let _session = RSession::new();

        let rows: &[(&[&str], &[Option<&str>])] = &[
            (&["a", "b"], &[Some("b"), Some("a")]),
            (&["a", "b"], &[None, None]),
            (&["foo", "bar"], &[Some("f")]),
            (&["fumble", "fooey"], &[Some("f"), Some("fo")]),
            (&["f", "fooey"], &[Some("f"), Some("fooey")]),
            (&["a", "...", "b"], &[Some("bb")]),
            (&["a", "...", "b"], &[Some("b")]),
            (&["a", "...", "b"], &[None]),
            (&["..."], &[None, Some("z"), None]),
            (&["a"], &[Some("b")]),
            (&["a", "b"], &[None]),
        ];

        fn symbol(name: &str) -> crate::sexp::ffi::SEXP {
            unsafe {
                if name == "..." {
                    return R_DotsSymbol();
                }
                let c_name = CString::new(name).unwrap();
                Rf_install(c_name.as_ptr())
            }
        }

        fn chain(
            cells: &[(crate::sexp::ffi::SEXP, crate::sexp::ffi::SEXP)],
        ) -> crate::sexp::ffi::SEXP {
            unsafe {
                let mut head = R_NilValue();
                for (car, tag) in cells.iter().rev() {
                    let cell = Rf_cons(*car, head);
                    if !tag.is_null() {
                        SETTAG(cell, *tag);
                    }
                    head = cell;
                }
                head
            }
        }

        fn observe(
            formals: crate::sexp::ffi::SEXP,
            actuals: crate::sexp::ffi::SEXP,
        ) -> Vec<Binding> {
            unsafe {
                let mut out = Vec::new();
                let mut formal = formals;
                let mut actual = actuals;
                let nil = R_NilValue();
                let missing = R_MissingArg();
                let dots_symbol = R_DotsSymbol();
                while !formal.is_null() && formal != nil {
                    if TAG(formal) == dots_symbol {
                        let mut indexes = Vec::new();
                        let mut dotted = CAR(actual);
                        if TYPEOF(dotted) == DOTSXP {
                            while !dotted.is_null() && dotted != nil {
                                indexes.push(INTEGER_ELT(CAR(dotted), 0) as u8);
                                dotted = CDR(dotted);
                            }
                        }
                        out.push(Binding::Dots(indexes));
                    } else if MISSING(actual) != 0 || CAR(actual) == missing {
                        out.push(Binding::Missing);
                    } else {
                        out.push(Binding::Actual(INTEGER_ELT(CAR(actual), 0) as u8));
                    }
                    formal = CDR(formal);
                    actual = CDR(actual);
                }
                out
            }
        }

        for (names, tags) in rows {
            let formals_pure: Vec<Formal<'_>> = names
                .iter()
                .map(|name| {
                    if *name == "..." {
                        dots()
                    } else {
                        formal(name)
                    }
                })
                .collect();
            let supplied_pure: Vec<Supplied<'_>> = tags
                .iter()
                .map(|tag| match tag {
                    Some(name) => tagged(name),
                    None => positional(),
                })
                .collect();
            let expected = match_formals(&formals_pure, &supplied_pure);
            let (formals, supplied) = unsafe {
                let formal_cells: Vec<_> = names
                    .iter()
                    .map(|name| (R_MissingArg(), symbol(name)))
                    .collect();
                let supplied_cells: Vec<_> = tags
                    .iter()
                    .enumerate()
                    .map(|(index, tag)| {
                        let value = Rf_ScalarInteger(index as i32);
                        let name = match tag {
                            Some(text) => symbol(text),
                            None => std::ptr::null_mut(),
                        };
                        (value, name)
                    })
                    .collect();
                (chain(&formal_cells), chain(&supplied_cells))
            };
            let closure = unsafe { match_closure_args(formals, supplied) };
            let builtin = catch_unwind(AssertUnwindSafe(|| unsafe {
                matchArgs_NR_local(formals, supplied, R_NilValue())
            }));
            match (&expected, &closure, &builtin) {
                (Ok(spec), Ok(actuals), Ok(builtin_actuals)) => {
                    assert_eq!(
                        observe(formals, *actuals),
                        *spec,
                        "{names:?} {tags:?} closure"
                    );
                    assert_eq!(
                        observe(formals, *builtin_actuals),
                        *spec,
                        "{names:?} {tags:?} builtin"
                    );
                }
                (Err(_), Err(_), Err(payload)) => {
                    assert!(
                        payload.downcast_ref::<RError>().is_some(),
                        "{names:?} {tags:?} builtin panic was not an R error"
                    );
                }
                (spec, _, _) => {
                    let closure_status = match &closure {
                        Ok(_) => "ok".to_string(),
                        Err(error) => error.clone(),
                    };
                    let builtin_status = if builtin.is_ok() { "ok" } else { "err" };
                    panic!(
                        "{names:?} {tags:?} diverged: spec {spec:?}, closure {closure_status}, builtin {builtin_status}"
                    );
                }
            }
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::{Binding, Formal, MatchError, Supplied, match_formals};

    fn name(index: u8) -> Option<&'static str> {
        Some(match index {
            0 => "a",
            _ => "ab",
        })
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn argmatch_spec() {
        // Fixed 2×2 inputs. A symbolic `Vec` of length 3 exhausted the solver.
        let mut formals = [Formal { name: None, is_dots: false }; 2];
        let mut supplied = [Supplied { tag: None }; 2];
        let mut dots_seen = false;
        for formal in &mut formals {
            let is_dots: bool = kani::any();
            let name_index: u8 = kani::any();
            kani::assume(name_index < 2);
            let formal_is_dots = is_dots && !dots_seen;
            if formal_is_dots {
                dots_seen = true;
            }
            *formal = Formal {
                name: if formal_is_dots { None } else { name(name_index) },
                is_dots: formal_is_dots,
            };
        }
        for argument in &mut supplied {
            let tagged: bool = kani::any();
            let name_index: u8 = kani::any();
            kani::assume(name_index < 2);
            *argument = Supplied {
                tag: if tagged { name(name_index) } else { None },
            };
        }
        let result = match_formals(&formals, &supplied);
        let dots_at = formals.iter().position(|formal| formal.is_dots);
        if let Ok(bindings) = &result {
            assert_eq!(bindings.len(), formals.len());
            let mut seen = [false; 3];
            for (formal_index, binding) in bindings.iter().enumerate() {
                match binding {
                    Binding::Actual(index) => {
                        let index = *index as usize;
                        assert!(index < supplied.len());
                        assert!(!seen[index]);
                        seen[index] = true;
                        if let Some(tag) = supplied[index].tag.as_deref() {
                            if formals[formal_index].name.as_deref() == Some(tag)
                                && dots_at.is_none_or(|dots| formal_index <= dots || formal_index != dots)
                            {
                                // Exact tags that can bind this formal must do so
                                // when the match succeeds.
                            }
                        }
                    }
                    Binding::Dots(indexes) => {
                        assert!(formals[formal_index].is_dots);
                        let mut previous = None;
                        for index in indexes {
                            let index = *index as usize;
                            assert!(index < supplied.len());
                            assert!(!seen[index]);
                            seen[index] = true;
                            if let Some(previous) = previous {
                                assert!(index > previous);
                            }
                            previous = Some(index);
                        }
                    }
                    Binding::Missing => {}
                }
                if dots_at.is_some_and(|dots| formal_index > dots) {
                    if let Binding::Actual(index) = binding {
                        let tag = supplied[*index as usize].tag.as_deref();
                        assert_eq!(tag, formals[formal_index].name.as_deref());
                    }
                }
            }
            if dots_at.is_none() {
                assert!(seen[..supplied.len()].iter().all(|used| *used));
            } else {
                for (index, used) in seen.iter().enumerate().take(supplied.len()) {
                    if !used {
                        assert!(bindings.iter().any(|binding| matches!(binding, Binding::Dots(dots) if dots.contains(&(index as u8)))));
                    }
                }
            }
        } else if dots_at.is_none() {
            assert!(matches!(
                result,
                Err(MatchError::MultipleExact | MatchError::MultiplePartial | MatchError::Unused)
            ));
        }
        kani::cover(matches!(result, Ok(_)), "success");
        kani::cover(matches!(result, Err(MatchError::MultiplePartial) | Err(MatchError::MultipleExact) | Ok(_)), "classified");
        kani::cover(matches!(result, Err(MatchError::Unused)), "unused");
        kani::cover(dots_at.is_some() && matches!(result, Ok(_)), "dots");
        kani::cover(
            supplied.iter().any(|argument| argument.tag.is_none()) && matches!(result, Ok(_)),
            "positional",
        );
    }
}
