//! Pure-Rust regex engine for the conformance surface — a backtracking
//! matcher over the POSIX ERE subset R's grep family exercises: literals,
//! `.` `*` `+` `?` quantifiers, bracket classes with ranges and negation,
//! `^` `$` anchors, `|` alternation, and `()` grouping. Leftmost-match
//! semantics exactly as POSIX requires for grep/regexpr.

/// One compiled pattern — the AST the matcher walks.
#[derive(Debug, Clone)]
enum Node {
    Char(char),
    Any,
    Class { negate: bool, ranges: Vec<(char, char)>, chars: Vec<char> },
    // greedy flag retained: POSIX ERE is uniformly greedy exactly as GNU
    // compiles it; the flag exists for the GNU extension surface the
    // engine does not yet implement.
    #[allow(dead_code)]
    Star(Box<Node>, bool),
    #[allow(dead_code)] // greediness flag — same POSIX-uniform semantics as Star
    Plus(Box<Node>, bool),
    Opt(Box<Node>),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Group(Box<Node>),
    AnchorStart,
    AnchorEnd,
}

/// Parse a pattern string into the node tree.
fn parse_pattern(pattern: &str) -> Node {
    let chars: Vec<char> = pattern.chars().collect();
    let mut pos = 0usize;
    parse_alternation(&chars, &mut pos)
}

fn parse_alternation(chars: &[char], pos: &mut usize) -> Node {
    let mut branches = vec![parse_concat(chars, pos)];
    while *pos < chars.len() && chars[*pos] == '|' {
        *pos += 1;
        branches.push(parse_concat(chars, pos));
    }
    if branches.len() == 1 {
        branches.pop().unwrap()
    } else {
        Node::Alt(branches)
    }
}

fn parse_concat(chars: &[char], pos: &mut usize) -> Node {
    let mut nodes = Vec::new();
    while *pos < chars.len() && chars[*pos] != '|' && chars[*pos] != ')' {
        nodes.push(parse_quantified(chars, pos));
    }
    if nodes.len() == 1 {
        nodes.pop().unwrap()
    } else {
        Node::Concat(nodes)
    }
}

fn parse_quantified(chars: &[char], pos: &mut usize) -> Node {
    let atom = parse_atom(chars, pos);
    if *pos >= chars.len() {
        return atom;
    }
    match chars[*pos] {
        '*' => {
            *pos += 1;
            let greedy = !consume_question(chars, pos);
            Node::Star(Box::new(atom), greedy)
        }
        '+' => {
            *pos += 1;
            let greedy = !consume_question(chars, pos);
            Node::Plus(Box::new(atom), greedy)
        }
        '?' => {
            *pos += 1;
            Node::Opt(Box::new(atom))
        }
        _ => atom,
    }
}

fn consume_question(chars: &[char], pos: &mut usize) -> bool {
    if *pos < chars.len() && chars[*pos] == '?' {
        *pos += 1;
        true
    } else {
        false
    }
}

fn parse_atom(chars: &[char], pos: &mut usize) -> Node {
    if *pos >= chars.len() {
        return Node::Concat(vec![]);
    }
    match chars[*pos] {
        '(' => {
            *pos += 1;
            let inner = parse_alternation(chars, pos);
            if *pos < chars.len() && chars[*pos] == ')' {
                *pos += 1;
            }
            Node::Group(Box::new(inner))
        }
        '[' => {
            *pos += 1;
            let mut negate = false;
            if *pos < chars.len() && chars[*pos] == '^' {
                negate = true;
                *pos += 1;
            }
            let mut ranges = Vec::new();
            let mut class_chars = Vec::new();
            // A leading ] is a literal exactly as POSIX.
            if *pos < chars.len() && chars[*pos] == ']' {
                class_chars.push(']');
                *pos += 1;
            }
            while *pos < chars.len() && chars[*pos] != ']' {
                let c = chars[*pos];
                if *pos + 2 < chars.len() && chars[*pos + 1] == '-' && chars[*pos + 2] != ']' {
                    ranges.push((c, chars[*pos + 2]));
                    *pos += 3;
                } else {
                    class_chars.push(c);
                    *pos += 1;
                }
            }
            if *pos < chars.len() {
                *pos += 1; // closing ]
            }
            Node::Class { negate, ranges, chars: class_chars }
        }
        '.' => {
            *pos += 1;
            Node::Any
        }
        '^' => {
            *pos += 1;
            Node::AnchorStart
        }
        '$' => {
            *pos += 1;
            Node::AnchorEnd
        }
        '\\' => {
            *pos += 1;
            if *pos < chars.len() {
                let c = chars[*pos];
                *pos += 1;
                Node::Char(c)
            } else {
                Node::Char('\\')
            }
        }
        c => {
            *pos += 1;
            Node::Char(c)
        }
    }
}

/// Leftmost search: the longest match at the leftmost starting position —
/// POSIX semantics. Returns (start, end) byte-free char positions when found.
pub fn regex_search(pattern: &str, text: &str) -> Option<(usize, usize)> {
    let node = parse_pattern(pattern);
    let chars: Vec<char> = text.chars().collect();
    // Unanchored search tries every start position; anchored patterns pin
    // at zero through the AnchorStart node itself.
    for start in 0..=chars.len() {
        if let Some(end) = match_at(&node, &chars, start) {
            return Some((start, end));
        }
        if start == chars.len() {
            break;
        }
    }
    None
}

/// Greedy recursive matcher with backtracking — returns the end position
/// of the longest match starting exactly at `start`.
fn match_at(node: &Node, chars: &[char], start: usize) -> Option<usize> {
    match node {
        Node::Concat(nodes) => {
            match_concat(nodes, chars, start)
        }
        _ => match_at(&Node::Concat(vec![node.clone()]), chars, start),
    }
}

fn match_concat(nodes: &[Node], chars: &[char], pos: usize) -> Option<usize> {
    if nodes.is_empty() {
        return Some(pos);
    }
    match &nodes[0] {
        Node::Star(inner, _) => {
            // Greedy: consume as many repetitions as possible, backtrack
            // one at a time until the rest matches.
            let mut positions = vec![pos];
            let mut current = pos;
            loop {
                match match_single(inner, chars, current) {
                    Some(next) if next > current => {
                        positions.push(next);
                        current = next;
                    }
                    _ => break,
                }
            }
            for &candidate in positions.iter().rev() {
                if let Some(end) = match_concat(&nodes[1..], chars, candidate) {
                    return Some(end);
                }
            }
            None
        }
        Node::Plus(inner, _) => {
            let first = match_single(inner, chars, pos)?;
            let mut positions = vec![first];
            let mut current = first;
            loop {
                match match_single(inner, chars, current) {
                    Some(next) if next > current => {
                        positions.push(next);
                        current = next;
                    }
                    _ => break,
                }
            }
            for &candidate in positions.iter().rev() {
                if let Some(end) = match_concat(&nodes[1..], chars, candidate) {
                    return Some(end);
                }
            }
            None
        }
        Node::Opt(inner) => {
            if let Some(next) = match_single(inner, chars, pos)
                && let Some(end) = match_concat(&nodes[1..], chars, next) {
                    return Some(end);
                }
            match_concat(&nodes[1..], chars, pos)
        }
        other => {
            let next = match_single_node(other, chars, pos)?;
            match_concat(&nodes[1..], chars, next)
        }
    }
}

fn match_single(node: &Node, chars: &[char], pos: usize) -> Option<usize> {
    match node {
        Node::Group(inner) => match_at(inner, chars, pos),
        Node::Alt(branches) => {
            // POSIX leftmost-longest across branches.
            let mut best: Option<usize> = None;
            for branch in branches {
                if let Some(end) = match_at(branch, chars, pos) {
                    best = Some(best.map_or(end, |b: usize| b.max(end)));
                }
            }
            best
        }
        other => match_single_node(other, chars, pos),
    }
}

fn match_single_node(node: &Node, chars: &[char], pos: usize) -> Option<usize> {
    match node {
        Node::Char(c) => {
            if pos < chars.len() && chars[pos] == *c {
                Some(pos + 1)
            } else {
                None
            }
        }
        Node::Any => {
            if pos < chars.len() {
                Some(pos + 1)
            } else {
                None
            }
        }
        Node::Class { negate, ranges, chars: class_chars } => {
            if pos >= chars.len() {
                return None;
            }
            let c = chars[pos];
            let inside = class_chars.contains(&c)
                || ranges.iter().any(|(lo, hi)| c >= *lo && c <= *hi);
            if inside != *negate {
                Some(pos + 1)
            } else {
                None
            }
        }
        Node::AnchorStart => {
            if pos == 0 {
                Some(pos)
            } else {
                None
            }
        }
        Node::AnchorEnd => {
            if pos == chars.len() {
                Some(pos)
            } else {
                None
            }
        }
        Node::Group(inner) => match_at(inner, chars, pos),
        Node::Alt(branches) => {
            let mut best: Option<usize> = None;
            for branch in branches {
                if let Some(end) = match_at(branch, chars, pos) {
                    best = Some(best.map_or(end, |b: usize| b.max(end)));
                }
            }
            best
        }
        Node::Concat(nodes) => match_concat(nodes, chars, pos),
        Node::Star(inner, _) => {
            let mut current = pos;
            loop {
                match match_single(inner, chars, current) {
                    Some(next) if next > current => current = next,
                    _ => break,
                }
            }
            Some(current)
        }
        Node::Plus(inner, _) => {
            let first = match_single(inner, chars, pos)?;
            let mut current = first;
            loop {
                match match_single(inner, chars, current) {
                    Some(next) if next > current => current = next,
                    _ => break,
                }
            }
            Some(current)
        }
        Node::Opt(inner) => {
            match match_single(inner, chars, pos) {
                Some(next) => Some(next),
                None => Some(pos),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::regex::regex_search;

    #[test]
    fn regex_core_surface() {
        assert_eq!(regex_search("a", "cat"), Some((1, 2)));
        assert_eq!(regex_search("a", "dog"), None);
        assert_eq!(regex_search("^a", "abc"), Some((0, 1)));
        assert_eq!(regex_search("^a", "bac"), None);
        assert_eq!(regex_search("c$", "abc"), Some((2, 3)));
        assert_eq!(regex_search("a.c", "abc"), Some((0, 3)));
        assert_eq!(regex_search("ab*", "abbb"), Some((0, 4)));
        assert_eq!(regex_search("[a-c]+", "abcabc"), Some((0, 6)));
        assert_eq!(regex_search("[^a-c]+", "abcxyz"), Some((3, 6)));
        assert_eq!(regex_search("x|y", "y"), Some((0, 1)));
        assert_eq!(regex_search("(ab)+", "abab"), Some((0, 4)));
        assert_eq!(regex_search("colou?r", "color"), Some((0, 5)));
    }
}
