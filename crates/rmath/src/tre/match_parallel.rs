#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
  tre/match_parallel.rs - TRE parallel regex matching engine

  Ported from tre-match-parallel.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;
use crate::mainutils::grep::{RegexWorkspace, WorkspaceBuffer};

#[derive(Clone, Copy)]
struct Reach {
    state: *mut tre_tnfa_transition_t,
    row: usize,
}

#[derive(Clone, Copy)]
struct ReachPos {
    pos: c_int,
    slot: usize,
}

/// Flat tag storage indexed by the row ids assigned by each matcher.
pub(super) struct TagRows {
    values: WorkspaceBuffer<c_int>,
    width: usize,
}

impl TagRows {
    pub(super) fn new(
        workspace: &RegexWorkspace,
        rows: usize,
        width: usize,
    ) -> Result<Self, ()> {
        let cells = rows.checked_mul(width).ok_or(())?;
        Ok(Self {
            values: filled(workspace, cells, 0)?,
            width,
        })
    }

    pub(super) fn row(&self, row: usize) -> &[c_int] {
        &self.values[self.range(row)]
    }

    pub(super) fn row_mut(&mut self, row: usize) -> &mut [c_int] {
        let range = self.range(row);
        &mut self.values[range]
    }

    fn range(&self, row: usize) -> std::ops::Range<usize> {
        let start = row * self.width;
        start..start + self.width
    }

    pub(super) fn copy_row(&mut self, source: usize, target: usize) {
        let range = self.range(source);
        self.values.copy_within(range, target * self.width);
    }
}

pub(super) fn filled<T: Clone>(
    workspace: &RegexWorkspace,
    len: usize,
    value: T,
) -> Result<WorkspaceBuffer<T>, ()> {
    let mut buffer = workspace.try_buffer(len)?;
    buffer.try_resize(len, value)?;
    Ok(buffer)
}

/// Apply a compiler-owned sentinel-terminated list to a bounded scratch row.
/// # Safety
/// Non-null `tags` must be a compiled transition's tag list.
pub(super) unsafe fn apply_tags(mut tags: *const c_int, row: &mut [c_int], pos: c_int) {
    if row.is_empty() || tags.is_null() {
        return;
    }
    // SAFETY: the compiled list is initialized through its first negative tag.
    unsafe {
        while *tags >= 0 {
            if let Some(tag) = row.get_mut(*tags as usize) {
                *tag = pos;
            }
            tags = tags.add(1);
        }
    }
}

pub(crate) fn tre_isalnum(c: tre_cint_t) -> bool {
    super::parse::tre_isalnum(c)
}

#[inline]
fn IS_WORD_CHAR(c: tre_cint_t) -> bool {
    c == '_' as tre_cint_t || tre_isalnum(c)
}

#[inline]
fn CHECK_ASSERTIONS(
    assertions: c_int,
    pos: c_int,
    prev_c: tre_cint_t,
    next_c: tre_cint_t,
    reg_notbol: c_int,
    reg_noteol: c_int,
    reg_newline: c_int,
) -> bool {
    ((assertions & ASSERT_AT_BOL) != 0
        && (pos > 0 || reg_notbol != 0)
        && (prev_c != '\n' as tre_cint_t || reg_newline == 0))
        || ((assertions & ASSERT_AT_EOL) != 0
            && (next_c != 0 || reg_noteol != 0)
            && (next_c != '\n' as tre_cint_t || reg_newline == 0))
        || ((assertions & ASSERT_AT_BOW) != 0 && (IS_WORD_CHAR(prev_c) || !IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_EOW) != 0 && (!IS_WORD_CHAR(prev_c) || IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_WB) != 0
            && (pos != 0 && next_c != 0 && IS_WORD_CHAR(prev_c) == IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_WB_NEG) != 0
            && (pos == 0 || next_c == 0 || IS_WORD_CHAR(prev_c) != IS_WORD_CHAR(next_c)))
}

#[inline]
/// # Safety
///
/// `trans_i.neg_classes` must be null or point to a live 0-terminated class array (as built by
/// `tre_compile`).
unsafe fn CHECK_CHAR_CLASSES(
    trans_i: &tre_tnfa_transition_t,
    tnfa: &tre_tnfa_t,
    eflags: c_int,
    prev_c: tre_cint_t,
) -> bool {
    let assertions = trans_i.assertions;
    let icase = tnfa.cflags & REG_ICASE;
    ((assertions & ASSERT_CHAR_CLASS) != 0
        && icase == 0
        && !super::parse::tre_isctype(prev_c, trans_i.u.get_class()))
        || ((assertions & ASSERT_CHAR_CLASS) != 0
            && icase != 0
            && !super::parse::tre_isctype(super::parse::tre_tolower(prev_c), trans_i.u.get_class())
            && !super::parse::tre_isctype(super::parse::tre_toupper(prev_c), trans_i.u.get_class()))
        || ((assertions & ASSERT_CHAR_CLASS_NEG) != 0
            // SAFETY: pointers are live TRE objects forwarded under the enclosing function's `# Safety` contract.
            && unsafe { super::parse::tre_neg_char_classes_match(trans_i.neg_classes, prev_c, icase) } != 0)
}

fn skip_minimal_state(
    pairs: &[[c_int; 2]],
    state: &[c_int],
    matched: &[c_int],
) -> Result<bool, ()> {
    for &[end, start] in pairs {
        let end = usize::try_from(end).map_err(|_| ())?;
        // The virtual final tag has no cell in the state or match buffer.
        if end >= matched.len() {
            return Ok(true);
        }
        let start = usize::try_from(start).map_err(|_| ())?;
        if state.get(start).ok_or(())? == matched.get(start).ok_or(())?
            && state.get(end).ok_or(())? < matched.get(end).ok_or(())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// # Safety
///
/// `tnfa` must be a live immutable compiler-produced descriptor, including
/// initialized `num_minimals` pairs when nonzero. `string` must satisfy the
/// byte/wide buffer and length/termination contract; `match_end_ofs` must be
/// writable and `match_tags` either null (no tags) or point to `tnfa.num_tags`
/// writable integers disjoint from inputs and each other. The output range is
/// written on success; it need not be initialized beforehand.
pub unsafe fn tre_tnfa_run_parallel(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    let workspace = RegexWorkspace::unbounded();
    let Ok(count) = usize::try_from(unsafe { (*tnfa).num_tags }) else {
        return REG_ESPACE;
    };
    let count = if match_tags.is_null() { 0 } else { count };
    let Ok(mut tags) = filled(&workspace, count, -1) else {
        return REG_ESPACE;
    };
    let status = unsafe {
        run_parallel(
            tnfa,
            string,
            len,
            type_,
            &mut tags,
            eflags,
            match_end_ofs,
            &workspace,
        )
    };
    if status == REG_OK && count != 0 {
        // SAFETY: caller supplied a disjoint output range; it need not be initialized.
        unsafe { ptr::copy_nonoverlapping(tags.as_ptr(), match_tags, count) };
    }
    status
}

#[allow(clippy::if_same_then_else)]
/// # Safety
///
/// `tnfa` must be a live immutable compiler-produced descriptor, including
/// initialized `num_minimals` pairs when nonzero. `string` must satisfy the
/// byte/wide buffer and length/termination contract; `match_end_ofs` must be
/// writable and `match_tags` either empty (no tags) or exactly `tnfa.num_tags`
/// writable integers disjoint from inputs and each other.
pub(super) unsafe fn run_parallel(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: &mut [c_int],
    eflags: c_int,
    match_end_ofs: *mut c_int,
    workspace: &RegexWorkspace,
) -> c_int {
    unsafe {
        let str_byte = string as *const u8;
        let mut pos: c_int = -1;
        let pos_add_next: u32 = 1;
        let reg_notbol = eflags & REG_NOTBOL;
        let reg_noteol = eflags & REG_NOTEOL;
        let reg_newline = (*tnfa).cflags & REG_NEWLINE;

        let mut prev_c: tre_cint_t = 0;
        let mut next_c: tre_cint_t = 0;

        let tag_count = match_tags.len();
        let Ok(compiled_tag_count) = usize::try_from((*tnfa).num_tags) else {
            return REG_ESPACE;
        };
        if tag_count != 0 && tag_count != compiled_tag_count {
            return REG_ESPACE;
        }
        let Ok(pair_count) = usize::try_from((*tnfa).num_minimals) else {
            return REG_ESPACE;
        };
        let pairs = if pair_count == 0 {
            &[][..]
        } else {
            let table = (*tnfa).minimal_tags;
            if table.is_null()
                || pair_count > compiled_tag_count
                || pair_count > isize::MAX as usize / std::mem::size_of::<[c_int; 2]>()
            {
                return REG_ESPACE;
            }
            // SAFETY: the compiled descriptor owns this many initialized pairs.
            std::slice::from_raw_parts(table.cast::<[c_int; 2]>(), pair_count)
        };
        let directions = if tag_count == 0 {
            &[][..]
        } else {
            // SAFETY: the compiled descriptor owns this many direction entries.
            std::slice::from_raw_parts((*tnfa).tag_directions, tag_count)
        };

        let Ok(state_count) = usize::try_from((*tnfa).num_states) else {
            return REG_ESPACE;
        };
        let scratch = || {
            let tmp_row = state_count.checked_mul(2).ok_or(())?;
            let tags = TagRows::new(workspace, tmp_row.checked_add(1).ok_or(())?, tag_count)?;
            let empty = Reach {
                state: ptr::null_mut(),
                row: 0,
            };
            let mut reach = filled(workspace, state_count, empty)?;
            let mut reach_next = filled(workspace, state_count, empty)?;
            for (index, (a, b)) in reach.iter_mut().zip(reach_next.iter_mut()).enumerate() {
                a.row = index;
                b.row = state_count + index;
            }
            let positions = filled(
                workspace,
                state_count,
                ReachPos { pos: -1, slot: 0 },
            )?;
            Ok::<_, ()>((tags, reach, reach_next, positions, tmp_row))
        };
        let Ok((mut tags, mut reach, mut reach_next, mut positions, mut tmp_row)) = scratch()
        else {
            return REG_ESPACE;
        };

        // First character optimization
        if (*tnfa).first_char >= 0 && type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
            let first = (*tnfa).first_char as u8;
            let mut found = false;
            let limit = if len >= 0 { len as usize } else { usize::MAX };
            let mut j = 0;
            while j < limit {
                let ch = *str_byte.add(j);
                if len < 0 && ch == 0 {
                    break;
                }
                if ch == first {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return REG_NOMATCH;
            }
            if j >= 1 {
                prev_c = *str_byte.add(j - 1) as tre_cint_t;
            }
            next_c = *str_byte.add(j) as tre_cint_t;
            pos = j as c_int;
            if len < 0 || pos < len {
                // str_byte already advanced
            }
        } else {
            // GET_NEXT_WCHAR - simplified for STR_BYTE
            prev_c = next_c;
            pos = 0;
            if len >= 0 && pos >= len {
                next_c = 0;
            } else if type_ == tre_str_type_t::STR_WIDE {
                next_c = *(string as *const tre_char_t);
            } else if !str_byte.is_null() {
                next_c = *str_byte as tre_cint_t;
            } else {
                next_c = 0;
            }
        }

        let mut match_eo: c_int = -1;
        let mut new_match: c_int = 0;
        let mut next_len = 0;

        loop {
            // Add initial states to reach_next if no match found yet
            if match_eo < 0 {
                let mut trans_i = (*tnfa).initial;
                while !(*trans_i).state.is_null() {
                    let Some(at) = usize::try_from((*trans_i).state_id)
                        .ok()
                        .and_then(|id| positions.get_mut(id))
                    else {
                        return REG_ESPACE;
                    };
                    if at.pos < pos {
                        if (*trans_i).assertions != 0
                            && CHECK_ASSERTIONS(
                                (*trans_i).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            )
                        {
                            trans_i = trans_i.add(1);
                            continue;
                        }

                        let Some(entry) = reach_next.get_mut(next_len) else {
                            return REG_ESPACE;
                        };
                        entry.state = (*trans_i).state;
                        let row = tags.row_mut(entry.row);
                        row.fill(-1);
                        apply_tags((*trans_i).tags, row, pos);
                        if entry.state == (*tnfa).final_ {
                            match_eo = pos;
                            new_match = 1;
                            match_tags.copy_from_slice(row);
                        }
                        *at = ReachPos {
                            pos,
                            slot: next_len,
                        };
                        next_len += 1;
                    }
                    trans_i = trans_i.add(1);
                }
            } else if tag_count == 0 || next_len == 0 {
                break;
            }

            // Check for end of string
            if len < 0 {
                if next_c == 0 {
                    break;
                }
            } else {
                if pos >= len {
                    break;
                }
            }

            // GET_NEXT_WCHAR
            prev_c = next_c;
            pos += 1;
            if len >= 0 && pos >= len {
                next_c = 0;
            } else if type_ == tre_str_type_t::STR_WIDE {
                next_c = *(string as *const tre_char_t).add(pos as usize);
            } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                next_c = *str_byte.add(pos as usize) as tre_cint_t;
            } else {
                next_c = 0;
            }

            // Swap reach and reach_next
            std::mem::swap(&mut reach, &mut reach_next);
            let mut reach_len = next_len;

            // Weed out states that don't fulfill minimal matching conditions
            if !pairs.is_empty() && new_match != 0 {
                new_match = 0;
                let mut kept = 0;
                for entry in &mut reach[..reach_len] {
                    let Ok(skip) =
                        skip_minimal_state(pairs, tags.row(entry.row), match_tags)
                    else {
                        return REG_ESPACE;
                    };
                    if !skip {
                        let next = &mut reach_next[kept];
                        next.state = entry.state;
                        std::mem::swap(&mut next.row, &mut entry.row);
                        kept += 1;
                    }
                }
                std::mem::swap(&mut reach, &mut reach_next);
                reach_len = kept;
            }

            // For each state in reach, find transitions
            next_len = 0;
            for entry in &reach[..reach_len] {
                let mut trans_i = entry.state;
                while !(*trans_i).state.is_null() {
                    if (*trans_i).code_min <= prev_c && (*trans_i).code_max >= prev_c {
                        if (*trans_i).assertions != 0
                            && (CHECK_ASSERTIONS(
                                (*trans_i).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            ) || CHECK_CHAR_CLASSES(&*trans_i, &*tnfa, eflags, prev_c))
                        {
                            trans_i = trans_i.add(1);
                            continue;
                        }

                        // Compute tags into the scratch row
                        tags.copy_row(entry.row, tmp_row);
                        apply_tags((*trans_i).tags, tags.row_mut(tmp_row), pos);

                        let Some(at) = usize::try_from((*trans_i).state_id)
                            .ok()
                            .and_then(|id| positions.get_mut(id))
                        else {
                            return REG_ESPACE;
                        };
                        if at.pos < pos {
                            let Some(next) = reach_next.get_mut(next_len) else {
                                return REG_ESPACE;
                            };
                            next.state = (*trans_i).state;
                            std::mem::swap(&mut next.row, &mut tmp_row);
                            *at = ReachPos {
                                pos,
                                slot: next_len,
                            };
                            let row = tags.row(next.row);
                            if next.state == (*tnfa).final_
                                && (match_eo == -1
                                    || (tag_count > 0 && row[0] <= match_tags[0]))
                            {
                                match_eo = pos;
                                new_match = 1;
                                match_tags.copy_from_slice(row);
                            }
                            next_len += 1;
                        } else {
                            // Another path reached this state - choose winner
                            if at.slot >= next_len {
                                return REG_ESPACE;
                            }
                            let next = &mut reach_next[at.slot];
                            if tre_tag_order(
                                directions,
                                tags.row(tmp_row),
                                tags.row(next.row),
                            ) != 0
                            {
                                std::mem::swap(&mut next.row, &mut tmp_row);
                                if (*trans_i).state == (*tnfa).final_ {
                                    match_eo = pos;
                                    new_match = 1;
                                    match_tags.copy_from_slice(tags.row(next.row));
                                }
                            }
                        }
                    }
                    trans_i = trans_i.add(1);
                }
            }
        }

        *match_end_ofs = match_eo;
        if match_eo >= 0 {
            REG_OK
        } else {
            REG_NOMATCH
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_tag_pruning_keeps_final_tag_and_checks_capture_indices() {
        let matched = [4, 8, 10];
        assert_eq!(skip_minimal_state(&[[2, 0]], &[4, 8, 9], &matched), Ok(true));
        assert_eq!(skip_minimal_state(&[[2, 0]], &[3, 8, 9], &matched), Ok(false));
        assert_eq!(
            skip_minimal_state(&[[2, 0], [1, 0]], &[4, 7, 10], &matched),
            Ok(true)
        );
        assert_eq!(skip_minimal_state(&[[3, -1]], &matched, &matched), Ok(true));
        assert_eq!(skip_minimal_state(&[[2, 0]], &[], &[]), Ok(true));
        assert_eq!(skip_minimal_state(&[[2, -1]], &matched, &matched), Err(()));
        assert_eq!(skip_minimal_state(&[[2, 3]], &matched, &matched), Err(()));
        assert_eq!(skip_minimal_state(&[[-1, 0]], &matched, &matched), Err(()));
        assert_eq!(skip_minimal_state(&[[2, 0]], &[4, 8], &matched), Err(()));
    }

    #[test]
    fn legacy_parallel_match_accepts_uninitialized_tags() {
        let pattern = std::ffi::CString::new("(a+)").unwrap();
        let mut preg = regex_t {
            re_nsub: 0,
            value: ptr::null_mut(),
        };
        // SAFETY: `preg` is writable and `pattern` is a NUL-terminated string.
        assert_eq!(
            unsafe {
                super::super::regapi::tre_regcomp(&mut preg, pattern.as_ptr(), REG_EXTENDED)
            },
            REG_OK
        );
        let descriptor = preg.value.cast::<tre_tnfa_t>();
        // SAFETY: `descriptor` was produced by the successful compile above.
        let count = usize::try_from(unsafe { (*descriptor).num_tags }).unwrap();
        assert!(count > 0);
        let mut slots = [std::mem::MaybeUninit::<c_int>::uninit(); 32];
        assert!(count <= slots.len());
        let mut end = 0;
        // SAFETY: the descriptor, byte buffer, and disjoint writable outputs
        // satisfy the legacy contract; the tag range need not be initialized.
        let status = unsafe {
            tre_tnfa_run_parallel(
                descriptor,
                b"aaa".as_ptr().cast(),
                3,
                tre_str_type_t::STR_BYTE,
                slots.as_mut_ptr().cast(),
                0,
                &mut end,
            )
        };
        assert_eq!(status, REG_OK);
        assert_eq!(end, 3);
        let via_raw: Vec<c_int> = slots[..count]
            .iter()
            // SAFETY: the successful run initialized all `count` entries.
            .map(|slot| unsafe { slot.assume_init() })
            .collect();

        let mut expected = vec![0; count];
        let mut expected_end = 0;
        let workspace = RegexWorkspace::unbounded();
        // SAFETY: same compiled descriptor and input; outputs are initialized.
        let status = unsafe {
            run_parallel(
                descriptor,
                b"aaa".as_ptr().cast(),
                3,
                tre_str_type_t::STR_BYTE,
                &mut expected,
                0,
                &mut expected_end,
                &workspace,
            )
        };
        assert_eq!(status, REG_OK);
        assert_eq!(expected_end, 3);
        assert_eq!(via_raw, expected);

        // SAFETY: `preg` was compiled above and is freed exactly once.
        unsafe { super::super::regapi::tre_regfree(&mut preg) };
    }
}
