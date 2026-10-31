//! Self-contained HTML layout.

use std::fmt::Write;

use super::doc::{Block, Col, Doc};

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

const CSS: &str = "\
:root{color-scheme:light;--fg:#1f2328;--muted:#59636e;--border:#d1d9e0;--soft:#f6f8fa}\
*{box-sizing:border-box}\
body{margin:0;font:14px/1.5 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:var(--fg);background:#fff}\
main{max-width:1040px;margin:0 auto;padding:32px 24px 48px}\
h1{font-size:22px;margin:0 0 16px}\
h2{font-size:16px;margin:28px 0 8px;padding-bottom:4px;border-bottom:1px solid var(--border)}\
h3{font-size:14px;margin:20px 0 6px}\
dl{display:grid;grid-template-columns:max-content 1fr;gap:4px 16px;margin:0}\
dt{color:var(--muted)}dd{margin:0;overflow-wrap:anywhere}\
p{margin:6px 0}\
.note{color:var(--muted);font-size:12px;margin-top:32px}\
table{border-collapse:collapse;width:100%;margin:8px 0;font-size:13px}\
th,td{border:1px solid var(--border);padding:4px 8px;text-align:left;vertical-align:top}\
th{background:var(--soft);font-weight:600}\
td{overflow-wrap:anywhere}\
.nw{white-space:nowrap;overflow-wrap:normal}\
code,pre{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px}\
pre{background:var(--soft);border:1px solid var(--border);padding:8px;white-space:pre-wrap;overflow-wrap:anywhere}\
.label{color:var(--muted);margin-top:10px}\
@media print{main{padding:0;max-width:none}h2{break-after:avoid}tr,pre{break-inside:avoid}}\
@page{size:A4;margin:16mm}";

/// Short columns (times, tools, hosts) stay on one line; long ones wrap.
fn nowrap(c: &Col) -> &'static str {
    if c.is_long() { "" } else { " class=\"nw\"" }
}

pub(super) fn to_html(doc: &Doc) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
         <meta name=\"generator\" content=\"bssh {}\">\n<title>{}</title>\n<style>{CSS}</style>\n\
         </head>\n<body>\n<main>\n<h1>{}</h1>\n",
        env!("CARGO_PKG_VERSION"),
        esc(&doc.title),
        esc(&doc.title)
    );
    for block in &doc.blocks {
        match block {
            Block::Heading(h) => {
                let _ = writeln!(s, "<h2>{}</h2>", esc(h));
            }
            Block::Subheading(h) => {
                let _ = writeln!(s, "<h3>{}</h3>", esc(h));
            }
            Block::Para(p) => {
                let _ = writeln!(s, "<p>{}</p>", esc(p));
            }
            Block::Note(p) => {
                let _ = writeln!(s, "<p class=\"note\">{}</p>", esc(p));
            }
            Block::Facts(facts) => {
                s.push_str("<dl>\n");
                for (k, v) in facts {
                    let _ = writeln!(s, "<dt>{}</dt><dd>{}</dd>", esc(k), esc(v));
                }
                s.push_str("</dl>\n");
            }
            Block::Table { cols, rows } => {
                s.push_str("<table>\n<thead><tr>");
                for c in cols {
                    let _ = write!(s, "<th{}>{}</th>", nowrap(c), esc(c.name));
                }
                s.push_str("</tr></thead>\n<tbody>\n");
                for r in rows {
                    s.push_str("<tr>");
                    for (c, v) in cols.iter().zip(r) {
                        if c.is_code() && v != "-" {
                            let _ = write!(s, "<td><code>{}</code></td>", esc(v));
                        } else {
                            let _ = write!(s, "<td{}>{}</td>", nowrap(c), esc(v));
                        }
                    }
                    s.push_str("</tr>\n");
                }
                s.push_str("</tbody>\n</table>\n");
            }
            Block::Pre { label, text } => {
                let _ = writeln!(
                    s,
                    "<p class=\"label\">{}</p>\n<pre>{}</pre>",
                    esc(label),
                    esc(text.trim_end())
                );
            }
        }
    }
    s.push_str("</main>\n</body>\n</html>\n");
    s
}
