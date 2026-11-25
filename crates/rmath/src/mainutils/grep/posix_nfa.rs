#![forbid(unsafe_code)]
//! POSIX leftmost-longest tagged Thompson NFA simulation.
//!
//! POSIX semantics differ fundamentally from Perl-ish backtracking: the
//! overall match is the LONGEST possible at the leftmost start position,
//! regardless of alternation order or greedy/lazy preference. Neither the
//! `regex` crate nor `fancy-regex` implements these semantics, so this is a
//! focused tagged-NFA simulator in the spirit of TRE's tnfa — pure safe
//! Rust, parsing via `regex-syntax` HIR.
//!
//! Semantics implemented:
//! - leftmost-longest overall match: scanning continues while any thread
//!   from the leftmost matching start survives; the last input position at
//!   which an accepting thread exists is the match end.
//! - submatch priority: threads are priority-ordered exactly like a greedy
//!   backtracker (repetition bodies preferred over exits, alternation
//!   branches in written order, all lazy modifiers meaningless in POSIX).
//!   When two threads merge at the same NFA state, the higher-priority
//!   thread's capture slots survive. This approximates TRE's POSIX submatch
//!   behavior for the cases the oracle exercises.
//! - captures: group slots are overwritten per iteration, so inner groups
//!   report their final iteration, matching TRE.
//!
//! Input alphabet: Unicode mode decodes UTF-8 chars as u32 codepoints;
//! bytes mode maps each byte to u32 directly — exactly TRE's wide/byte
//! duality. Match positions are reported as byte offsets into the original
//! text.

use regex_syntax::hir::{self, Hir, HirKind};
use regex_syntax::ParserBuilder;

#[derive(Clone, Debug)]
enum Inst {
    /// Consume one input char whose u32 falls in any inclusive range.
    Consume(Vec<(u32, u32)>),
    /// Consume any one char except newline (REG_NEWLINE dot semantics).
    ConsumeAny,
    /// Split execution: `0` is the higher-priority continuation.
    Split(usize, usize),
    Jmp(usize),
    /// Record the current input position into capture slot N.
    Save(usize),
    /// Assertion: start of text (not start of scan offset).
    AssertTextStart,
    /// Assertion: end of text.
    AssertTextEnd,
    /// Assertion: position after a newline (or text start) — REG_NEWLINE `^`.
    AssertLineStart,
    /// Assertion: position before a newline (or text end) — REG_NEWLINE `$`.
    AssertLineEnd,
    Match,
}

pub struct PosixNfa {
    prog: Vec<Inst>,
    nslots: usize,
}

/// One thread of the simulation: priority-ordered, carrying capture slots.
/// `start` records which scan start position this thread descends from, so
/// leftmost-longest bookkeeping can ignore matches from later starts.
#[derive(Clone)]
struct Thread {
    pc: usize,
    slots: Vec<Option<usize>>,
    start: usize,
}

pub fn compile(pattern: &str, ignore_case: bool, bytes_mode: bool) -> Result<PosixNfa, String> {
    let hir = ParserBuilder::new()
        .unicode(!bytes_mode)
        // Bytes mode must accept arbitrary byte patterns: `utf8(false)`
        // yields byte-oriented classes (dot = \\x00-\\xFF minus newline)
        // instead of rejecting patterns that can match invalid UTF-8.
        .utf8(!bytes_mode)
        .case_insensitive(ignore_case)
        .multi_line(true) // REG_NEWLINE: ^/$ operate at line boundaries
        .dot_matches_new_line(false) // REG_NEWLINE: dot excludes newline
        .build()
        .parse(pattern)
        .map_err(|err| err.to_string())?;
    let mut prog = Vec::new();
    // Slot 0/1 are the whole-match positions; capture group N uses 2N/2N+1.
    prog.push(Inst::Save(0));
    compile_hir(&hir, &mut prog, bytes_mode)?;
    prog.push(Inst::Save(1));
    prog.push(Inst::Match);
    // explicit groups are numbered from 1; slot pair 0 is the whole match.
    let groups = hir.properties().explicit_captures_len();
    Ok(PosixNfa {
        prog,
        nslots: 2 * (groups + 1),
    })
}

fn compile_hir(hir: &Hir, prog: &mut Vec<Inst>, bytes_mode: bool) -> Result<(), String> {
    match hir.kind() {
        HirKind::Empty => Ok(()),
        HirKind::Literal(lit) => {
            if bytes_mode {
                // Bytes mode consumes raw bytes 1:1 — never decode UTF-8:
                // lossy decoding would corrupt non-ASCII byte sequences and
                // break exact byte matching.
                for &byte in lit.0.iter() {
                    let c = u32::from(byte);
                    prog.push(Inst::Consume(vec![(c, c)]));
                }
            } else {
                // Unicode mode: the literal is valid UTF-8 by construction.
                let text = String::from_utf8_lossy(&lit.0);
                for ch in text.chars() {
                    let c = ch as u32;
                    prog.push(Inst::Consume(vec![(c, c)]));
                }
            }
            Ok(())
        }
        HirKind::Class(class) => {
            let ranges = match class {
                hir::Class::Unicode(uc) => uc
                    .iter()
                    .map(|r| (r.start() as u32, r.end() as u32))
                    .collect(),
                hir::Class::Bytes(bc) => bc
                    .iter()
                    .map(|r| (r.start() as u32, r.end() as u32))
                    .collect(),
            };
            prog.push(Inst::Consume(ranges));
            Ok(())
        }
        HirKind::Look(look) => {
            match look {
                hir::Look::Start => prog.push(Inst::AssertTextStart),
                hir::Look::End => prog.push(Inst::AssertTextEnd),
                hir::Look::StartLF | hir::Look::StartCRLF => prog.push(Inst::AssertLineStart),
                hir::Look::EndLF | hir::Look::EndCRLF => prog.push(Inst::AssertLineEnd),
                _ => {
                    // POSIX has no word boundaries; TRE exposes them only as
                    // \< \> \b extensions the dialect translation never emits.
                    return Err("word boundaries are not POSIX ERE".to_string());
                }
            }
            Ok(())
        }
        HirKind::Capture(cap) => {
            prog.push(Inst::Save(2 * cap.index as usize));
            compile_hir(&cap.sub, prog, bytes_mode)?;
            prog.push(Inst::Save(2 * cap.index as usize + 1));
            Ok(())
        }
        HirKind::Concat(subs) => {
            for sub in subs {
                compile_hir(sub, prog, bytes_mode)?;
            }
            Ok(())
        }
        HirKind::Alternation(subs) => {
            // Chain of splits: first alternative highest priority.
            let mut jmp_holes = Vec::new();
            for (i, sub) in subs.iter().enumerate() {
                if i + 1 < subs.len() {
                    let split_pos = prog.len();
                    prog.push(Inst::Split(usize::MAX, usize::MAX));
                    let body_start = prog.len();
                    compile_hir(sub, prog, bytes_mode)?;
                    let jmp_pos = prog.len();
                    prog.push(Inst::Jmp(usize::MAX));
                    jmp_holes.push(jmp_pos);
                    let next_alt = prog.len();
                    prog[split_pos] = Inst::Split(body_start, next_alt);
                } else {
                    compile_hir(sub, prog, bytes_mode)?;
                }
            }
            let end = prog.len();
            for hole in jmp_holes {
                prog[hole] = Inst::Jmp(end);
            }
            Ok(())
        }
        HirKind::Repetition(rep) => {
            // regex-syntax normalizes `?`/`+`/`*` into {0,1}/{1,}/{0,} ranges.
            // POSIX: greedy/lazy preference is irrelevant — overall longest
            // wins either way, so every expansion uses greedy priority order.
            let min = rep.min;
            let max = rep.max;
            for _ in 0..min {
                compile_hir(&rep.sub, prog, bytes_mode)?;
            }
            match max {
                None => {
                    if min == 0 {
                        // star: split(body, end); body; jmp split
                        let split_pos = prog.len();
                        prog.push(Inst::Split(usize::MAX, usize::MAX));
                        let body = prog.len();
                        compile_hir(&rep.sub, prog, bytes_mode)?;
                        prog.push(Inst::Jmp(split_pos));
                        let end = prog.len();
                        prog[split_pos] = Inst::Split(body, end);
                    } else {
                        // plus tail after mandatory copies: split(body, end)
                        let split_pos = prog.len();
                        prog.push(Inst::Split(usize::MAX, usize::MAX));
                        let body = prog.len();
                        compile_hir(&rep.sub, prog, bytes_mode)?;
                        prog.push(Inst::Jmp(split_pos));
                        let end = prog.len();
                        prog[split_pos] = Inst::Split(body, end);
                    }
                }
                Some(n) => {
                    // Optional copies each split around their body; all exits
                    // jump forward past the repetition.
                    let mut holes = Vec::new();
                    for _ in min..n {
                        let split_pos = prog.len();
                        prog.push(Inst::Split(usize::MAX, usize::MAX));
                        holes.push(split_pos);
                        let body = prog.len();
                        compile_hir(&rep.sub, prog, bytes_mode)?;
                        prog[split_pos] = Inst::Split(body, usize::MAX);
                    }
                    let end = prog.len();
                    for hole in holes {
                        if let Inst::Split(b, _) = prog[hole] {
                            prog[hole] = Inst::Split(b, end);
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

impl PosixNfa {
    /// Leftmost-longest match of the pattern in `chars` (u32 alphabet)
    /// scanning from `start_at`. Returns capture slots in char positions:
    /// slots[0]/slots[1] are the whole-match start/end.
    pub fn captures_chars(&self, chars: &[u32], start_at: usize) -> Option<Vec<Option<usize>>> {
        let mut clist: Vec<Thread> = Vec::new();
        let mut fixed_start: Option<usize> = None;
        let mut best: Option<(usize, Vec<Option<usize>>)> = None; // (end, slots)

        for pos in start_at..=chars.len() {
            // Inject a fresh start thread while no leftmost match exists.
            if fixed_start.is_none() {
                let t = Thread {
                    pc: 0,
                    slots: vec![None; self.nslots],
                    start: pos,
                };
                // Closure expands the entry thread; appended in priority
                // order after the initial entry-state insertion point.
                self.add_thread(&mut clist, t, chars, pos);
            }
            // Record accepts among current threads at this position. Thread
            // order is priority order, so the first accepting thread at this
            // position carries the winning capture slots. POSIX longest: a
            // strictly later end always wins regardless of priority.
            for t in &clist {
                if self.prog[t.pc] == Inst::Match {
                    let s = t.start;
                    match fixed_start {
                        None => {
                            fixed_start = Some(s);
                            best = Some((pos, t.slots.clone()));
                            break;
                        }
                        Some(fs) if fs == s => {
                            let advance = best.as_ref().map_or(true, |(end, _)| pos > *end);
                            if advance {
                                best = Some((pos, t.slots.clone()));
                            }
                            break;
                        }
                        _ => {}
                    }
                }
            }
            if pos == chars.len() {
                break;
            }
            // Stop early once all surviving threads predate the fixed start:
            // nothing later can extend the leftmost match.
            if let Some(fs) = fixed_start {
                if clist.iter().all(|t| t.start > fs) {
                    break;
                }
            }
            // Step: build next thread list.
            let ch = chars[pos];
            let mut nlist: Vec<Thread> = Vec::new();
            for t in &clist {
                // Once the leftmost start is fixed, threads from later starts
                // can never win — drop them to bound work.
                if let Some(fs) = fixed_start {
                    if t.start > fs {
                        continue;
                    }
                }
                match &self.prog[t.pc] {
                    Inst::Consume(ranges) => {
                        if ranges.iter().any(|&(lo, hi)| ch >= lo && ch <= hi) {
                            let mut nt = t.clone();
                            nt.pc += 1;
                            self.add_thread(&mut nlist, nt, chars, pos + 1);
                        }
                    }
                    Inst::ConsumeAny => {
                        if ch != b'\n' as u32 {
                            let mut nt = t.clone();
                            nt.pc += 1;
                            self.add_thread(&mut nlist, nt, chars, pos + 1);
                        }
                    }
                    // Match is terminal: accepting threads are recorded in the
                    // accept pass above and have no outgoing transitions.
                    Inst::Match => {}
                    other => unreachable!("closure leaves only consume instructions, got {:?}", other),
                }
            }
            clist = nlist;
            if clist.is_empty() && fixed_start.is_some() {
                break;
            }
        }
        best.map(|(end, mut slots)| {
            slots[1] = Some(end);
            slots
        })
    }

    /// Epsilon-closure insertion preserving priority order. Threads already
    /// in the list keep priority; when a duplicate state is reached the
    /// higher-priority (earlier-inserted) thread wins.
    fn add_thread(&self, list: &mut Vec<Thread>, t: Thread, chars: &[u32], pos: usize) {
        // Iterative closure with an explicit stack of (pc, slots, start).
        // Priority = insertion order; a visited pc within this insertion
        // chain prevents epsilon-loop infinite recursion.
        fn closure(
            nfa: &PosixNfa,
            list: &mut Vec<Thread>,
            mut pc: usize,
            slots: Vec<Option<usize>>,
            start: usize,
            chars: &[u32],
            pos: usize,
            visited: &mut Vec<usize>,
        ) {
            loop {
                if visited.contains(&pc) {
                    return;
                }
                visited.push(pc);
                match &nfa.prog[pc] {
                    Inst::Jmp(target) => {
                        pc = *target;
                    }
                    Inst::Split(a, b) => {
                        // First arm has priority: recurse into it before
                        // continuing with the second so insertion order
                        // reflects priority.
                        closure(nfa, list, *a, slots.clone(), start, chars, pos, visited);
                        pc = *b;
                    }
                    Inst::Save(slot) => {
                        let mut slots = slots;
                        if *slot < slots.len() {
                            slots[*slot] = Some(pos);
                        }
                        // chained saves: continue in the same thread with
                        // updated slots
                        return closure_continue(nfa, list, pc + 1, slots, start, chars, pos, visited);
                    }
                    Inst::AssertTextStart => {
                        if pos == 0 {
                            pc += 1;
                        } else {
                            return;
                        }
                    }
                    Inst::AssertTextEnd => {
                        if pos == chars.len() {
                            pc += 1;
                        } else {
                            return;
                        }
                    }
                    Inst::AssertLineStart => {
                        if pos == 0 || chars[pos - 1] == b'\n' as u32 {
                            pc += 1;
                        } else {
                            return;
                        }
                    }
                    Inst::AssertLineEnd => {
                        if pos == chars.len() || chars[pos] == b'\n' as u32 {
                            pc += 1;
                        } else {
                            return;
                        }
                    }
                    Inst::Consume(_) | Inst::ConsumeAny | Inst::Match => {
                        // Merge rule: first insertion of this pc wins.
                        if !list.iter().any(|existing| existing.pc == pc) {
                            list.push(Thread { pc, slots, start });
                        }
                        return;
                    }
                }
            }
        }
        // Tail-continuation helper so Save updates propagate down the chain.
        fn closure_continue(
            nfa: &PosixNfa,
            list: &mut Vec<Thread>,
            pc: usize,
            slots: Vec<Option<usize>>,
            start: usize,
            chars: &[u32],
            pos: usize,
            visited: &mut Vec<usize>,
        ) {
            closure(nfa, list, pc, slots, start, chars, pos, visited)
        }
        let mut visited = Vec::new();
        closure(self, list, t.pc, t.slots, t.start, chars, pos, &mut visited);
    }
}

impl PartialEq for Inst {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
impl Eq for Inst {}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<u32> {
        s.chars().map(|c| c as u32).collect()
    }

    fn caps(pat: &str, text: &str) -> Option<Vec<Option<usize>>> {
        let nfa = compile(pat, false, false).unwrap();
        nfa.captures_chars(&chars(text), 0)
    }

    #[test]
    fn leftmost_longest_alternation() {
        let c = caps("(a|ab)", "xab").unwrap();
        assert_eq!(c[0], Some(1));
        assert_eq!(c[1], Some(3)); // POSIX longest: "ab" not "a"
    }

    #[test]
    fn greedy_plus_longest() {
        let c = caps("a+", "aaab").unwrap();
        assert_eq!(c[1], Some(3));
    }

    #[test]
    fn lazy_is_longest_anyway() {
        // POSIX: lazy preference cannot shorten the overall match.
        let c = caps("((a|b)+?)((a|b)+?)b", "aaabbb").unwrap();
        assert_eq!(c[1], Some(6));
    }

    #[test]
    fn no_match() {
        assert!(caps("z+", "aaa").is_none());
    }

    #[test]
    fn line_anchors() {
        let c = caps("^b", "a\nb").unwrap();
        assert_eq!(c[0], Some(2));
    }

    #[test]
    fn capture_positions() {
        let c = caps("(a)(b+)", "xabbb").unwrap();
        assert_eq!(c[2], Some(1));
        assert_eq!(c[3], Some(2));
        assert_eq!(c[4], Some(2));
        assert_eq!(c[5], Some(5));
    }
}
