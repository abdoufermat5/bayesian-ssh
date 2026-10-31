//! Matching paths and `deny_paths` entries, literal or glob on either side.

use super::glob::{GlobMode, Tok, glob_match, normalize_path, tokens};

pub(super) fn path_matches_any(patterns: &[String], path: &str) -> Option<String> {
    let norm = normalize_path(path);
    patterns
        .iter()
        .find(|p| glob_match(p, &norm, GlobMode::Path))
        .cloned()
}

/// Does this text contain characters the shell expands as a glob?
pub(super) fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// One path-like piece of a call.
pub(super) struct Candidate {
    /// As written, glob characters kept.
    pub(super) text: String,
    /// Whether the shell expands this before the command runs.
    pub(super) expansion: Expansion,
}

/// What the shell does with an operand before the command sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Expansion {
    /// Used as written.
    Literal,
    /// Expanded to several paths.
    Glob,
}

impl Expansion {
    pub(super) fn of_text(text: &str) -> Self {
        if has_glob(text) {
            Expansion::Glob
        } else {
            Expansion::Literal
        }
    }
}

/// A `deny_paths` entry can hit a path three ways: the entry and the text are both literal
/// (`/etc/shadow`), a glob argument certainly expands to a literal entry (`/etc/sha*`), or
/// only some expansion could match an entry that is itself a glob (`/etc/*` against
/// `/etc/ssh/ssh_host_*_key`). The last case is not provable, so the caller asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PathHit {
    Deny(String),
    Unsure(String),
}

/// Match one path-like operand against `deny_paths`, keeping glob characters as written.
pub(super) fn deny_path_hit(patterns: &[String], candidate: &Candidate) -> Option<PathHit> {
    if candidate.expansion == Expansion::Literal {
        return path_matches_any(patterns, &candidate.text).map(PathHit::Deny);
    }
    let norm = normalize_path(&candidate.text);
    let mut unsure = None;
    for p in patterns {
        if has_glob(p) {
            if glob_could_match(p, &norm) {
                unsure = Some(p.clone());
            }
        } else if glob_match(&norm, p, GlobMode::Path) {
            return Some(PathHit::Deny(p.clone()));
        }
    }
    unsure.map(PathHit::Unsure)
}

/// Could the shell-expanded `glob` be a path that `pattern` matches? Both may hold glob
/// characters. Conservative where the expansion is not decidable: `true` means "could be".
fn glob_could_match(pattern: &str, glob: &str) -> bool {
    if !has_glob(glob) {
        return glob_match(pattern, glob, GlobMode::Path);
    }
    if !has_glob(pattern) {
        return glob_match(glob, pattern, GlobMode::Path);
    }
    // Compare path segments, letting `**` stand for any run of them. `*`, `?` and classes
    // stop at `/`, so one segment only ever meets one segment.
    let p: Vec<&str> = pattern.split('/').collect();
    let g: Vec<&str> = glob.split('/').collect();
    let mut dp = vec![vec![false; g.len() + 1]; p.len() + 1];
    dp[p.len()][g.len()] = true;
    for i in (0..p.len()).rev() {
        // Glob exhausted: only trailing `**` segments can still match nothing.
        dp[i][g.len()] = p[i] == "**" && dp[i + 1][g.len()];
        for j in (0..g.len()).rev() {
            dp[i][j] = if p[i] == "**" {
                dp[i + 1][j] || dp[i][j + 1]
            } else {
                segments_overlap(p[i], g[j]) && dp[i + 1][j + 1]
            };
        }
    }
    dp[0][0]
}

/// What one path segment glob can and cannot vary.
struct Seg {
    /// Literal start, up to the first wildcard.
    pre: String,
    /// Literal end, after the last wildcard.
    suf: String,
    /// Length in characters when the segment has no `*`; minimum length otherwise.
    len: Option<usize>,
    /// Token count, the minimum length.
    min: usize,
    /// `*…*`: no fixed start and no fixed end, so it matches anything.
    any: bool,
}

impl Seg {
    fn new(seg: &str) -> Self {
        let toks = tokens(seg, GlobMode::Path);
        let pre: String = toks
            .iter()
            .map_while(|t| match t {
                Tok::Lit(c) => Some(*c),
                _ => None,
            })
            .collect();
        let suf: String = toks
            .iter()
            .rev()
            .map_while(|t| match t {
                Tok::Lit(c) => Some(*c),
                _ => None,
            })
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        let star = toks
            .iter()
            .any(|t| matches!(t, Tok::Star | Tok::DoubleStar));
        let min = toks
            .iter()
            .filter(|t| !matches!(t, Tok::Star | Tok::DoubleStar))
            .count();
        Seg {
            any: pre.is_empty() && suf.is_empty() && star,
            pre,
            suf,
            len: (!star).then_some(min),
            min,
        }
    }
}

/// Can two single-path-segment globs match the same text? Conservative: `true` when the
/// fixed starts, fixed ends and lengths don't rule it out.
fn segments_overlap(a: &str, b: &str) -> bool {
    let a = Seg::new(a);
    let b = Seg::new(b);
    if a.any || b.any {
        return true;
    }
    if !(a.pre.starts_with(&b.pre) || b.pre.starts_with(&a.pre)) {
        return false;
    }
    if !(a.suf.ends_with(&b.suf) || b.suf.ends_with(&a.suf)) {
        return false;
    }
    match (a.len, b.len) {
        (Some(x), Some(y)) => x == y,
        (Some(x), None) => x >= b.min,
        (None, Some(y)) => y >= a.min,
        (None, None) => true,
    }
}
