#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
  tre/match_backtrack.rs - TRE backtracking regex matching engine

  Ported from tre-match-backtrack.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;
use crate::mainutils::grep::{RegexWorkspace, WorkspaceBuffer};

use super::match_input::MatchInput;
use super::match_parallel::{apply_tags, filled};

#[derive(Clone, Copy)]
struct BacktrackFrame {
    pos: c_int,
    state: *mut tre_tnfa_transition_t,
    next_c: tre_cint_t,
    seen_slot: Option<c_int>,
}

struct BacktrackStack {
    frames: WorkspaceBuffer<BacktrackFrame>,
    tag_rows: WorkspaceBuffer<c_int>,
    depth: usize,
    width: usize,
}

impl BacktrackStack {
    fn new(workspace: &RegexWorkspace, width: usize) -> Result<Self, ()> {
        Ok(Self {
            frames: workspace.try_buffer(0)?,
            tag_rows: workspace.try_buffer(0)?,
            depth: 0,
            width,
        })
    }

    fn push(&mut self, frame: BacktrackFrame, tags: &[c_int]) -> Result<&mut [c_int], ()> {
        if self.depth == self.frames.len() {
            let capacity = self.depth.checked_mul(2).ok_or(())?.max(16);
            let cells = capacity.checked_mul(self.width).ok_or(())?;
            self.tag_rows.try_resize(cells, 0)?;
            self.frames.try_resize(capacity, frame)?;
        }
        // Both buffers cover this depth, including when the tag width is zero.
        let start = self.depth * self.width;
        let row = &mut self.tag_rows[start..start + self.width];
        row.copy_from_slice(tags);
        self.frames[self.depth] = frame;
        self.depth += 1;
        Ok(row)
    }

    fn pop(
        &mut self,
        tags: &mut [c_int],
        states_seen: &mut [bool],
    ) -> Result<Option<BacktrackFrame>, c_int> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Ok(None);
        };
        let frame = self.frames[depth];
        if let Some(state_id) = frame.seen_slot {
            let Some(seen) = states_seen.get_mut(state_id as usize) else {
                return Err(REG_BADPAT);
            };
            *seen = false;
        }
        let start = depth * self.width;
        tags.copy_from_slice(&self.tag_rows[start..start + self.width]);
        self.depth = depth;
        Ok(Some(frame))
    }
}

#[inline]
fn IS_WORD_CHAR(c: tre_cint_t) -> bool {
    c == '_' as tre_cint_t || super::parse::tre_isalnum(c)
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

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; `string` must describe `len` code units or be NUL-terminated when `len` is negative;
/// null is accepted only where the original C TRE API accepts it.
pub unsafe fn tre_tnfa_run_backtrack(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    // SAFETY: forward the legacy pointer contracts unchanged.
    unsafe {
        run_backtrack(
            tnfa,
            string,
            len,
            type_,
            match_tags,
            eflags,
            match_end_ofs,
            &RegexWorkspace::unbounded(),
        )
    }
}

#[allow(clippy::if_same_then_else)]
/// # Safety
/// Same raw pointer requirements as [`tre_tnfa_run_backtrack`].
pub(super) unsafe fn run_backtrack(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
    workspace: &RegexWorkspace,
) -> c_int {
    // SAFETY: the raw matcher boundary supplies the input extent or terminator.
    let input = match unsafe { MatchInput::from_raw(string, len, type_) } {
        Ok(input) => input,
        Err(status) => return status,
    };
    let input_len = input.len() as c_int;
    unsafe {
        let mut pos: c_int = 0;

        let reg_notbol = eflags & REG_NOTBOL;
        let reg_noteol = eflags & REG_NOTEOL;
        let reg_newline = (*tnfa).cflags & REG_NEWLINE;

        let mut next_c_start: tre_cint_t = 0;
        let mut pos_start: c_int = -1;

        let mut match_eo: c_int = -1;
        let mut next_tags: *const c_int = ptr::null();
        let mut state: *mut tre_tnfa_transition_t = ptr::null_mut();

        let (Ok(tag_count), Ok(state_count)) = (
            usize::try_from((*tnfa).num_tags),
            usize::try_from((*tnfa).num_states),
        ) else {
            return REG_BADPAT;
        };
        let Ok(mut tags) = filled(workspace, tag_count, -1) else {
            return REG_ESPACE;
        };
        let Ok(mut pmatch) = filled(
            workspace,
            (*tnfa).num_submatches as usize,
            regmatch_t::default(),
        ) else {
            return REG_ESPACE;
        };
        let Ok(mut states_seen) = filled(workspace, state_count, false) else {
            return REG_ESPACE;
        };
        let Ok(mut stack) = BacktrackStack::new(workspace, tag_count) else {
            return REG_ESPACE;
        };

        let mut prev_c: tre_cint_t = 0;
        let mut next_c: tre_cint_t = 0;

        // retry label
        loop {
            // Initialize tags and states_seen
            tags.fill(-1);
            if !match_tags.is_null() && match_eo < 0 {
                for i in 0..tags.len() {
                    match_tags.add(i).write(-1);
                }
            }
            states_seen.fill(false);

            state = ptr::null_mut();
            pos = pos_start;

            // GET_NEXT_WCHAR
            prev_c = next_c;
            pos += 1;
            next_c = input.at(pos);

            pos_start = pos;
            next_c_start = next_c;

            // Handle initial states
            next_tags = ptr::null();
            let mut trans_i = (*tnfa).initial;
            while !(*trans_i).state.is_null() {
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
                if state.is_null() {
                    state = (*trans_i).state;
                    next_tags = (*trans_i).tags;
                } else {
                    // Backtrack to this state
                    // BT_STACK_PUSH
                    let frame = BacktrackFrame {
                        pos,
                        state: (*trans_i).state,
                        next_c,
                        // The transition's destination is non-null in this loop.
                        seen_slot: (((*(*trans_i).state).assertions & ASSERT_BACKREF) != 0)
                            .then_some((*trans_i).state_id),
                    };
                    let Ok(row) = stack.push(frame, &tags) else {
                        return REG_ESPACE;
                    };
                    apply_tags((*trans_i).tags, row, pos);
                }
                trans_i = trans_i.add(1);
            }

            apply_tags(next_tags, &mut tags, pos);

            if state.is_null() {
                // goto backtrack
                let Ok(frame) = stack.pop(&mut tags, &mut states_seen) else {
                    return REG_BADPAT;
                };
                if let Some(frame) = frame {
                    pos = frame.pos;
                    state = frame.state;
                    next_c = frame.next_c;
                    continue; // Back to main loop
                } else if match_eo < 0 {
                    // Try starting from a later position
                    if pos_start >= input_len {
                        break;
                    }
                    next_c = next_c_start;
                    continue; // retry
                } else {
                    break;
                }
            }

            // Main matching loop
            let mut done = false;
            while !done {
                if state == (*tnfa).final_ {
                    if match_eo < pos
                        || (match_eo == pos
                            && !match_tags.is_null()
                            && tre_tag_order(
                                std::slice::from_raw_parts(
                                    ptr_or_dangling((*tnfa).tag_directions),
                                    tag_count,
                                ),
                                &tags,
                                std::slice::from_raw_parts(ptr_or_dangling(match_tags), tag_count),
                            ) != 0)
                    {
                        match_eo = pos;
                        if !match_tags.is_null() {
                            ptr::copy_nonoverlapping(tags.as_ptr(), match_tags, tags.len());
                        }
                    }
                    // goto backtrack
                    let Ok(frame) = stack.pop(&mut tags, &mut states_seen) else {
                        return REG_BADPAT;
                    };
                    if let Some(frame) = frame {
                        pos = frame.pos;
                        state = frame.state;
                        next_c = frame.next_c;
                        continue;
                    } else if match_eo < 0 {
                        if pos_start >= input_len {
                            break;
                        }
                        next_c = next_c_start;
                        done = true;
                    } else {
                        break;
                    }
                    continue;
                }

                let mut empty_br_match: c_int = 0;
                let trans_i = state;

                if !(*trans_i).state.is_null() && (*trans_i).assertions & ASSERT_BACKREF != 0 {
                    // Back reference
                    let Ok(bt) = usize::try_from((*trans_i).u.get_backref()) else {
                        return REG_BADPAT;
                    };
                    let Some(prefix) = pmatch.get_mut(..=bt) else {
                        return REG_BADPAT;
                    };
                    // Get the substring to match against
                    super::regapi::tre_fill_pmatch(
                        prefix.len(),
                        prefix.as_mut_ptr(),
                        (*tnfa).cflags & !REG_NOSUB,
                        tnfa,
                        tags.as_ptr(),
                        pos,
                    );
                    let regmatch_t {
                        rm_so: so,
                        rm_eo: eo,
                    } = prefix[bt];
                    if let Some(end) = input.backref_end(so, eo, pos) {
                        if end == pos {
                            empty_br_match = 1;
                        }
                        let Some(&seen) = states_seen.get((*trans_i).state_id as usize) else {
                            return REG_BADPAT;
                        };
                        if empty_br_match != 0 && seen {
                            // goto backtrack
                            let Ok(frame) = stack.pop(&mut tags, &mut states_seen) else {
                                return REG_BADPAT;
                            };
                            if let Some(frame) = frame {
                                pos = frame.pos;
                                state = frame.state;
                                next_c = frame.next_c;
                                continue;
                            } else if match_eo < 0 {
                                if pos_start >= input_len {
                                    break;
                                }
                                next_c = next_c_start;
                                done = true;
                            } else {
                                break;
                            }
                            continue;
                        }
                        states_seen[(*trans_i).state_id as usize] = empty_br_match != 0;
                        // Advance in input string
                        prev_c = next_c;
                        pos = end;
                        next_c = input.at(pos);
                    } else {
                        // goto backtrack
                        let Ok(frame) = stack.pop(&mut tags, &mut states_seen) else {
                            return REG_BADPAT;
                        };
                        if let Some(frame) = frame {
                            pos = frame.pos;
                            state = frame.state;
                            next_c = frame.next_c;
                            continue;
                        } else if match_eo < 0 {
                            if pos_start >= input_len {
                                break;
                            }
                            next_c = next_c_start;
                            done = true;
                        } else {
                            break;
                        }
                        continue;
                    }
                } else {
                    // Check for end of string
                    if pos >= input_len {
                        // goto backtrack
                        let Ok(frame) = stack.pop(&mut tags, &mut states_seen) else {
                            return REG_BADPAT;
                        };
                        if let Some(frame) = frame {
                            pos = frame.pos;
                            state = frame.state;
                            next_c = frame.next_c;
                            continue;
                        } else if match_eo < 0 {
                            if pos_start >= input_len {
                                break;
                            }
                            next_c = next_c_start;
                            done = true;
                        } else {
                            break;
                        }
                        continue;
                    }

                    // GET_NEXT_WCHAR
                    prev_c = next_c;
                    pos += 1;
                    next_c = input.at(pos);
                }

                let mut next_state: *mut tre_tnfa_transition_t = ptr::null_mut();
                let mut trans_i = state;
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

                        if next_state.is_null() {
                            next_state = (*trans_i).state;
                            next_tags = (*trans_i).tags;
                        } else {
                            // BT_STACK_PUSH
                            let frame = BacktrackFrame {
                                pos,
                                state: (*trans_i).state,
                                next_c,
                                seen_slot: (((*(*trans_i).state).assertions & ASSERT_BACKREF) != 0)
                                    .then_some((*trans_i).state_id),
                            };
                            let Ok(row) = stack.push(frame, &tags) else {
                                return REG_ESPACE;
                            };
                            apply_tags((*trans_i).tags, row, pos);
                        }
                    }
                    trans_i = trans_i.add(1);
                }

                if !next_state.is_null() {
                    state = next_state;
                    apply_tags(next_tags, &mut tags, pos);
                } else {
                    // goto backtrack
                    let Ok(frame) = stack.pop(&mut tags, &mut states_seen) else {
                        return REG_BADPAT;
                    };
                    if let Some(frame) = frame {
                        pos = frame.pos;
                        state = frame.state;
                        next_c = frame.next_c;
                    } else if match_eo < 0 {
                        if pos_start >= input_len {
                            break;
                        }
                        next_c = next_c_start;
                        done = true;
                    } else {
                        break;
                    }
                }
            }
            // Only an explicit retry advances to another starting position.
            if !done {
                break;
            }
        }

        let ret = if match_eo >= 0 {
            REG_OK as c_int
        } else {
            REG_NOMATCH as c_int
        };
        *match_end_ofs = match_eo;

        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backtrack_stack_restores_saved_rows_in_reverse_order() {
        let workspace = RegexWorkspace::unbounded();
        let mut stack = BacktrackStack::new(&workspace, 2).unwrap();
        for pos in 0..40 {
            let row = stack
                .push(
                    BacktrackFrame {
                        pos,
                        state: ptr::null_mut(),
                        next_c: 0,
                        seen_slot: (pos % 2 == 0).then_some(0),
                    },
                    &[pos, -1],
                )
                .unwrap();
            row[1] = pos + 1;
        }
        let mut tags = [-1; 2];
        let mut seen = [true];
        for pos in (0..40).rev() {
            seen[0] = true;
            assert_eq!(stack.pop(&mut tags, &mut seen).unwrap().unwrap().pos, pos);
            assert_eq!(tags, [pos, pos + 1]);
            assert_eq!(seen[0], pos % 2 != 0);
        }
        assert!(stack.pop(&mut tags, &mut seen).unwrap().is_none());
    }
}
