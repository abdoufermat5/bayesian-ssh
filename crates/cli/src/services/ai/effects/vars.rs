//! Shell variables a command line assigns, expanded into targets.

use std::collections::HashMap;

use super::lines::command_lines;

/// `NAME=value` assignments made by the command line, in order. Only plain
/// statements count (`A=x;`, `export A=x`); `A=x cmd` is scoped to `cmd`,
/// and values built by command substitution are unknown.
pub(super) fn assignments(command: &str) -> HashMap<String, String> {
    let mut vars: HashMap<String, String> = HashMap::new();
    for line in command_lines(command) {
        let words = shell_words(&line);
        let mut it = words.iter().peekable();
        let declares = it.peek().is_some_and(|w| {
            matches!(
                w.text.as_str(),
                "export" | "declare" | "typeset" | "readonly" | "local"
            )
        });
        if declares {
            it.next();
        }
        let rest: Vec<&Word> = it.collect();
        if rest.first().is_some_and(|w| w.text == "unset") {
            for w in &rest[1..] {
                vars.remove(&w.text);
            }
            continue;
        }
        let all_assign = !rest.is_empty()
            && rest.iter().all(|w| {
                assignment_name(&w.text).is_some()
                    || (declares && is_name(&w.text))
                    || (declares && w.text.starts_with('-'))
            });
        if !all_assign {
            continue;
        }
        for w in rest {
            let Some((name, value)) = w.text.split_once('=').filter(|(n, _)| is_name(n)) else {
                continue;
            };
            if w.substitution {
                vars.remove(name);
            } else {
                let v = expand(value, &vars).replace(LITERAL_DOLLAR, "$");
                vars.insert(name.to_string(), v);
            }
        }
    }
    vars
}

fn assignment_name(word: &str) -> Option<&str> {
    word.split_once('=').map(|(n, _)| n).filter(|n| is_name(n))
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct Word {
    /// Quotes removed; a `$` that is not special (single-quoted, escaped) is
    /// held as `\u{1}` until the value is final.
    text: String,
    /// Holds `$(...)` or backticks.
    substitution: bool,
}

const LITERAL_DOLLAR: char = '\u{1}';

/// Words of a simple line with quotes removed; `|`, `&` and redirects become
/// words of their own so they spoil an "all assignments" check.
fn shell_words(line: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut subst = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    fn flush(words: &mut Vec<Word>, cur: &mut String, started: &mut bool, subst: &mut bool) {
        if *started {
            words.push(Word {
                text: std::mem::take(cur),
                substitution: *subst,
            });
            *started = false;
            *subst = false;
        }
    }
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), '$') => cur.push(LITERAL_DOLLAR),
            (Some('"'), '\\') => {
                if let Some(n) = chars.next() {
                    match n {
                        '$' => cur.push(LITERAL_DOLLAR),
                        '"' | '\\' | '`' => cur.push(n),
                        _ => {
                            cur.push('\\');
                            cur.push(n);
                        }
                    }
                }
            }
            (Some('"'), '`') => {
                subst = true;
                cur.push(c);
            }
            (Some('"'), '$') if chars.peek() == Some(&'(') => {
                subst = true;
                cur.push(c);
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                started = true;
                match chars.next() {
                    Some('$') => cur.push(LITERAL_DOLLAR),
                    Some(n) => cur.push(n),
                    None => {}
                }
            }
            (None, '`') => {
                started = true;
                subst = true;
                cur.push(c);
            }
            (None, '$') => {
                started = true;
                if chars.peek() == Some(&'(') {
                    subst = true;
                }
                cur.push(c);
            }
            (None, c) if c.is_whitespace() => flush(&mut words, &mut cur, &mut started, &mut subst),
            (None, '|' | '&' | '<' | '>' | '(' | ')') => {
                flush(&mut words, &mut cur, &mut started, &mut subst);
                words.push(Word {
                    text: c.to_string(),
                    substitution: false,
                });
            }
            (None, c) => {
                started = true;
                cur.push(c);
            }
        }
    }
    flush(&mut words, &mut cur, &mut started, &mut subst);
    words
}

/// `$A` and `${A}` replaced where `A` is known; everything else stays.
pub(super) fn expand(text: &str, vars: &HashMap<String, String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let (name, end) = if chars.get(i + 1) == Some(&'{') {
            match chars[i + 2..].iter().position(|c| *c == '}') {
                Some(p) => (
                    chars[i + 2..i + 2 + p].iter().collect::<String>(),
                    i + 3 + p,
                ),
                None => (String::new(), i + 1),
            }
        } else {
            let n: String = chars[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                .collect();
            let len = n.chars().count();
            (n, i + 1 + len)
        };
        match vars.get(&name).filter(|_| is_name(&name)) {
            Some(v) => {
                out.push_str(v);
                i = end;
            }
            None => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}
