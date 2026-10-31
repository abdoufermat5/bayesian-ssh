//! Glob matching and lexical path normalization.

/// How `*`, `?` and `[..]` treat `/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobMode {
    /// Plain text (hostnames, command lines): `*` crosses everything.
    Text,
    /// Paths: `*`, `?` and classes stop at `/`, `**` crosses it.
    Path,
}

impl GlobMode {
    fn is_path(self) -> bool {
        self == GlobMode::Path
    }
}

/// One glob token. `Star`/`DoubleStar` are both wide, so matching is a DP over the text
/// positions instead of backtracking: O(pattern × text) however many stars it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tok {
    Lit(char),
    /// `?`
    Any,
    /// `*`
    Star,
    /// `**` in path mode: any run, `/` included.
    DoubleStar,
    /// `**/` in path mode: any run ending in `/`, or nothing at all.
    DoubleStarSlash,
    /// `[abc]`, `[a-z]`, `[!abc]`; ranges are inclusive.
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

/// Tokenize a glob. An unclosed `[` is a literal `[`, as in the shell.
pub(super) fn tokens(pattern: &str, mode: GlobMode) -> Vec<Tok> {
    let path_mode = mode.is_path();
    let mut out = Vec::new();
    let mut it = pattern.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '*' if path_mode && it.peek() == Some(&'*') => {
                it.next();
                if it.peek() == Some(&'/') {
                    it.next();
                    out.push(Tok::DoubleStarSlash);
                } else {
                    out.push(Tok::DoubleStar);
                }
            }
            '*' => out.push(Tok::Star),
            '?' => out.push(Tok::Any),
            '[' => {
                // Scan on a clone: an unclosed `[` is a literal bracket, not a class.
                let mut probe = it.clone();
                match class(&mut probe) {
                    Some(tok) => {
                        it = probe;
                        out.push(tok);
                    }
                    None => out.push(Tok::Lit('[')),
                }
            }
            c => out.push(Tok::Lit(c)),
        }
    }
    out
}

/// Parse a `[...]` body; `None` when there is no closing `]`.
fn class(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<Tok> {
    let mut negated = false;
    if matches!(it.peek(), Some('!') | Some('^')) {
        negated = true;
        it.next();
    }
    let mut ranges = Vec::new();
    while let Some(&c) = it.peek() {
        if c == ']' && !ranges.is_empty() {
            it.next();
            return Some(Tok::Class { negated, ranges });
        }
        it.next();
        let mut hi = c;
        // `a-z`, but a `-` before the closing `]` is a literal member.
        if it.peek() == Some(&'-') {
            let mut ahead = it.clone();
            ahead.next();
            if ahead.peek().is_some_and(|h| *h != ']') {
                it.next();
                hi = it.next().unwrap_or(c);
            }
        }
        ranges.push((c, hi));
    }
    None
}

fn in_ranges(ranges: &[(char, char)], c: char) -> bool {
    ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi)
}

/// Glob match. `*` matches any run of characters, `?` one character, `[abc]`/`[a-z]`/
/// `[!abc]` one character from a set. With [`GlobMode::Path`], `*`, `?` and classes stop at `/`,
/// `**` crosses it, and `**/` also matches zero directories (`/etc/**/x` matches `/etc/x`).
pub(crate) fn glob_match(pattern: &str, text: &str, mode: GlobMode) -> bool {
    let path_mode = mode.is_path();
    let toks = tokens(pattern, mode);
    let t: Vec<char> = text.chars().collect();
    let mut cur = vec![false; t.len() + 1];
    cur[0] = true;
    for tok in &toks {
        let mut next = vec![false; t.len() + 1];
        match tok {
            Tok::Lit(c) => {
                for j in 1..=t.len() {
                    next[j] = cur[j - 1] && t[j - 1] == *c;
                }
            }
            Tok::Any => {
                for j in 1..=t.len() {
                    next[j] = cur[j - 1] && !(path_mode && t[j - 1] == '/');
                }
            }
            Tok::Class { negated, ranges } => {
                for j in 1..=t.len() {
                    next[j] = cur[j - 1]
                        && !(path_mode && t[j - 1] == '/')
                        && (in_ranges(ranges, t[j - 1]) != *negated);
                }
            }
            Tok::Star => {
                next[0] = cur[0];
                for j in 1..=t.len() {
                    let carry = !(path_mode && t[j - 1] == '/') && next[j - 1];
                    next[j] = cur[j] || carry;
                }
            }
            Tok::DoubleStar => {
                next[0] = cur[0];
                for j in 1..=t.len() {
                    next[j] = cur[j] || next[j - 1];
                }
            }
            Tok::DoubleStarSlash => {
                next[0] = cur[0];
                let mut any = cur[0];
                for j in 1..=t.len() {
                    next[j] = cur[j] || (t[j - 1] == '/' && any);
                    any |= cur[j];
                }
            }
        }
        cur = next;
    }
    cur[t.len()]
}

/// Lexically normalize an absolute path (`/a/b/../c` -> `/a/c`). Relative paths are
/// returned as is: they can't be judged against absolute patterns.
pub(crate) fn normalize_path(path: &str) -> String {
    if !path.starts_with('/') {
        return path.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}
