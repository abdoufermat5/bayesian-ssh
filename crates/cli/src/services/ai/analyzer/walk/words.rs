//! Turning shell words into rule-table arguments: globs, braces, `-C dir` options.

use crate::services::ai::analyzer::ast::{Part, Word};
use crate::services::ai::analyzer::rules::Arg;

/// `git -C dir`, `tar -C dir`, `make --directory=dir`: the command reads its relative paths
/// inside `dir`. Only literal absolute values count; anything else is no help.
pub(super) fn chdir_option(args: &[Arg]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        let value = if t == "-C" || t == "--directory" {
            i += 1;
            args.get(i).map(|v| v.text.clone())
        } else {
            t.strip_prefix("--directory=")
                .or_else(|| t.strip_prefix("-C"))
                .filter(|v| !v.is_empty())
                .map(String::from)
        };
        if let Some(v) = value
            && v.starts_with('/')
        {
            return Some(v);
        }
        i += 1;
    }
    None
}

pub(super) fn harmless_target(t: &str) -> bool {
    t == "/dev/null"
        || t == "/dev/stdout"
        || t == "/dev/stderr"
        || t == "/dev/tty"
        || t.starts_with("/dev/fd/")
}

pub(super) fn is_block_device(t: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/dev/sd",
        "/dev/hd",
        "/dev/vd",
        "/dev/xvd",
        "/dev/nvme",
        "/dev/mmcblk",
        "/dev/loop",
        "/dev/dm-",
        "/dev/md",
        "/dev/mapper/",
        "/dev/mem",
        "/dev/kmem",
        "/dev/port",
        "/dev/disk/",
    ];
    PREFIXES.iter().any(|p| t.starts_with(p))
}

/// Whether the shell expands this word as a glob: an unquoted `*`, `?` or `[`.
pub(super) fn shell_glob(w: &Word) -> bool {
    w.parts
        .iter()
        .any(|p| matches!(p, Part::Lit { text, quoted: false } if text.contains(['*', '?', '['])))
}

/// One word becomes one argument, or several after brace expansion.
pub(super) fn word_args(w: &Word) -> Vec<Arg> {
    let text = w.text();
    let dynamic = w.is_dynamic();
    let glob = shell_glob(w);
    let first_dyn = matches!(w.parts.first(), Some(Part::Dynamic(_)));
    let glob_lead = matches!(
        w.parts.first(),
        Some(Part::Lit { text, quoted: false }) if text.starts_with(['*', '?', '['])
    );
    let has_brace = w.parts.iter().any(|p| {
        matches!(p, Part::Lit { text, quoted: false } if text.contains('{') && text.contains('}'))
    });
    if has_brace {
        if dynamic {
            return vec![Arg {
                text,
                dynamic: true,
                flaggy: true,
                glob,
            }];
        }
        let chars: Vec<(char, bool)> = w
            .parts
            .iter()
            .flat_map(|p| match p {
                Part::Lit { text, quoted } => {
                    text.chars().map(|c| (c, *quoted)).collect::<Vec<_>>()
                }
                Part::Dynamic(_) => Vec::new(),
            })
            .collect();
        let mut budget = 256usize;
        return match brace_expand(&chars, &mut budget) {
            Some(items) => items
                .into_iter()
                .map(|t| {
                    let glob = glob || t.contains(['*', '?', '[']);
                    Arg {
                        flaggy: t.starts_with(['*', '?', '[']),
                        text: t,
                        dynamic: false,
                        glob,
                    }
                })
                .collect(),
            None => vec![Arg {
                text,
                dynamic: true,
                flaggy: true,
                glob,
            }],
        };
    }
    let flaggy = (dynamic && (first_dyn || text.starts_with('-'))) || glob_lead;
    vec![Arg {
        text,
        dynamic,
        flaggy,
        glob,
    }]
}

pub(super) enum Brace {
    Literal,
    TooBig,
    Alts(usize, Vec<Vec<(char, bool)>>),
}

pub(super) fn brace_expand(cs: &[(char, bool)], budget: &mut usize) -> Option<Vec<String>> {
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == ('{', false) {
            match parse_brace(cs, i) {
                Brace::TooBig => return None,
                Brace::Alts(close, alts) => {
                    let mut out = Vec::new();
                    for alt in alts {
                        let mut combined: Vec<(char, bool)> = cs[..i].to_vec();
                        combined.extend(alt);
                        combined.extend_from_slice(&cs[close + 1..]);
                        out.extend(brace_expand(&combined, budget)?);
                        if out.len() > *budget {
                            return None;
                        }
                    }
                    return Some(out);
                }
                Brace::Literal => {}
            }
        }
        i += 1;
    }
    Some(vec![cs.iter().map(|c| c.0).collect()])
}

pub(super) fn parse_brace(cs: &[(char, bool)], open: usize) -> Brace {
    let mut depth = 0usize;
    let mut commas: Vec<usize> = Vec::new();
    let mut close = None;
    for (k, &(c, quoted)) in cs.iter().enumerate().skip(open + 1) {
        if quoted {
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => {
                close = Some(k);
                break;
            }
            '}' => depth -= 1,
            ',' if depth == 0 => commas.push(k),
            _ => {}
        }
    }
    let Some(close) = close else {
        return Brace::Literal;
    };
    if !commas.is_empty() {
        let mut alts = Vec::new();
        let mut start = open + 1;
        for &c in commas.iter().chain(std::iter::once(&close)) {
            alts.push(cs[start..c].to_vec());
            start = c + 1;
        }
        return Brace::Alts(close, alts);
    }
    // `{a..z}`, `{1..10..2}`
    let body: String = cs[open + 1..close].iter().map(|c| c.0).collect();
    let pieces: Vec<&str> = body.split("..").collect();
    if pieces.len() < 2 || pieces.len() > 3 {
        return Brace::Literal;
    }
    let step = match pieces.get(2) {
        Some(s) => match s.parse::<i64>() {
            Ok(v) if v != 0 => v.abs(),
            _ => return Brace::Literal,
        },
        None => 1,
    };
    let items: Vec<String> =
        if let (Ok(a), Ok(b)) = (pieces[0].parse::<i64>(), pieces[1].parse::<i64>()) {
            if (b - a).unsigned_abs() / step as u64 > 256 {
                return Brace::TooBig;
            }
            let mut v = Vec::new();
            let mut x = a;
            while if a <= b { x <= b } else { x >= b } {
                v.push(x.to_string());
                x += if a <= b { step } else { -step };
            }
            v
        } else {
            let (mut a, mut b) = (pieces[0].chars(), pieces[1].chars());
            match (a.next(), a.next(), b.next(), b.next()) {
                (Some(x), None, Some(y), None)
                    if x.is_ascii_alphabetic() && y.is_ascii_alphabetic() =>
                {
                    let (x, y) = (x as i64, y as i64);
                    let mut v = Vec::new();
                    let mut c = x;
                    while if x <= y { c <= y } else { c >= y } {
                        v.push(((c as u8) as char).to_string());
                        c += if x <= y { step } else { -step };
                    }
                    v
                }
                _ => return Brace::Literal,
            }
        };
    Brace::Alts(
        close,
        items
            .into_iter()
            .map(|s| s.chars().map(|c| (c, false)).collect())
            .collect(),
    )
}
