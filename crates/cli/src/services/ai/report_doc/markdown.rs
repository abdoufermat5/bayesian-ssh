//! Markdown layout.

use std::fmt::Write;

use super::doc::{Block, Doc};

fn md_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~' => {
                o.push('\\');
                o.push(c);
            }
            '\n' => o.push(' '),
            _ => o.push(c),
        }
    }
    o
}

/// Inline code that survives backticks in `s`; `|` stays escaped for tables.
fn md_code(s: &str) -> String {
    let longest = s.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let body = s.replace('\n', " ").replace('|', "\\|");
    let pad = if body.starts_with('`') || body.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{body}{pad}{fence}")
}

pub(super) fn to_markdown(doc: &Doc) -> String {
    let mut s = format!("# {}\n", md_escape(&doc.title));
    for block in &doc.blocks {
        s.push('\n');
        match block {
            Block::Heading(h) => {
                let _ = writeln!(s, "## {}", md_escape(h));
            }
            Block::Subheading(h) => {
                let _ = writeln!(s, "### {}", md_escape(h));
            }
            Block::Para(p) => {
                let _ = writeln!(s, "{}", md_escape(p));
            }
            Block::Note(p) => {
                let _ = writeln!(s, "_{}_", md_escape(p));
            }
            Block::Facts(facts) => {
                for (k, v) in facts {
                    let _ = writeln!(s, "- **{k}:** {}", md_escape(v));
                }
            }
            Block::Table { cols, rows } => {
                let head: Vec<&str> = cols.iter().map(|c| c.name).collect();
                let _ = writeln!(s, "| {} |", head.join(" | "));
                let _ = writeln!(s, "|{}", "---|".repeat(cols.len()));
                for r in rows {
                    let cells: Vec<String> = cols
                        .iter()
                        .zip(r)
                        .map(|(c, v)| {
                            if c.is_code() && v != "-" {
                                md_code(v)
                            } else {
                                md_escape(v)
                            }
                        })
                        .collect();
                    let _ = writeln!(s, "| {} |", cells.join(" | "));
                }
            }
            Block::Pre { label, text } => {
                let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                let _ = writeln!(s, "{}\n", md_escape(label));
                let _ = writeln!(s, "{fence}text\n{}\n{fence}", text.trim_end());
            }
        }
    }
    s
}
