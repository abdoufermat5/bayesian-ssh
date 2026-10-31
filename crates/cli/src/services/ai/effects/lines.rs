//! A shell line split into the commands a person reads.

/// A shell line split into the commands a person reads, one per line: split
/// at `;`, `&&`, `||` and newlines outside quotes, with quotes, pipes and
/// here-doc bodies kept intact, `\` continuations joined, comments dropped
/// and `set -e…` boilerplate left out. Display only; the exact text is what
/// runs.
pub fn command_lines(command: &str) -> Vec<String> {
    let chars: Vec<char> = command.chars().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut backtick = false;
    // Here-docs opened on this line: (delimiter, strip leading tabs).
    let mut pending: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    let at = |i: usize| chars.get(i).copied();
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            {
                cur.push(c);
                if c == q {
                    quote = None;
                } else if c == '\\'
                    && q == '"'
                    && let Some(n) = at(i + 1)
                {
                    cur.push(n);
                    i += 1;
                }
                i += 1;
                continue;
            }
        }
        let word_start = cur.is_empty() || cur.ends_with(char::is_whitespace);
        match c {
            '\\' => match at(i + 1) {
                Some('\n') => {
                    if !cur.is_empty() && !cur.ends_with(' ') {
                        cur.push(' ');
                    }
                    i += 1;
                    while at(i + 1).is_some_and(|n| n == ' ' || n == '\t') {
                        i += 1;
                    }
                }
                Some(n) => {
                    cur.push(c);
                    cur.push(n);
                    i += 1;
                }
                None => cur.push(c),
            },
            '\'' | '"' => {
                quote = Some(c);
                cur.push(c);
            }
            '`' => {
                backtick = !backtick;
                cur.push(c);
            }
            '#' if word_start && depth == 0 && !backtick => {
                while at(i + 1).is_some_and(|n| n != '\n') {
                    i += 1;
                }
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            '<' if at(i + 1) == Some('<') && at(i + 2) != Some('<') && depth == 0 => {
                // Here-doc: remember the delimiter, read the body at the newline.
                cur.push_str("<<");
                i += 2;
                let strip = at(i) == Some('-');
                if strip {
                    cur.push('-');
                    i += 1;
                }
                while at(i).is_some_and(|n| n == ' ' || n == '\t') {
                    cur.push(chars[i]);
                    i += 1;
                }
                let mut delim = String::new();
                let mut q: Option<char> = None;
                while let Some(n) = at(i) {
                    match q {
                        Some(qc) if n == qc => q = None,
                        Some(_) => delim.push(n),
                        None if n == '\'' || n == '"' => q = Some(n),
                        None if n.is_whitespace()
                            || matches!(n, ';' | '&' | '|' | '<' | '>' | ')') =>
                        {
                            break;
                        }
                        None => delim.push(n),
                    }
                    cur.push(n);
                    i += 1;
                }
                pending.push((delim, strip));
                continue;
            }
            ';' | '&' | '|' | '\n' if depth > 0 || backtick => cur.push(c),
            '\n' => {
                if !pending.is_empty() {
                    // Body lines up to each terminator belong to this command.
                    for (delim, strip) in std::mem::take(&mut pending) {
                        cur.push('\n');
                        loop {
                            i += 1;
                            let start = i;
                            while at(i).is_some_and(|n| n != '\n') {
                                i += 1;
                            }
                            let body: String = chars[start..i].iter().collect();
                            cur.push_str(&body);
                            let check = if strip {
                                body.trim_start_matches('\t')
                            } else {
                                body.as_str()
                            };
                            if check == delim || i >= chars.len() {
                                break;
                            }
                            cur.push('\n');
                        }
                    }
                    lines.push(std::mem::take(&mut cur));
                } else if cur.trim_end().ends_with('|') && !cur.trim_end().ends_with("||") {
                    cur.push(' ');
                    while at(i + 1).is_some_and(|n| n == ' ' || n == '\t') {
                        i += 1;
                    }
                } else {
                    lines.push(std::mem::take(&mut cur));
                }
            }
            ';' | '&' | '|' if !pending.is_empty() => cur.push(c),
            ';' => lines.push(std::mem::take(&mut cur)),
            '&' if at(i + 1) == Some('&') => {
                i += 1;
                lines.push(std::mem::take(&mut cur));
            }
            '|' if at(i + 1) == Some('|') => {
                i += 1;
                lines.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    lines.push(cur);
    lines
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !is_boilerplate(l))
        .collect()
}

fn is_boilerplate(line: &str) -> bool {
    let mut words = line.split_whitespace();
    words.next() == Some("set")
        && words.all(|w| {
            w.starts_with('-')
                || w.starts_with('+')
                || matches!(w, "pipefail" | "errexit" | "nounset" | "xtrace")
        })
}
