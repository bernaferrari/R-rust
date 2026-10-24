#![forbid(unsafe_code)]
use super::{
    RegexWorkspace,
    charset::{Kind, set},
    flags::Flags,
    lexical::{letter_property, quote_end, quoted, scalar_end},
};

fn literal_class(source: &str, quoted_text: bool, more: bool, emit: &mut impl FnMut(&str)) {
    if source.is_empty() {
        return;
    }
    // PCRE's x mode keeps spaces in ordinary brackets.
    emit("(?-x:[");
    if quoted_text {
        quoted(source, true, emit);
    } else {
        let mut at = 0;
        while at < source.len() {
            if more && matches!(source.as_bytes()[at], b' ' | b'\t') {
                at += 1;
                continue;
            }
            let end = scalar_end(source, at);
            if source.as_bytes()[at] == b'\\' && end < source.len() {
                let escaped_end = scalar_end(source, end);
                if source.as_bytes()[end] == b'b' {
                    emit(r"\x08");
                } else {
                    emit(&source[at..escaped_end]);
                }
                at = escaped_end;
            } else {
                if matches!(source.as_bytes()[at], b'[' | b']' | b'^' | b'&') {
                    emit(r"\");
                }
                emit(&source[at..end]);
                at = end;
            }
        }
    }
    emit("])");
}

#[derive(Clone, Copy)]
enum Part {
    Literal(usize, usize),
    Quoted(usize, usize),
    Set(Kind, bool, bool),
}
fn skip_space(source: &str, mut at: usize, end: usize, more: bool) -> usize {
    while more && at < end && matches!(source.as_bytes()[at], b' ' | b'\t') {
        at += 1;
    }
    at
}
fn class_start(source: &str, opening: usize, more: bool) -> (usize, bool) {
    let at = skip_space(source, opening + 1, source.len(), more);
    let negative = source.as_bytes().get(at) == Some(&b'^');
    (
        skip_space(source, at + usize::from(negative), source.len(), more),
        negative,
    )
}
fn class_end(source: &str, opening: usize, more: bool) -> Option<usize> {
    let bytes = source.as_bytes();
    let (mut at, _) = class_start(source, opening, more);
    if bytes.get(at) == Some(&b']') {
        at += 1;
    }
    while at < bytes.len() {
        if bytes[at] == b'\\' {
            if at + 1 == source.len() {
                return None;
            }
            if bytes.get(at + 1) == Some(&b'Q') {
                at = quote_end(source, at + 2).1;
            } else {
                at = scalar_end(source, at + 1);
            }
        } else if source[at..].starts_with("[:") {
            at += source[at + 2..].find(":]")? + 4;
        } else if bytes[at] == b']' {
            return Some(at);
        } else {
            at = scalar_end(source, at);
        }
    }
    None
}
fn escaped(source: &str, at: usize) -> bool {
    source.as_bytes()[..at]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\\')
        .count()
        % 2
        != 0
}
pub(super) fn class(
    source: &str,
    opening: usize,
    ucp: bool,
    flags: Flags,
    workspace: &RegexWorkspace,
    emit: &mut impl FnMut(&str),
) -> Result<usize, &'static str> {
    let more = flags.extended_more();
    let end = class_end(source, opening, more).ok_or("unterminated character class")?;
    let (start, negative) = class_start(source, opening, more);
    let mut parts = workspace.buffer(0);
    let mut run = start;
    let mut at = start;
    while at < end {
        let mut special = None;
        let mut explicit_unicode = false;
        let mut next = scalar_end(source, at);
        if source.as_bytes()[at] == b'\\' {
            if let Some(&b) = source.as_bytes().get(at + 1) {
                next = scalar_end(source, at + 1);
                if b == b'Q' {
                    let (qend, after) = quote_end(source, at + 2);
                    if qend > end {
                        return Err("unterminated quote in character class");
                    }
                    if run < at {
                        parts.push(Part::Literal(run, at));
                    }
                    if qend > at + 2 {
                        parts.push(Part::Quoted(at + 2, qend));
                    }
                    at = after;
                    run = at;
                    continue;
                }
                if let Some((end, negative)) = letter_property(source, at) {
                    special = Some((Kind::Alpha, negative));
                    explicit_unicode = true;
                    next = end;
                } else {
                    special = Kind::escape(b);
                }
            }
        } else if source[at..].starts_with("[:") {
            let offset = source[at + 2..end]
                .find(":]")
                .ok_or("invalid POSIX character class")?;
            let name = &source[at + 2..at + 2 + offset];
            let (name, neg) = name
                .strip_prefix('^')
                .map_or((name, false), |name| (name, true));
            special = Some((
                Kind::posix(name).ok_or("invalid POSIX character class")?,
                neg,
            ));
            next = at + offset + 4;
        } else if source[at..].starts_with("[.") || source[at..].starts_with("[=") {
            return Err("unsupported collating character class");
        }
        if let Some((kind, neg)) = special {
            let mut before = at;
            while more && before > start && matches!(source.as_bytes()[before - 1], b' ' | b'\t') {
                before -= 1;
            }
            let after = skip_space(source, next, end, more);
            if (before > start + 1
                && source.as_bytes()[before - 1] == b'-'
                && !escaped(source, before - 1))
                || (source.as_bytes().get(after) == Some(&b'-')
                    && skip_space(source, after + 1, end, more) < end)
            {
                return Err("invalid character class range");
            }
            if run < at {
                parts.push(Part::Literal(run, at));
            }
            // POSIX restrictions and shorthand restrictions have separate scopes.
            let unicode =
                explicit_unicode || flags.unicode(ucp, kind, source.as_bytes()[at] != b'\\');
            parts.push(Part::Set(kind, neg, unicode));
            at = next;
            run = at;
        } else {
            at = next;
        }
    }
    if run < end {
        parts.push(Part::Literal(run, end));
    }
    if parts.is_empty() {
        return Err("empty character class");
    }
    if negative {
        emit("(?!");
    }
    emit("(?:");
    let mut count = 0;
    for part in parts.iter() {
        if matches!(part, Part::Literal(a,b) if more && source[*a..*b].chars().all(|c| matches!(c, ' ' | '\t')))
        {
            continue;
        }
        if count != 0 {
            emit("|");
        }
        count += 1;
        match *part {
            Part::Literal(a, b) => literal_class(&source[a..b], false, more, emit),
            Part::Quoted(a, b) => literal_class(&source[a..b], true, false, emit),
            Part::Set(kind, neg, unicode) => set(kind, neg, unicode, flags, emit),
        }
    }
    if count == 0 {
        return Err("empty character class");
    }
    emit(")");
    if negative {
        emit(")(?s:.)");
    }
    Ok(end + 1)
}
