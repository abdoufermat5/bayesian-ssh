//! POSIX/bash shell tokenizer. Command substitutions are parsed on the spot (recursively),
//! so every command hidden in a word ends up in `Word::subs`.

use super::ast::{Node, Part, Word};
use super::parser::parse_program;

/// Nesting limit for `$(...)`, backticks, and `sh -c` strings.
pub const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Semi,
    /// `;;`, `;&`, `;;&`
    CaseEnd,
    Amp,
    AndIf,
    OrIf,
    Pipe,
    PipeAmp,
    LParen,
    RParen,
    Less,
    Great,
    DGreat,
    Clobber,
    LessGreat,
    LessAnd,
    GreatAnd,
    AndGreat,
    AndDGreat,
    TLess,
}

impl Op {
    /// Source text, for `[[ ... ]]` bodies where operators are plain words.
    pub fn text(self) -> &'static str {
        match self {
            Op::Semi => ";",
            Op::CaseEnd => ";;",
            Op::Amp => "&",
            Op::AndIf => "&&",
            Op::OrIf => "||",
            Op::Pipe => "|",
            Op::PipeAmp => "|&",
            Op::LParen => "(",
            Op::RParen => ")",
            Op::Less => "<",
            Op::Great => ">",
            Op::DGreat => ">>",
            Op::Clobber => ">|",
            Op::LessGreat => "<>",
            Op::LessAnd => "<&",
            Op::GreatAnd => ">&",
            Op::AndGreat => "&>",
            Op::AndDGreat => "&>>",
            Op::TLess => "<<<",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Tok {
    Word(Word),
    /// `2` in `2>file`
    IoNumber(u32),
    Op(Op),
    Newline,
    /// Index into `Lexed::heredocs`.
    HereDoc(usize),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub pos: usize,
}

#[derive(Debug, Clone)]
pub struct Heredoc {
    pub delim: String,
    pub subs: Vec<Node>,
}

pub struct Lexed {
    pub toks: Vec<Token>,
    pub heredocs: Vec<Heredoc>,
}

pub fn lex(src: &str, depth: usize) -> Result<Lexed, String> {
    let mut lx = Lexer {
        chars: src.chars().collect(),
        i: 0,
        depth,
        toks: Vec::new(),
        heredocs: Vec::new(),
        pending: Vec::new(),
        want_delim: None,
    };
    lx.run()?;
    if lx.want_delim.is_some() {
        return Err("here-doc without delimiter".into());
    }
    Ok(Lexed {
        toks: lx.toks,
        heredocs: lx.heredocs,
    })
}

/// Commands hidden in text the shell expands but doesn't split into words
/// (here-doc bodies, `${...}` and `$((...))` contents).
fn subs_in_text(text: &str, depth: usize) -> Result<Vec<Node>, String> {
    let mut lx = Lexer::bare(text, depth);
    let mut wb = WordBuilder::default();
    lx.dq_content(None, &mut wb)?;
    Ok(wb.subs)
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    depth: usize,
    toks: Vec<Token>,
    heredocs: Vec<Heredoc>,
    /// Here-docs whose bodies start after the next newline: (index, strip tabs, quoted).
    pending: Vec<(usize, bool, bool)>,
    /// After `<<` / `<<-`: the next word is the delimiter. Holds `strip_tabs`.
    want_delim: Option<bool>,
}

#[derive(Default)]
struct WordBuilder {
    parts: Vec<Part>,
    subs: Vec<Node>,
}

impl WordBuilder {
    fn lit(&mut self, text: &str, quoted: bool) {
        if let Some(Part::Lit {
            text: last,
            quoted: q,
        }) = self.parts.last_mut()
        {
            if *q == quoted {
                last.push_str(text);
                return;
            }
        }
        self.parts.push(Part::Lit {
            text: text.to_string(),
            quoted,
        });
    }

    fn ch(&mut self, c: char, quoted: bool) {
        let mut buf = [0u8; 4];
        self.lit(c.encode_utf8(&mut buf), quoted);
    }

    fn finish(self) -> Word {
        Word {
            parts: self.parts,
            subs: self.subs,
        }
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

impl Lexer {
    fn bare(text: &str, depth: usize) -> Self {
        Lexer {
            chars: text.chars().collect(),
            i: 0,
            depth,
            toks: Vec::new(),
            heredocs: Vec::new(),
            pending: Vec::new(),
            want_delim: None,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.i + n).copied()
    }

    fn push(&mut self, tok: Tok, pos: usize) {
        self.toks.push(Token { tok, pos });
    }

    fn run(&mut self) -> Result<(), String> {
        loop {
            while let Some(c) = self.peek() {
                if c == ' ' || c == '\t' {
                    self.i += 1;
                } else if c == '\\' && self.peek_at(1) == Some('\n') {
                    self.i += 2;
                } else {
                    break;
                }
            }
            let Some(c) = self.peek() else {
                return Ok(());
            };
            let pos = self.i;
            match c {
                '#' => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                '\n' => {
                    self.i += 1;
                    self.push(Tok::Newline, pos);
                    self.read_heredoc_bodies()?;
                }
                '<' | '>' if self.peek_at(1) == Some('(') => self.word()?,
                ';' | '&' | '|' | '(' | ')' | '<' | '>' => self.operator()?,
                _ => self.word()?,
            }
        }
    }

    fn operator(&mut self) -> Result<(), String> {
        let pos = self.i;
        let c = self.chars[self.i];
        let n1 = self.peek_at(1);
        let n2 = self.peek_at(2);
        let (op, len) = match (c, n1, n2) {
            (';', Some(';'), Some('&')) => (Op::CaseEnd, 3),
            (';', Some(';'), _) | (';', Some('&'), _) => (Op::CaseEnd, 2),
            (';', _, _) => (Op::Semi, 1),
            ('&', Some('&'), _) => (Op::AndIf, 2),
            ('&', Some('>'), Some('>')) => (Op::AndDGreat, 3),
            ('&', Some('>'), _) => (Op::AndGreat, 2),
            ('&', _, _) => (Op::Amp, 1),
            ('|', Some('|'), _) => (Op::OrIf, 2),
            ('|', Some('&'), _) => (Op::PipeAmp, 2),
            ('|', _, _) => (Op::Pipe, 1),
            ('(', _, _) => (Op::LParen, 1),
            (')', _, _) => (Op::RParen, 1),
            ('<', Some('<'), Some('<')) => (Op::TLess, 3),
            ('<', Some('<'), Some('-')) => {
                self.i += 3;
                self.want_delim = Some(true);
                return Ok(());
            }
            ('<', Some('<'), _) => {
                self.i += 2;
                self.want_delim = Some(false);
                return Ok(());
            }
            ('<', Some('&'), _) => (Op::LessAnd, 2),
            ('<', Some('>'), _) => (Op::LessGreat, 2),
            ('<', _, _) => (Op::Less, 1),
            ('>', Some('>'), _) => (Op::DGreat, 2),
            ('>', Some('&'), _) => (Op::GreatAnd, 2),
            ('>', Some('|'), _) => (Op::Clobber, 2),
            ('>', _, _) => (Op::Great, 1),
            _ => return Err(format!("unexpected character '{c}'")),
        };
        if self.want_delim.is_some() {
            return Err("here-doc without delimiter".into());
        }
        self.i += len;
        self.push(Tok::Op(op), pos);
        Ok(())
    }

    fn word(&mut self) -> Result<(), String> {
        let pos = self.i;
        let word = self.read_word()?;
        if let Some(strip) = self.want_delim.take() {
            let quoted = word
                .parts
                .iter()
                .any(|p| matches!(p, Part::Lit { quoted: true, .. }));
            let idx = self.heredocs.len();
            self.heredocs.push(Heredoc {
                delim: word.text(),
                subs: Vec::new(),
            });
            self.pending.push((idx, strip, quoted));
            self.push(Tok::HereDoc(idx), pos);
            return Ok(());
        }
        if let [Part::Lit {
            text,
            quoted: false,
        }] = word.parts.as_slice()
        {
            if text.chars().all(|c| c.is_ascii_digit())
                && matches!(self.peek(), Some('<') | Some('>'))
                && self.peek_at(1) != Some('(')
            {
                if let Ok(n) = text.parse::<u32>() {
                    self.push(Tok::IoNumber(n), pos);
                    return Ok(());
                }
            }
        }
        self.push(Tok::Word(word), pos);
        Ok(())
    }

    fn read_heredoc_bodies(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.pending);
        for (idx, strip, quoted) in pending {
            let delim = self.heredocs[idx].delim.clone();
            let mut body = String::new();
            while self.i < self.chars.len() {
                let start = self.i;
                while self.peek().is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
                let line: String = self.chars[start..self.i].iter().collect();
                if self.peek() == Some('\n') {
                    self.i += 1;
                }
                let cmp = if strip {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if cmp == delim {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if !quoted {
                self.heredocs[idx].subs = subs_in_text(&body, self.depth + 1)?;
            }
        }
        Ok(())
    }

    /// One shell word, with quotes removed and expansions recorded.
    fn read_word(&mut self) -> Result<Word, String> {
        let mut wb = WordBuilder::default();
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' => break,
                '<' | '>' => {
                    if self.peek_at(1) == Some('(') {
                        self.process_subst(&mut wb)?;
                    } else {
                        break;
                    }
                }
                '\\' => match self.peek_at(1) {
                    Some('\n') => self.i += 2,
                    Some(n) => {
                        wb.ch(n, true);
                        self.i += 2;
                    }
                    None => {
                        wb.ch('\\', true);
                        self.i += 1;
                    }
                },
                '\'' => {
                    self.i += 1;
                    let start = self.i;
                    while self.peek().is_some_and(|c| c != '\'') {
                        self.i += 1;
                    }
                    if self.peek().is_none() {
                        return Err("unterminated single quote".into());
                    }
                    let text: String = self.chars[start..self.i].iter().collect();
                    self.i += 1;
                    wb.lit(&text, true);
                }
                '"' => {
                    self.i += 1;
                    wb.lit("", true);
                    self.dq_content(Some('"'), &mut wb)?;
                }
                '$' => self.dollar(&mut wb, false)?,
                '`' => self.backtick(&mut wb)?,
                _ => {
                    wb.ch(c, false);
                    self.i += 1;
                }
            }
        }
        Ok(wb.finish())
    }

    /// Inside double quotes (terminator `"`) or a here-doc body (no terminator).
    fn dq_content(&mut self, term: Option<char>, wb: &mut WordBuilder) -> Result<(), String> {
        loop {
            let Some(c) = self.peek() else {
                return if term.is_some() {
                    Err("unterminated double quote".into())
                } else {
                    Ok(())
                };
            };
            if Some(c) == term {
                self.i += 1;
                return Ok(());
            }
            match c {
                '\\' => match self.peek_at(1) {
                    Some('\n') => self.i += 2,
                    Some(n @ ('$' | '`' | '"' | '\\')) => {
                        wb.ch(n, true);
                        self.i += 2;
                    }
                    _ => {
                        wb.ch('\\', true);
                        self.i += 1;
                    }
                },
                '$' => self.dollar(wb, true)?,
                '`' => self.backtick(wb)?,
                _ => {
                    wb.ch(c, true);
                    self.i += 1;
                }
            }
        }
    }

    fn dollar(&mut self, wb: &mut WordBuilder, in_dq: bool) -> Result<(), String> {
        let start = self.i;
        match self.peek_at(1) {
            Some('\'') if !in_dq => {
                self.i += 2;
                let text = self.ansi_c()?;
                wb.lit(&text, true);
            }
            Some('"') if !in_dq => {
                self.i += 2;
                wb.lit("", true);
                self.dq_content(Some('"'), wb)?;
            }
            Some('(') => {
                if self.peek_at(2) == Some('(') {
                    if let Some(end) = find_close_paren(&self.chars, self.i + 2) {
                        let inner = &self.chars[self.i + 2..end];
                        if inner.last() == Some(&')')
                            && find_close_paren(inner, 1) == Some(inner.len() - 1)
                        {
                            // `$(( ... ))`: arithmetic. Expansions inside still run.
                            let body: String = inner[1..inner.len() - 1].iter().collect();
                            wb.subs.extend(subs_in_text(&body, self.depth + 1)?);
                            let src: String = self.chars[start..=end].iter().collect();
                            wb.parts.push(Part::Dynamic(src));
                            self.i = end + 1;
                            return Ok(());
                        }
                    }
                }
                let end = find_close_paren(&self.chars, self.i + 2).ok_or("unterminated $(")?;
                let inner: String = self.chars[self.i + 2..end].iter().collect();
                wb.subs.push(self.parse_inner(&inner)?);
                let src: String = self.chars[start..=end].iter().collect();
                wb.parts.push(Part::Dynamic(src));
                self.i = end + 1;
            }
            Some('{') => {
                let end = find_close_brace(&self.chars, self.i + 2).ok_or("unterminated ${")?;
                let inner: String = self.chars[self.i + 2..end].iter().collect();
                wb.subs.extend(subs_in_text(&inner, self.depth + 1)?);
                let src: String = self.chars[start..=end].iter().collect();
                wb.parts.push(Part::Dynamic(src));
                self.i = end + 1;
            }
            Some(c) if is_name_start(c) => {
                self.i += 1;
                let s = self.i;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    self.i += 1;
                }
                let name: String = self.chars[s..self.i].iter().collect();
                wb.parts.push(Part::Dynamic(format!("${name}")));
            }
            Some(c)
                if c.is_ascii_digit() || matches!(c, '@' | '*' | '#' | '?' | '$' | '!' | '-') =>
            {
                self.i += 2;
                wb.parts.push(Part::Dynamic(format!("${c}")));
            }
            _ => {
                wb.ch('$', in_dq);
                self.i += 1;
            }
        }
        Ok(())
    }

    fn backtick(&mut self, wb: &mut WordBuilder) -> Result<(), String> {
        let start = self.i;
        let end = find_backtick_end(&self.chars, self.i + 1).ok_or("unterminated backtick")?;
        let mut inner = String::new();
        let mut k = self.i + 1;
        while k < end {
            let c = self.chars[k];
            if c == '\\' && matches!(self.chars.get(k + 1), Some('`' | '\\' | '$')) {
                inner.push(self.chars[k + 1]);
                k += 2;
            } else {
                inner.push(c);
                k += 1;
            }
        }
        wb.subs.push(self.parse_inner(&inner)?);
        let src: String = self.chars[start..=end].iter().collect();
        wb.parts.push(Part::Dynamic(src));
        self.i = end + 1;
        Ok(())
    }

    /// `<(cmd)` and `>(cmd)`
    fn process_subst(&mut self, wb: &mut WordBuilder) -> Result<(), String> {
        let start = self.i;
        let end =
            find_close_paren(&self.chars, self.i + 2).ok_or("unterminated process substitution")?;
        let inner: String = self.chars[self.i + 2..end].iter().collect();
        wb.subs.push(self.parse_inner(&inner)?);
        let src: String = self.chars[start..=end].iter().collect();
        wb.parts.push(Part::Dynamic(src));
        self.i = end + 1;
        Ok(())
    }

    fn parse_inner(&self, src: &str) -> Result<Node, String> {
        if self.depth + 1 > MAX_DEPTH {
            return Err("substitutions nested too deep".into());
        }
        parse_program(src, self.depth + 1)
    }

    /// Body of `$'...'`, `self.i` just after the opening quote.
    fn ansi_c(&mut self) -> Result<String, String> {
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or("unterminated $'")?;
            self.i += 1;
            match c {
                '\'' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or("unterminated $'")?;
                    self.i += 1;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'a' => out.push('\x07'),
                        'b' => out.push('\x08'),
                        'e' | 'E' => out.push('\x1b'),
                        'f' => out.push('\x0c'),
                        'v' => out.push('\x0b'),
                        '\\' | '\'' | '"' | '?' => out.push(e),
                        'x' => out.push(self.hex_escape(2)),
                        'u' => out.push(self.hex_escape(4)),
                        'U' => out.push(self.hex_escape(8)),
                        '0'..='7' => {
                            let mut v = e.to_digit(8).unwrap_or(0);
                            for _ in 0..2 {
                                match self.peek().and_then(|c| c.to_digit(8)) {
                                    Some(d) => {
                                        v = v * 8 + d;
                                        self.i += 1;
                                    }
                                    None => break,
                                }
                            }
                            out.push(char::from_u32(v).unwrap_or('\u{fffd}'));
                        }
                        'c' => {
                            if let Some(x) = self.peek() {
                                self.i += 1;
                                out.push(char::from_u32((x as u32) & 0x1f).unwrap_or('\u{fffd}'));
                            }
                        }
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                }
                _ => out.push(c),
            }
        }
    }

    fn hex_escape(&mut self, max: usize) -> char {
        let mut v = 0u32;
        let mut n = 0;
        while n < max {
            match self.peek().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    v = v.wrapping_mul(16).wrapping_add(d);
                    self.i += 1;
                    n += 1;
                }
                None => break,
            }
        }
        if n == 0 {
            return '\u{fffd}';
        }
        char::from_u32(v).unwrap_or('\u{fffd}')
    }
}

// ------------------------------------------------------------- bracket scanners
// All take the index just after the opening delimiter and return the index of the closer.

fn find_close_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => i = skip_dq(chars, i + 1)?,
            '`' => i = find_backtick_end(chars, i + 1)?,
            '#' if i == start
                || matches!(chars[i - 1], ' ' | '\t' | '\n' | ';' | '&' | '|' | '(') =>
            {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_close_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => i = skip_dq(chars, i + 1)?,
            '`' => i = find_backtick_end(chars, i + 1)?,
            '$' if chars.get(i + 1) == Some(&'(') => i = find_close_paren(chars, i + 2)?,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index of the closing `"`, `start` just after the opening one.
fn skip_dq(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '"' => return Some(i),
            '`' => i = find_backtick_end(chars, i + 1)?,
            '$' if chars.get(i + 1) == Some(&'(') => i = find_close_paren(chars, i + 2)?,
            '$' if chars.get(i + 1) == Some(&'{') => i = find_close_brace(chars, i + 2)?,
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_backtick_end(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '`' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}
