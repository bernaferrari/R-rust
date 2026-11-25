#![forbid(unsafe_code)]
//! POSIX ERE → fancy-regex syntax translation.
//!
//! TRE implements IEEE POSIX 1003.2 ERE semantics with backreference
//! extensions; fancy-regex implements Perl-ish syntax over Rust's regex
//! crate. The grammars overlap heavily, so translation is a targeted scan:
//!
//! - `\X` where X is a PCRE-only class/anchor letter (d, w, s, b, ...) is a
//!   LITERAL in POSIX — the backslash is dropped and the byte emitted bare.
//!   POSIX control escapes `\n \t \r \f \a` are spelled identically in both
//!   dialects and pass through. `\1`..`\9` are backreferences in both.
//! - Inside bracket expressions POSIX treats backslash as a literal
//!   character, while the regex crate reads `\d` & friends inside brackets
//!   as classes — backslashes inside brackets are re-escaped.
//! - `^` is an anchor in POSIX only at the start of the pattern, after `|`,
//!   or after `(`; elsewhere it is a literal. `$` is an anchor only at the
//!   end of the pattern, before `|`, or before `)`. Positional occurrences
//!   are escaped into literals.
//!
//! Output is assembled as raw bytes: pattern input is UTF-8 and every
//! syntax-significant byte is ASCII, so non-ASCII bytes copy verbatim.

/// Translate `pattern` from POSIX ERE syntax into fancy-regex syntax.
pub fn prepare(pattern: &str) -> Result<String, &'static str> {
    let bytes = pattern.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(pattern.len() + 8);
    let mut i = 0usize;
    let mut bracket = false;
    let mut bracket_start = 0usize;
    // True when the next atom begins the pattern, a group, or an alternation
    // branch — the positions where POSIX treats `^` as an anchor.
    let mut branch_start = true;
    while i < bytes.len() {
        let b = bytes[i];
        if bracket {
            // POSIX classes `[[:name:]]` pass through as units after name
            // validation; the regex crate rejects malformed ones with worse
            // diagnostics than a direct error.
            if b == b'[' && bytes.get(i + 1) == Some(&b':') {
                let mut j = i + 2;
                while j + 1 < bytes.len() && !(bytes[j] == b':' && bytes[j + 1] == b']') {
                    j += 1;
                }
                if j + 1 >= bytes.len() {
                    return Err("unterminated POSIX character class");
                }
                let name = &pattern[i + 2..j];
                const POSIX_CLASSES: &[&str] = &[
                    "alpha", "digit", "alnum", "upper", "lower", "space", "punct", "print",
                    "graph", "cntrl", "xdigit", "blank", "ascii",
                ];
                if !POSIX_CLASSES.contains(&name) {
                    return Err("invalid POSIX character class name");
                }
                out.extend_from_slice(&bytes[i..=j + 1]);
                i = j + 2;
                continue;
            }
            match b {
                b']' => {
                    // Leading `]`/`[^]` literals are consumed verbatim when the
                    // class opens, so any `]` seen inside the scan closes it.
                    out.push(b']');
                    bracket = false;
                    i += 1;
                }
                b'\\' => {
                    // POSIX bracket: literal backslash; re-escape for the
                    // regex crate so it is not read as a class escape.
                    out.extend_from_slice(b"\\\\");
                    i += 1;
                }
                _ => {
                    out.push(b);
                    i += 1;
                }
            }
            continue;
        }
        match b {
            b'[' => {
                bracket = true;
                bracket_start = i;
                out.push(b'[');
                i += 1;
                // POSIX leading `^` negation and leading-`]` literal copy
                // through verbatim so the scan does not misclose.
                if i < bytes.len() && bytes[i] == b'^' {
                    out.push(b'^');
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b']' {
                    out.push(b']');
                    i += 1;
                }
                branch_start = false;
            }
            b'\\' => {
                let Some(&esc) = bytes.get(i + 1) else {
                    return Err("trailing backslash in pattern");
                };
                match esc {
                    b'1'..=b'9' => {
                        // Backreference: identical in both dialects.
                        out.push(b'\\');
                        out.push(esc);
                    }
                    b'n' | b't' | b'r' | b'f' | b'a' => {
                        // Control escapes TRE honors; same spelling downstream.
                        out.push(b'\\');
                        out.push(esc);
                    }
                    b'.' | b'^' | b'$' | b'|' | b'?' | b'*' | b'+' | b'(' | b')' | b'[' | b']'
                    | b'{' | b'}' | b'\\' => {
                        // Escaped regex-active literal: identical in both.
                        out.push(b'\\');
                        out.push(esc);
                    }
                    other => {
                        // PCRE-only class/anchor letters (\d \w \s \b ...) are
                        // ordinary literals in POSIX: drop the backslash.
                        if other.is_ascii_alphabetic()
                            || other.is_ascii_digit()
                            || other == b'_'
                            || other >= 0x80
                        {
                            out.push(other);
                        } else {
                            out.push(b'\\');
                            out.push(other);
                        }
                    }
                }
                i += 2;
                branch_start = false;
            }
            b'^' => {
                if branch_start {
                    out.push(b'^');
                } else {
                    out.extend_from_slice(b"\\^");
                }
                i += 1;
            }
            b'$' => {
                // Anchor only at pattern end, before `|`, or before `)`.
                let next = bytes.get(i + 1).copied();
                if next.is_none() || next == Some(b'|') || next == Some(b')') {
                    out.push(b'$');
                } else {
                    out.extend_from_slice(b"\\$");
                }
                i += 1;
                branch_start = false;
            }
            b'{' => {
                // Validate `{m}`, `{m,}`, `{m,n}` syntax and bounds; POSIX
                // rejects reversed ranges, which the regex crate tolerates.
                let mut j = i + 1;
                let mut digits1 = 0usize;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                    digits1 += 1;
                }
                let valid_open = digits1 > 0;
                if valid_open {
                    let mut k = j;
                    let mut has_comma = false;
                    let mut digits2 = 0usize;
                    if k < bytes.len() && bytes[k] == b',' {
                        has_comma = true;
                        k += 1;
                        while k < bytes.len() && bytes[k].is_ascii_digit() {
                            k += 1;
                            digits2 += 1;
                        }
                    }
                    if k < bytes.len() && bytes[k] == b'}' {
                        let lo: u64 = pattern[i + 1..j].parse().unwrap_or(u64::MAX);
                        let hi: Option<u64> = if has_comma && digits2 > 0 {
                            Some(pattern[j + 1..k].parse().unwrap_or(u64::MAX))
                        } else if has_comma {
                            None
                        } else {
                            Some(lo)
                        };
                        if let Some(hi) = hi {
                            if hi < lo {
                                return Err("invalid repetition range: minimum exceeds maximum");
                            }
                        }
                        out.extend_from_slice(&bytes[i..=k]);
                        i = k + 1;
                        branch_start = false;
                        continue;
                    }
                }
                // Not a valid interval: POSIX treats `{` as a literal here.
                out.extend_from_slice(b"\\{");
                i += 1;
                branch_start = false;
            }
            b'|' | b'(' => {
                out.push(b);
                i += 1;
                branch_start = true;
            }
            _ => {
                out.push(b);
                i += 1;
                branch_start = false;
            }
        }
    }
    if bracket {
        return Err("unterminated bracket expression");
    }
    String::from_utf8(out).map_err(|_| "pattern translation corrupted UTF-8")
}

#[cfg(test)]
mod tests {
    use super::prepare;

    #[test]
    fn pcre_classes_become_literals() {
        assert_eq!(prepare("\\d").unwrap(), "d");
        assert_eq!(prepare("\\w+").unwrap(), "w+");
        assert_eq!(prepare("a\\bz").unwrap(), "abz");
    }

    #[test]
    fn backrefs_pass_through() {
        assert_eq!(prepare("(a)\\1").unwrap(), "(a)\\1");
    }

    #[test]
    fn positional_caret_dollar_are_literal() {
        assert_eq!(prepare("a^b").unwrap(), "a\\^b");
        assert_eq!(prepare("^ab").unwrap(), "^ab");
        assert_eq!(prepare("a$b").unwrap(), "a\\$b");
        assert_eq!(prepare("ab$").unwrap(), "ab$");
        assert_eq!(prepare("(a|^b)").unwrap(), "(a|^b)");
        assert_eq!(prepare("x$|y$").unwrap(), "x$|y$");
    }

    #[test]
    fn bracket_backslash_is_literal() {
        assert_eq!(prepare("[\\d]").unwrap(), "[\\\\d]");
        assert_eq!(prepare("[]a]").unwrap(), "[]a]");
    }

    #[test]
    fn utf8_bytes_pass_verbatim() {
        assert_eq!(prepare("é+").unwrap(), "é+");
        assert_eq!(prepare("^héllo$").unwrap(), "^héllo$");
    }
}

/// Scan a translated pattern for backreference escapes (`\1`..`\9`).
/// Translation preserves them verbatim, so a plain scan is exact.
pub fn has_backrefs(prepared: &str) -> bool {
    let bytes = prepared.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            if bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0' {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}
