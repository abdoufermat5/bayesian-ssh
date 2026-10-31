//! Option parsing shared by the wrapper handlers.

use crate::services::ai::analyzer::rules::Arg;

use super::state::SYSTEM_DIRS;

/// What a wrapper's options looked like.
pub(super) struct Opts {
    /// Index of the first argument that is not an option.
    pub(super) rest: usize,
    /// Flags seen: `-i`, `--login`.
    pub(super) seen: Vec<String>,
    /// Options with values: (`-u`, `root`).
    pub(super) values: Vec<(String, String)>,
}

impl Opts {
    pub(super) fn has(&self, names: &[&str]) -> bool {
        self.seen
            .iter()
            .chain(self.values.iter().map(|(k, _)| k))
            .any(|s| names.contains(&s.as_str()))
    }

    pub(super) fn value(&self, names: &[&str]) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| names.contains(&k.as_str()))
            .map(|(_, v)| v.as_str())
    }
}

/// Option grammar of a wrapper.
pub(super) struct Spec {
    /// Short flags without a value.
    pub(super) flags: &'static str,
    /// Short flags with a value (attached or next argument).
    pub(super) with_arg: &'static str,
    /// Long options: (`--name`, takes a value).
    pub(super) long: &'static [(&'static str, bool)],
    /// Accept `-5` style numbers (`nice -5`).
    pub(super) numeric: bool,
}

pub(super) fn parse_opts(args: &[Arg], spec: &Spec) -> Result<Opts, String> {
    let mut o = Opts {
        rest: 0,
        seen: Vec::new(),
        values: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        if t == "--" {
            i += 1;
            break;
        }
        if !t.starts_with('-') || t == "-" {
            break;
        }
        if let Some(long) = t.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            let full = format!("--{name}");
            let Some((_, takes)) = spec.long.iter().find(|(n, _)| *n == full) else {
                return Err(format!("unknown option {full}"));
            };
            if *takes {
                let v = match value {
                    Some(v) => v.to_string(),
                    None => {
                        i += 1;
                        args.get(i)
                            .map(|a| a.text.clone())
                            .ok_or_else(|| format!("{full} needs a value"))?
                    }
                };
                o.values.push((full, v));
            } else {
                o.seen.push(full);
            }
            i += 1;
            continue;
        }
        if spec.numeric && t[1..].chars().all(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        let chars: Vec<char> = t[1..].chars().collect();
        for (k, ch) in chars.iter().enumerate() {
            if spec.with_arg.contains(*ch) {
                let rest: String = chars[k + 1..].iter().collect();
                let v = if rest.is_empty() {
                    i += 1;
                    args.get(i)
                        .map(|a| a.text.clone())
                        .ok_or_else(|| format!("-{ch} needs a value"))?
                } else {
                    rest
                };
                o.values.push((format!("-{ch}"), v));
                break;
            } else if spec.flags.contains(*ch) {
                o.seen.push(format!("-{ch}"));
            } else {
                return Err(format!("unknown option -{ch}"));
            }
        }
        i += 1;
    }
    o.rest = i;
    Ok(o)
}

pub(super) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `program` is trusted by name when it has no path or lives in a system directory.
pub(super) fn trusted_path(program: &str) -> bool {
    match program.rfind('/') {
        None => true,
        Some(p) => SYSTEM_DIRS.contains(&&program[..p]),
    }
}

pub(super) fn is_assignment(text: &str) -> bool {
    let Some(eq) = text.find('=') else {
        return false;
    };
    let name = text[..eq].strip_suffix('+').unwrap_or(&text[..eq]);
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
