#![forbid(unsafe_code)]
//! GNU's Unicode matching mode and Unicode character classes are independent.
//! PCRE2 10.49 semantics are pinned by complete scalar oracle fixtures.
use super::{RegexWorkspace, WorkspaceBuffer, WorkspaceText};
mod charset;
mod classes;
mod flags;
mod lexical;
#[cfg(test)]
mod owned_tests;
mod tables;
use charset::{Kind, boundary, set};
use classes::class;
use flags::{Flags, flags_text, parse_flags};
use lexical::{letter_property, quote_end, quoted, scalar_end};

fn walk(
    source: &str,
    workspace: &RegexWorkspace,
    caseless: bool,
    mut emit: impl FnMut(&str),
) -> Result<(), &'static str> {
    let mut flags = Flags::new(caseless);
    let mut stack: WorkspaceBuffer<Flags> = workspace.buffer(0);
    let mut depth = 0;
    let mut at = 0;
    let mut ucp = false;
    loop {
        if source[at..].starts_with("(*UCP)") {
            ucp = true;
            at += 6;
        } else if source[at..].starts_with("(*UTF)") {
            at += 6;
        } else {
            break;
        }
    }
    while at < source.len() {
        let bytes = source.as_bytes();
        if flags.extended() && bytes[at] == b'#' {
            let end = source[at..].find('\n').map_or(source.len(), |n| at + n);
            emit(&source[at..end]);
            at = end;
            continue;
        }
        if source[at..].starts_with("(?#") {
            let end = source[at + 3..]
                .find(')')
                .ok_or("unterminated pattern comment")?
                + at
                + 4;
            emit(&source[at..end]);
            at = end;
            continue;
        }
        if let Some((next, scoped, updated)) = parse_flags(source, at, flags) {
            if scoped {
                if depth == stack.len() {
                    stack.push(flags);
                } else {
                    stack[depth] = flags;
                }
                depth += 1;
            }
            flags = updated;
            flags_text(flags, scoped, &mut emit);
            at = next;
            continue;
        }
        match bytes[at] {
            b'\\' => {
                let Some(&escape) = bytes.get(at + 1) else {
                    return Err("trailing pattern escape");
                };
                if let Some((end, negative)) = letter_property(source, at) {
                    set(Kind::Alpha, negative, true, flags, &mut emit);
                    at = end;
                    continue;
                }
                if escape == b'Q' {
                    let (end, next) = quote_end(source, at + 2);
                    if end > at + 2 {
                        emit("(?-x:");
                        quoted(&source[at + 2..end], false, &mut emit);
                        emit(")");
                    }
                    at = next;
                    continue;
                }
                if let Some((kind, negative)) = Kind::escape(escape) {
                    set(
                        kind,
                        negative,
                        flags.unicode(ucp, kind, false),
                        flags,
                        &mut emit,
                    );
                } else if escape == b'b' || escape == b'B' {
                    boundary(escape == b'B', ucp, flags, &mut emit);
                } else if escape != b'E' {
                    emit(&source[at..scalar_end(source, at + 1)]);
                }
                at = scalar_end(source, at + 1);
            }
            b'[' => at = class(source, at, ucp, flags, workspace, &mut emit)?,
            b'(' => {
                if depth == stack.len() {
                    stack.push(flags);
                } else {
                    stack[depth] = flags;
                }
                depth += 1;
                emit("(");
                at += 1;
            }
            b')' => {
                if depth != 0 {
                    depth -= 1;
                    flags = stack[depth];
                }
                emit(")");
                at += 1;
            }
            b'$' if !flags.multiline() => {
                emit(r"(?=\n?\z)");
                at += 1;
            }
            _ => {
                let end = scalar_end(source, at);
                emit(&source[at..end]);
                at = end;
            }
        }
    }
    Ok(())
}

pub(super) fn prepare(
    source: &str,
    workspace: &RegexWorkspace,
    caseless: bool,
) -> Result<WorkspaceText, &'static str> {
    let mut length = 0usize;
    walk(source, workspace, caseless, |text| {
        length = length
            .checked_add(text.len())
            .unwrap_or_else(|| super::workspace::memory_error());
    })?;
    let mut output = workspace.text(length);
    walk(source, workspace, caseless, |text| output.push_str(text))?;
    output.finish().ok_or("invalid UTF8 dialect output")
}
