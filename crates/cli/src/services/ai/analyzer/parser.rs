//! Recursive-descent parser for POSIX shell (plus the bash bits agents use):
//! lists, `&&`/`||`, pipelines, subshells, groups, `if`/`for`/`while`/`until`/`case`/`select`,
//! functions, `[[ ]]`, `(( ))`, redirections and here-docs.

use super::ast::{Assign, GroupKind, Node, Redir, RedirKind, SimpleCmd, Word};
use super::lexer::{lex, Heredoc, Op, Tok, Token, MAX_DEPTH};

/// Nesting limit for compound commands (guards the recursion).
const MAX_NEST: usize = 48;

pub fn parse_program(src: &str, depth: usize) -> Result<Node, String> {
    if depth > MAX_DEPTH {
        return Err("nested too deep".into());
    }
    let lexed = lex(src, depth)?;
    let mut p = Parser {
        toks: lexed.toks,
        heredocs: lexed.heredocs,
        i: 0,
        nest: 0,
    };
    let body = p.parse_list(&[])?;
    if let Some(t) = p.toks.get(p.i) {
        return Err(format!("unexpected {}", describe(&t.tok)));
    }
    Ok(Node::Seq(body))
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Word(w) => format!("'{}'", w.text()),
        Tok::IoNumber(n) => format!("'{n}'"),
        Tok::Op(op) => format!("'{}'", op.text()),
        Tok::Newline => "newline".into(),
        Tok::HereDoc(_) => "'<<'".into(),
    }
}

struct Parser {
    toks: Vec<Token>,
    heredocs: Vec<Heredoc>,
    i: usize,
    nest: usize,
}

type Res<T> = Result<T, String>;

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.tok)
    }

    fn peek_op(&self) -> Option<Op> {
        match self.peek() {
            Some(Tok::Op(op)) => Some(*op),
            _ => None,
        }
    }

    fn peek_reserved(&self) -> Option<&str> {
        match self.peek() {
            Some(Tok::Word(w)) => w.reserved(),
            _ => None,
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.i += 1;
        }
    }

    /// Skips `;` and newlines between compound-command parts.
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline) | Some(Tok::Op(Op::Semi))) {
            self.i += 1;
        }
    }

    fn expect_reserved(&mut self, word: &str) -> Res<()> {
        if self.peek_reserved() == Some(word) {
            self.i += 1;
            Ok(())
        } else {
            Err(match self.peek() {
                Some(t) => format!("expected '{word}', found {}", describe(t)),
                None => format!("expected '{word}', found end of input"),
            })
        }
    }

    fn expect_op(&mut self, op: Op) -> Res<()> {
        if self.peek_op() == Some(op) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}'", op.text()))
        }
    }

    fn is_stop(&self, stops: &[&str]) -> bool {
        match self.peek() {
            Some(Tok::Word(w)) => w.reserved().is_some_and(|r| stops.contains(&r)),
            Some(Tok::Op(Op::RParen)) => stops.contains(&")"),
            Some(Tok::Op(Op::CaseEnd)) => stops.contains(&";;"),
            _ => false,
        }
    }

    fn parse_list(&mut self, stops: &[&str]) -> Res<Vec<Node>> {
        self.nest += 1;
        if self.nest > MAX_NEST {
            return Err("nested too deep".into());
        }
        let mut out = Vec::new();
        loop {
            while matches!(self.peek(), Some(Tok::Newline)) {
                self.i += 1;
            }
            if self.peek().is_none() || self.is_stop(stops) {
                break;
            }
            out.push(self.parse_and_or()?);
            match self.peek() {
                Some(Tok::Op(Op::Semi)) | Some(Tok::Op(Op::Amp)) | Some(Tok::Newline) => {
                    self.i += 1;
                }
                None => break,
                _ if self.is_stop(stops) => break,
                Some(t) => return Err(format!("unexpected {}", describe(t))),
            }
        }
        self.nest -= 1;
        Ok(out)
    }

    fn parse_and_or(&mut self) -> Res<Node> {
        let mut parts = vec![self.parse_pipeline()?];
        while matches!(self.peek_op(), Some(Op::AndIf) | Some(Op::OrIf)) {
            self.i += 1;
            self.skip_newlines();
            parts.push(self.parse_pipeline()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Node::Seq(parts)
        })
    }

    fn parse_pipeline(&mut self) -> Res<Node> {
        while self.peek_reserved() == Some("!") {
            self.i += 1;
        }
        let mut stages = vec![self.parse_command()?];
        while matches!(self.peek_op(), Some(Op::Pipe) | Some(Op::PipeAmp)) {
            self.i += 1;
            self.skip_newlines();
            stages.push(self.parse_command()?);
        }
        Ok(if stages.len() == 1 {
            stages.remove(0)
        } else {
            Node::Pipeline(stages)
        })
    }

    fn parse_command(&mut self) -> Res<Node> {
        if let Some(op) = self.peek_op() {
            return match op {
                Op::LParen => {
                    if self.is_arith_start() {
                        self.parse_arith()
                    } else {
                        self.i += 1;
                        let body = self.parse_list(&[")"])?;
                        self.expect_op(Op::RParen)?;
                        self.finish_group(GroupKind::Subshell, body, Vec::new())
                    }
                }
                other if is_redir_op(other) => self.parse_simple(),
                other => Err(format!("unexpected '{}'", other.text())),
            };
        }
        match self.peek_reserved().map(str::to_owned).as_deref() {
            Some("{") => {
                self.i += 1;
                let body = self.parse_list(&["}"])?;
                self.expect_reserved("}")?;
                self.finish_group(GroupKind::Brace, body, Vec::new())
            }
            Some("if") => self.parse_if(),
            Some("while") | Some("until") => self.parse_while(),
            Some("for") | Some("select") => self.parse_for(),
            Some("case") => self.parse_case(),
            Some("function") => self.parse_function_kw(),
            Some(r @ ("then" | "else" | "elif" | "fi" | "do" | "done" | "esac" | "}" | "in")) => {
                Err(format!("unexpected '{r}'"))
            }
            _ => {
                if self.is_function_def() {
                    return self.parse_function_def();
                }
                self.parse_simple()
            }
        }
    }

    fn finish_group(&mut self, kind: GroupKind, body: Vec<Node>, words: Vec<Word>) -> Res<Node> {
        let redirs = self.parse_redirs()?;
        Ok(Node::Group {
            kind,
            body,
            words,
            redirs,
        })
    }

    fn parse_if(&mut self) -> Res<Node> {
        self.expect_reserved("if")?;
        let mut body = self.parse_list(&["then"])?;
        self.expect_reserved("then")?;
        body.extend(self.parse_list(&["elif", "else", "fi"])?);
        loop {
            match self.peek_reserved() {
                Some("elif") => {
                    self.i += 1;
                    body.extend(self.parse_list(&["then"])?);
                    self.expect_reserved("then")?;
                    body.extend(self.parse_list(&["elif", "else", "fi"])?);
                }
                Some("else") => {
                    self.i += 1;
                    body.extend(self.parse_list(&["fi"])?);
                }
                _ => break,
            }
        }
        self.expect_reserved("fi")?;
        self.finish_group(GroupKind::If, body, Vec::new())
    }

    fn parse_while(&mut self) -> Res<Node> {
        self.i += 1; // while / until
        let mut body = self.parse_list(&["do"])?;
        self.expect_reserved("do")?;
        body.extend(self.parse_list(&["done"])?);
        self.expect_reserved("done")?;
        self.finish_group(GroupKind::Loop, body, Vec::new())
    }

    fn parse_for(&mut self) -> Res<Node> {
        self.i += 1; // for / select
        let mut words = Vec::new();
        if self.peek_op() == Some(Op::LParen) && self.is_arith_start() {
            // for (( init; cond; step ))
            self.skip_arith_tokens(&mut words)?;
        } else {
            match self.peek() {
                Some(Tok::Word(_)) => self.i += 1, // loop variable
                _ => return Err("expected loop variable".into()),
            }
            self.skip_newlines();
            if self.peek_reserved() == Some("in") {
                self.i += 1;
                while let Some(Tok::Word(w)) = self.peek() {
                    words.push(w.clone());
                    self.i += 1;
                }
            }
        }
        self.skip_separators();
        self.expect_reserved("do")?;
        let body = self.parse_list(&["done"])?;
        self.expect_reserved("done")?;
        self.finish_group(GroupKind::Loop, body, words)
    }

    fn parse_case(&mut self) -> Res<Node> {
        self.expect_reserved("case")?;
        let mut words = Vec::new();
        match self.peek() {
            Some(Tok::Word(w)) => {
                words.push(w.clone());
                self.i += 1;
            }
            _ => return Err("expected case subject".into()),
        }
        self.skip_newlines();
        self.expect_reserved("in")?;
        let mut body = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_reserved() == Some("esac") {
                break;
            }
            if self.peek_op() == Some(Op::LParen) {
                self.i += 1;
            }
            loop {
                match self.peek() {
                    Some(Tok::Word(w)) => {
                        words.push(w.clone());
                        self.i += 1;
                    }
                    _ => return Err("expected case pattern".into()),
                }
                if self.peek_op() == Some(Op::Pipe) {
                    self.i += 1;
                } else {
                    break;
                }
            }
            self.expect_op(Op::RParen)?;
            body.extend(self.parse_list(&[";;", "esac"])?);
            if self.peek_op() == Some(Op::CaseEnd) {
                self.i += 1;
            } else if self.peek_reserved() != Some("esac") {
                return Err("expected ';;' or 'esac'".into());
            }
        }
        self.expect_reserved("esac")?;
        self.finish_group(GroupKind::Case, body, words)
    }

    fn is_function_def(&self) -> bool {
        matches!(self.toks.get(self.i).map(|t| &t.tok), Some(Tok::Word(_)))
            && matches!(
                self.toks.get(self.i + 1).map(|t| &t.tok),
                Some(Tok::Op(Op::LParen))
            )
            && matches!(
                self.toks.get(self.i + 2).map(|t| &t.tok),
                Some(Tok::Op(Op::RParen))
            )
    }

    fn parse_function_def(&mut self) -> Res<Node> {
        self.i += 3; // name ( )
        self.skip_newlines();
        let body = self.parse_command()?;
        Ok(Node::Group {
            kind: GroupKind::Function,
            body: vec![body],
            words: Vec::new(),
            redirs: Vec::new(),
        })
    }

    fn parse_function_kw(&mut self) -> Res<Node> {
        self.i += 1; // function
        match self.peek() {
            Some(Tok::Word(_)) => self.i += 1,
            _ => return Err("expected function name".into()),
        }
        if self.peek_op() == Some(Op::LParen) {
            self.i += 1;
            self.expect_op(Op::RParen)?;
        }
        self.skip_newlines();
        let body = self.parse_command()?;
        Ok(Node::Group {
            kind: GroupKind::Function,
            body: vec![body],
            words: Vec::new(),
            redirs: Vec::new(),
        })
    }

    /// `((` with the two parens adjacent and closed by an adjacent `))`.
    fn is_arith_start(&self) -> bool {
        let (Some(a), Some(b)) = (self.toks.get(self.i), self.toks.get(self.i + 1)) else {
            return false;
        };
        if !matches!(a.tok, Tok::Op(Op::LParen))
            || !matches!(b.tok, Tok::Op(Op::LParen))
            || b.pos != a.pos + 1
        {
            return false;
        }
        let Some(outer_end) = self.matching_rparen(self.i) else {
            return false;
        };
        let Some(inner_end) = self.matching_rparen(self.i + 1) else {
            return false;
        };
        inner_end + 1 == outer_end && self.toks[outer_end].pos == self.toks[inner_end].pos + 1
    }

    /// Index of the `)` matching the `(` at `open`.
    fn matching_rparen(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (k, t) in self.toks.iter().enumerate().skip(open) {
            match t.tok {
                Tok::Op(Op::LParen) => depth += 1,
                Tok::Op(Op::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Consumes `(( ... ))`, keeping its words (they may hold expansions).
    fn skip_arith_tokens(&mut self, words: &mut Vec<Word>) -> Res<()> {
        let end = self.matching_rparen(self.i).ok_or("unterminated '(('")?;
        for t in &self.toks[self.i..=end] {
            if let Tok::Word(w) = &t.tok {
                words.push(w.clone());
            }
        }
        self.i = end + 1;
        Ok(())
    }

    fn parse_arith(&mut self) -> Res<Node> {
        let mut words = Vec::new();
        self.skip_arith_tokens(&mut words)?;
        self.finish_group(GroupKind::Arith, Vec::new(), words)
    }

    fn parse_simple(&mut self) -> Res<Node> {
        let mut cmd = SimpleCmd::default();
        loop {
            match self.peek() {
                Some(Tok::Word(w)) => {
                    let w = w.clone();
                    self.i += 1;
                    if cmd.words.is_empty() {
                        if let Some((name, value)) = w.split_assignment() {
                            cmd.assigns.push(Assign { name, value });
                            continue;
                        }
                        if w.reserved() == Some("[[") {
                            cmd.words.push(w);
                            self.parse_cond_body(&mut cmd.words)?;
                            continue;
                        }
                    }
                    cmd.words.push(w);
                }
                Some(Tok::IoNumber(_)) | Some(Tok::HereDoc(_)) => {
                    cmd.redirs.push(self.parse_redir()?)
                }
                Some(Tok::Op(op)) if is_redir_op(*op) => cmd.redirs.push(self.parse_redir()?),
                _ => break,
            }
        }
        if cmd.words.is_empty() && cmd.assigns.is_empty() && cmd.redirs.is_empty() {
            return Err(match self.peek() {
                Some(t) => format!("unexpected {}", describe(t)),
                None => "unexpected end of input".into(),
            });
        }
        Ok(Node::Simple(cmd))
    }

    /// After `[[`: everything up to `]]` is part of the test; operators are plain words.
    fn parse_cond_body(&mut self, words: &mut Vec<Word>) -> Res<()> {
        loop {
            match self.toks.get(self.i).map(|t| t.tok.clone()) {
                Some(Tok::Word(w)) => {
                    self.i += 1;
                    let end = w.reserved() == Some("]]");
                    words.push(w);
                    if end {
                        return Ok(());
                    }
                }
                Some(Tok::Op(op)) => {
                    self.i += 1;
                    words.push(literal_word(op.text()));
                }
                Some(Tok::Newline) => self.i += 1,
                Some(_) => return Err("unsupported syntax inside '[['".into()),
                None => return Err("unterminated '[['".into()),
            }
        }
    }

    fn parse_redirs(&mut self) -> Res<Vec<Redir>> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::IoNumber(_)) | Some(Tok::HereDoc(_)) => out.push(self.parse_redir()?),
                Some(Tok::Op(op)) if is_redir_op(*op) => out.push(self.parse_redir()?),
                _ => return Ok(out),
            }
        }
    }

    fn parse_redir(&mut self) -> Res<Redir> {
        let fd = if let Some(Tok::IoNumber(n)) = self.peek() {
            let n = *n;
            self.i += 1;
            Some(n)
        } else {
            None
        };
        match self.peek().cloned() {
            Some(Tok::HereDoc(idx)) => {
                self.i += 1;
                let hd = &self.heredocs[idx];
                Ok(Redir {
                    fd,
                    kind: RedirKind::HereDoc,
                    target: literal_word(&hd.delim),
                    subs: hd.subs.clone(),
                })
            }
            Some(Tok::Op(op)) if is_redir_op(op) => {
                self.i += 1;
                let Some(Tok::Word(target)) = self.peek().cloned() else {
                    return Err(format!("missing target after '{}'", op.text()));
                };
                self.i += 1;
                Ok(Redir {
                    fd,
                    kind: redir_kind(op),
                    target,
                    subs: Vec::new(),
                })
            }
            _ => Err("expected redirection".into()),
        }
    }
}

fn literal_word(text: &str) -> Word {
    Word {
        parts: vec![super::ast::Part::Lit {
            text: text.to_string(),
            quoted: true,
        }],
        subs: Vec::new(),
    }
}

fn is_redir_op(op: Op) -> bool {
    matches!(
        op,
        Op::Less
            | Op::Great
            | Op::DGreat
            | Op::Clobber
            | Op::LessGreat
            | Op::LessAnd
            | Op::GreatAnd
            | Op::AndGreat
            | Op::AndDGreat
            | Op::TLess
    )
}

fn redir_kind(op: Op) -> RedirKind {
    match op {
        Op::Less => RedirKind::In,
        Op::Great => RedirKind::Out,
        Op::DGreat => RedirKind::Append,
        Op::Clobber => RedirKind::Clobber,
        Op::LessGreat => RedirKind::ReadWrite,
        Op::LessAnd => RedirKind::DupIn,
        Op::GreatAnd => RedirKind::DupOut,
        Op::AndGreat => RedirKind::OutErr,
        Op::AndDGreat => RedirKind::AppendErr,
        _ => RedirKind::HereString,
    }
}
