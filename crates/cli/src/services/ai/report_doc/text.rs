//! Plain-text and PDF layouts.

use crate::services::ai::report::RunReport;
use crate::services::ai::ui;
use chrono::{DateTime, Local};

use super::doc::{Block, Doc};

/// Characters per line of the text and PDF layouts.
pub(super) const WIDTH: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Bold,
    Normal,
}

/// Word-wrap `text` to `width` columns, breaking words longer than a line.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut len = 0;
        for word in para.split(' ') {
            let mut word: Vec<char> = word.chars().collect();
            loop {
                let wl = word.len();
                let sep = usize::from(len > 0);
                if len + sep + wl <= width {
                    if sep == 1 {
                        line.push(' ');
                    }
                    line.extend(word.iter());
                    len += sep + wl;
                    break;
                }
                if len > 0 {
                    out.push(std::mem::take(&mut line));
                    len = 0;
                    continue;
                }
                // A word longer than a line.
                let rest = word.split_off(width.max(1));
                line.extend(word.iter());
                out.push(std::mem::take(&mut line));
                word = rest;
            }
        }
        out.push(line);
    }
    out
}

fn width_of(s: &str) -> usize {
    s.chars().count()
}

pub(super) fn pad(s: &str, w: usize) -> String {
    let mut p = s.to_string();
    p.extend(std::iter::repeat_n(' ', w.saturating_sub(width_of(s))));
    p
}

fn text_lines(doc: &Doc) -> Vec<(Look, String)> {
    let mut out: Vec<(Look, String)> = Vec::new();
    let blank = |out: &mut Vec<(Look, String)>| {
        if out.last().is_some_and(|(_, l)| !l.is_empty()) {
            out.push((Look::Normal, String::new()));
        }
    };
    let indented = |out: &mut Vec<(Look, String)>, text: &str, indent: usize| {
        for l in wrap(text, WIDTH - indent) {
            out.push((Look::Normal, format!("{}{l}", " ".repeat(indent))));
        }
    };

    out.push((Look::Bold, doc.title.clone()));
    out.push((Look::Bold, "=".repeat(width_of(&doc.title))));
    for block in &doc.blocks {
        match block {
            Block::Heading(h) => {
                blank(&mut out);
                out.push((Look::Normal, String::new()));
                out.push((Look::Bold, h.to_uppercase()));
            }
            Block::Subheading(h) => {
                blank(&mut out);
                for l in wrap(h, WIDTH) {
                    out.push((Look::Bold, l));
                }
            }
            Block::Para(p) | Block::Note(p) => {
                blank(&mut out);
                indented(&mut out, p, 0);
            }
            Block::Facts(facts) => {
                blank(&mut out);
                let kw = facts.iter().map(|(k, _)| width_of(k)).max().unwrap_or(0) + 2;
                for (k, v) in facts {
                    let lines = wrap(v, WIDTH - kw);
                    for (i, l) in lines.iter().enumerate() {
                        let key = if i == 0 {
                            format!("{k}:")
                        } else {
                            String::new()
                        };
                        out.push((Look::Normal, format!("{}{l}", pad(&key, kw))));
                    }
                }
            }
            Block::Table { cols, rows } => {
                blank(&mut out);
                let widths: Vec<usize> = cols
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        rows.iter()
                            .map(|r| width_of(&r[i]))
                            .chain([width_of(c.name)])
                            .max()
                            .unwrap_or(0)
                    })
                    .collect();
                let total = widths.iter().sum::<usize>() + 2 * (cols.len() - 1);
                if total <= WIDTH {
                    let row_line = |cells: Vec<&str>| {
                        let last = cells.len() - 1;
                        cells
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                if i == last {
                                    c.to_string()
                                } else {
                                    pad(c, widths[i])
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("  ")
                    };
                    out.push((Look::Bold, row_line(cols.iter().map(|c| c.name).collect())));
                    out.push((
                        Look::Normal,
                        widths
                            .iter()
                            .map(|w| "-".repeat(*w))
                            .collect::<Vec<_>>()
                            .join("  "),
                    ));
                    for r in rows {
                        out.push((
                            Look::Normal,
                            row_line(r.iter().map(String::as_str).collect()),
                        ));
                    }
                } else {
                    // Too wide: short columns on one line, long ones below it.
                    for (n, r) in rows.iter().enumerate() {
                        if n > 0 {
                            out.push((Look::Normal, String::new()));
                        }
                        let head: Vec<String> = cols
                            .iter()
                            .zip(r)
                            .enumerate()
                            .filter(|(_, (c, _))| !c.is_long())
                            .map(|(i, (_, v))| pad(v, widths[i]))
                            .collect();
                        out.push((Look::Normal, head.join("  ").trim_end().to_string()));
                        for (c, v) in cols.iter().zip(r).filter(|(c, _)| c.is_long()) {
                            if v != "-" {
                                indented(&mut out, &format!("{}: {v}", c.name), 4);
                            }
                        }
                    }
                }
            }
            Block::Pre { label, text } => {
                blank(&mut out);
                indented(&mut out, label, 0);
                for l in text.trim_end().lines() {
                    for w in wrap(&l.replace('\t', "    "), WIDTH - 4) {
                        out.push((Look::Normal, format!("    {w}")));
                    }
                }
            }
        }
    }
    out
}

pub(super) fn to_text(doc: &Doc) -> String {
    let mut s = String::new();
    for (_, l) in text_lines(doc) {
        s.push_str(l.trim_end());
        s.push('\n');
    }
    s
}

/// One character in the PDF's WinAnsi encoding.
fn win_ansi(c: char, out: &mut Vec<u8>) {
    let b = match c {
        '(' | ')' | '\\' => {
            out.push(b'\\');
            c as u8
        }
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        _ => b'?',
    };
    out.push(b);
}

fn pdf_string(s: &str) -> Vec<u8> {
    let mut v = vec![b'('];
    for c in s.chars() {
        win_ansi(if c == '\t' { ' ' } else { c }, &mut v);
    }
    v.push(b')');
    v
}

/// A text-only A4 PDF in Courier, from the text layout. No dependency, no embedded font.
pub(super) fn to_pdf(doc: &Doc, report: &RunReport, generated: DateTime<Local>) -> Vec<u8> {
    const PAGE_W: u32 = 595;
    const PAGE_H: u32 = 842;
    const LEFT: u32 = 42;
    const TOP: u32 = 794;
    const LEADING: u32 = 11;
    const PER_PAGE: usize = 67;

    let lines = text_lines(doc);
    let pages: Vec<&[(Look, String)]> = lines.chunks(PER_PAGE).collect();
    let n_pages = pages.len().max(1);

    // Objects: 1 catalog, 2 pages, 3 Courier, 4 Courier-Bold, 5 info, then page + content pairs.
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 6 + 2 * i)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {n_pages} >>",
            kids.join(" ")
        )
        .into_bytes(),
    );
    for font in ["Courier", "Courier-Bold"] {
        objects.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding >>"
            )
            .into_bytes(),
        );
    }
    let mut info = b"<< /Title ".to_vec();
    info.extend(pdf_string(&doc.title));
    info.extend(b" /Producer ");
    info.extend(pdf_string(&format!("bssh {}", env!("CARGO_PKG_VERSION"))));
    info.extend(format!(" /CreationDate (D:{}) >>", generated.format("%Y%m%d%H%M%S")).into_bytes());
    objects.push(info);

    let footer = format!("bssh AI run report {}", ui::short_id(&report.run_id));
    for i in 0..n_pages {
        let mut content: Vec<u8> = Vec::new();
        content.extend(format!("BT /F1 9 Tf {LEADING} TL {LEFT} {TOP} Td\n").into_bytes());
        let mut look = Look::Normal;
        for (l, text) in pages.get(i).copied().unwrap_or_default() {
            if *l != look {
                look = *l;
                let f = if look == Look::Bold { "F2" } else { "F1" };
                content.extend(format!("/{f} 9 Tf\n").into_bytes());
            }
            content.extend(pdf_string(text.trim_end()));
            content.extend(b" Tj T*\n");
        }
        content.extend(b"ET\n");
        content.extend(format!("BT /F1 8 Tf {LEFT} 28 Td ").into_bytes());
        content.extend(pdf_string(&format!(
            "{footer} - page {} of {n_pages}",
            i + 1
        )));
        content.extend(b" Tj ET\n");

        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                7 + 2 * i
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend(content);
        stream.extend(b"\nendstream");
        objects.push(stream);
    }

    let mut pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", i + 1).into_bytes());
        pdf.extend(body);
        pdf.extend(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for o in offsets {
        pdf.extend(format!("{o:010} 00000 n \n").into_bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .into_bytes(),
    );
    pdf
}
