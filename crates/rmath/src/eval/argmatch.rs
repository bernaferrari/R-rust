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

/// Formal after argument names have been interned to small ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NameFormal {
    name: Option<u8>,
    is_dots: bool,
}

/// Supplied argument after tags have been interned to the same id space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NameSupplied {
    tag: Option<u8>,
}

/// Three matching passes on interned name ids. Callers own the buffers, so the
/// Kani harness can pass stack arrays and never enter the allocator or `memchr`.
///
/// `formal_state` / `supplied_state` start at 0. Afterwards 2 means an exact
/// tag and 1 means a partial or positional claim. `chosen[i]` is the supplied
/// index bound to formal `i`. `is_prefix(name, tag)` is `name.starts_with(tag)`.
fn match_states(
    formals: &[NameFormal],
    supplied: &[NameSupplied],
    formal_state: &mut [u8],
    supplied_state: &mut [u8],
    chosen: &mut [Option<u8>],
    is_prefix: impl Fn(u8, u8) -> bool,
) -> Result<(), MatchError> {
    let mut dots_count = 0usize;
    let mut dots_at = None;
    let mut formal_index = 0usize;
    while formal_index < formals.len() {
        if formals[formal_index].is_dots {
            dots_count += 1;
            if dots_at.is_none() {
                dots_at = Some(formal_index);
            }
        }
        formal_index += 1;
    }
    if dots_count > 1 {
        return Err(MatchError::MultipleExact);
    }

    formal_index = 0;
    while formal_index < formals.len() {
        if !formals[formal_index].is_dots {
            if let Some(name) = formals[formal_index].name {
                let mut supplied_index = 0usize;
                while supplied_index < supplied.len() {
                    if let Some(tag) = supplied[supplied_index].tag {
                        if tag == name {
                            if formal_state[formal_index] == 2 || supplied_state[supplied_index] == 2 {
                                return Err(MatchError::MultipleExact);
                            }
                            chosen[formal_index] = Some(supplied_index as u8);
                            formal_state[formal_index] = 2;
                            supplied_state[supplied_index] = 2;
                        }
                    }
                    supplied_index += 1;
                }
            }
        }
        formal_index += 1;
    }

    formal_index = 0;
    while formal_index < formals.len() {
        if formal_state[formal_index] == 0
            && !formals[formal_index].is_dots
            && dots_at.is_none_or(|dots| formal_index <= dots)
        {
            if let Some(name) = formals[formal_index].name {
                let mut supplied_index = 0usize;
                while supplied_index < supplied.len() {
                    if supplied_state[supplied_index] != 2 {
                        if let Some(tag) = supplied[supplied_index].tag {
                            if tag != name && is_prefix(name, tag) {
                                if supplied_state[supplied_index] != 0 || formal_state[formal_index] == 1 {
                                    return Err(MatchError::MultiplePartial);
                                }
                                chosen[formal_index] = Some(supplied_index as u8);
                                formal_state[formal_index] = 1;
                                supplied_state[supplied_index] = 1;
                            }
                        }
                    }
                    supplied_index += 1;
                }
            }
        }
        formal_index += 1;
    }

    let mut supplied_index = 0usize;
    formal_index = 0;
    while formal_index < formals.len() {
        if formals[formal_index].is_dots {
            break;
        }
        if chosen[formal_index].is_none() {
            while supplied_index < supplied.len()
                && (supplied_state[supplied_index] != 0 || supplied[supplied_index].tag.is_some())
            {
                supplied_index += 1;
            }
            if supplied_index >= supplied.len() {
                break;
            }
            chosen[formal_index] = Some(supplied_index as u8);
            supplied_state[supplied_index] = 1;
            supplied_index += 1;
        }
        formal_index += 1;
    }

    if dots_at.is_none() {
        let mut index = 0usize;
        while index < supplied.len() {
            if supplied_state[index] == 0 {
                return Err(MatchError::Unused);
            }
            index += 1;
        }
    }
    Ok(())
}

#[cfg(not(kani))]
fn intern_name<'a>(names: &mut Vec<&'a str>, text: &'a str) -> u8 {
    if let Some(index) = names.iter().position(|name| *name == text) {
        return index as u8;
    }
    let id = u8::try_from(names.len()).expect("at most 256 distinct argument names");
    names.push(text);
    id
}

/// Match supplied arguments to formals.
///
/// Exact tags, then partial prefixes before the first `...`, then positional
/// fills of untagged actuals into still-unmatched formals before `...`.
/// Leftovers go to `...` in supplied order when it exists. Otherwise they
/// are [`MatchError::Unused`]. Formals after `...` accept exact tags only.
#[cfg(not(kani))]
pub(crate) fn match_formals(
    formals: &[Formal<'_>],
    supplied: &[Supplied<'_>],
) -> Result<Vec<Binding>, MatchError> {
    let mut names = Vec::new();
    let id_formals: Vec<NameFormal> = formals
        .iter()
        .map(|formal| NameFormal {
            name: match formal.name {
                Some(text) if !formal.is_dots => Some(intern_name(&mut names, text)),
                _ => None,
            },
            is_dots: formal.is_dots,
        })
        .collect();
    let id_supplied: Vec<NameSupplied> = supplied
        .iter()
        .map(|argument| NameSupplied {
            tag: argument.tag.map(|text| intern_name(&mut names, text)),
        })
        .collect();
    match_ids(&id_formals, &id_supplied, |name, tag| {
        names[usize::from(name)].starts_with(names[usize::from(tag)])
    })
}

/// [`match_states`] plus the `Vec` projection the oracle compares to both ports.
#[cfg(not(kani))]
fn match_ids(
    formals: &[NameFormal],
    supplied: &[NameSupplied],
    is_prefix: impl Fn(u8, u8) -> bool,
) -> Result<Vec<Binding>, MatchError> {
    let mut formal_state = vec![0u8; formals.len()];
    let mut supplied_state = vec![0u8; supplied.len()];
    let mut chosen = vec![None; formals.len()];
    match_states(
        formals,
        supplied,
        &mut formal_state,
        &mut supplied_state,
        &mut chosen,
        is_prefix,
    )?;
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
                Binding::Actual(index)
            } else {
                Binding::Missing
            }
        })
        .collect())
}

/// Prefix table for the names `"a"` and `"ab"`. Equal ids count as prefixes.
#[cfg(any(test, kani))]
fn proof_name_is_prefix(name: u8, tag: u8) -> bool {
    matches!((name, tag), (0, 0) | (1, 0) | (1, 1))
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

    #[test]
    fn integer_prefix_matches_starts_with_and_both_matchers() {
        const NAMES: [&str; 2] = ["a", "ab"];
        for name in 0..2u8 {
            for tag in 0..2u8 {
                assert_eq!(
                    proof_name_is_prefix(name, tag),
                    NAMES[usize::from(name)].starts_with(NAMES[usize::from(tag)])
                );
            }
        }
        let formal_of = |choice: u8| -> (Formal<'static>, NameFormal) {
            match choice {
                0 => (
                    Formal {
                        name: None,
                        is_dots: true,
                    },
                    NameFormal {
                        name: None,
                        is_dots: true,
                    },
                ),
                1 => (formal(NAMES[0]), NameFormal { name: Some(0), is_dots: false }),
                _ => (formal(NAMES[1]), NameFormal { name: Some(1), is_dots: false }),
            }
        };
        let supplied_of = |choice: u8| -> (Supplied<'static>, NameSupplied) {
            match choice {
                0 => (positional(), NameSupplied { tag: None }),
                1 => (tagged(NAMES[0]), NameSupplied { tag: Some(0) }),
                _ => (tagged(NAMES[1]), NameSupplied { tag: Some(1) }),
            }
        };
        for f0 in 0..3u8 {
            for f1 in 0..3u8 {
                for s0 in 0..3u8 {
                    for s1 in 0..3u8 {
                        let (left_formal, left_id) = formal_of(f0);
                        let (right_formal, right_id) = formal_of(f1);
                        let (left_supplied, left_tag) = supplied_of(s0);
                        let (right_supplied, right_tag) = supplied_of(s1);
                        assert_eq!(
                            match_formals(&[left_formal, right_formal], &[left_supplied, right_supplied]),
                            match_ids(
                                &[left_id, right_id],
                                &[left_tag, right_tag],
                                proof_name_is_prefix
                            ),
                            "formals {f0},{f1} supplied {s0},{s1}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::{MatchError, NameFormal, NameSupplied, match_states, proof_name_is_prefix};

    #[kani::proof]
    #[kani::unwind(3)]
    fn argmatch_spec() {
        // Ids 0 and 1 stand for "a" and "ab". The unit test checks that table
        // against `str::starts_with`. This harness never builds a string.
        let mut formals = [NameFormal { name: None, is_dots: false }; 2];
        let mut supplied = [NameSupplied { tag: None }; 2];
        let mut index = 0usize;
        while index < 2 {
            let is_dots: bool = kani::any();
            let name_index: u8 = kani::any();
            kani::assume(name_index < 2);
            formals[index] = NameFormal {
                name: if is_dots { None } else { Some(name_index) },
                is_dots,
            };
            let tagged: bool = kani::any();
            let tag_index: u8 = kani::any();
            kani::assume(tag_index < 2);
            supplied[index] = NameSupplied {
                tag: if tagged { Some(tag_index) } else { None },
            };
            index += 1;
        }

        let mut formal_state = [0u8; 2];
        let mut supplied_state = [0u8; 2];
        let mut chosen = [None; 2];
        let result = match_states(
            &formals,
            &supplied,
            &mut formal_state,
            &mut supplied_state,
            &mut chosen,
            proof_name_is_prefix,
        );

        let mut dots_at = None;
        let mut dots_count = 0u8;
        index = 0;
        while index < 2 {
            if formals[index].is_dots {
                dots_count += 1;
                if dots_at.is_none() {
                    dots_at = Some(index);
                }
            }
            index += 1;
        }

        if result.is_ok() {
            let mut seen = [false; 2];
            index = 0;
            while index < 2 {
                if let Some(supplied_index) = chosen[index] {
                    let supplied_index = supplied_index as usize;
                    assert!(supplied_index < 2);
                    assert!(!seen[supplied_index]);
                    seen[supplied_index] = true;
                    if dots_at.is_some_and(|dots| index > dots) {
                        assert_eq!(supplied[supplied_index].tag, formals[index].name);
                    }
                }
                index += 1;
            }
            index = 0;
            let mut previous_dot = None;
            while index < 2 {
                if supplied_state[index] == 0 {
                    assert!(dots_at.is_some());
                    if let Some(previous) = previous_dot {
                        assert!(index > previous);
                    }
                    previous_dot = Some(index);
                } else {
                    assert!(seen[index]);
                }
                index += 1;
            }
        } else if dots_count < 2 {
            assert!(matches!(
                result,
                Err(MatchError::MultipleExact | MatchError::MultiplePartial | MatchError::Unused)
            ));
        }
        kani::cover(result.is_ok(), "success");
        kani::cover(matches!(result, Err(MatchError::MultipleExact)), "multiple exact");
        kani::cover(matches!(result, Err(MatchError::MultiplePartial)), "multiple partial");
        kani::cover(matches!(result, Err(MatchError::Unused)), "unused");
        kani::cover(dots_at.is_some() && result.is_ok(), "dots");
        kani::cover(supplied[0].tag.is_none() && result.is_ok(), "positional");
    }
}
