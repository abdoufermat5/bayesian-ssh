//! Shell AST. Only what the analyzer needs: words with their expansions, simple
//! commands with redirections, and the compound constructs that contain them.

/// One piece of a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Literal text. `quoted`: it came from quotes or a backslash (no globbing, no braces).
    Lit { text: String, quoted: bool },
    /// An expansion we can't resolve statically: `$x`, `${x}`, `$(...)`, `` `...` ``, `$((...))`.
    /// Holds the source text.
    Dynamic(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    pub parts: Vec<Part>,
    /// Commands found inside the word's expansions (`$(...)`, backticks, `<(...)`, `${x:-$(...)}`).
    pub subs: Vec<Node>,
}

impl Word {
    /// The word as the shell sees it after quote removal. Expansions keep their source text.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Lit { text, .. } => out.push_str(text),
                Part::Dynamic(src) => out.push_str(src),
            }
        }
        out
    }

    pub fn is_dynamic(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Dynamic(_)))
    }

    /// Reserved-word candidate: one unquoted literal part.
    pub fn reserved(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [Part::Lit {
                text,
                quoted: false,
            }] => Some(text.as_str()),
            _ => None,
        }
    }

    /// `NAME=value` or `NAME+=value` written without quotes around the name.
    pub fn split_assignment(&self) -> Option<(String, Word)> {
        let (first, rest) = self.parts.split_first()?;
        let Part::Lit {
            text,
            quoted: false,
        } = first
        else {
            return None;
        };
        let eq = text.find('=')?;
        let name = text[..eq].strip_suffix('+').unwrap_or(&text[..eq]);
        let mut chars = name.chars();
        let head = chars.next()?;
        if !(head.is_ascii_alphabetic() || head == '_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return None;
        }
        let mut value = Word {
            parts: Vec::new(),
            subs: self.subs.clone(),
        };
        let tail = &text[eq + 1..];
        if !tail.is_empty() {
            value.parts.push(Part::Lit {
                text: tail.to_string(),
                quoted: false,
            });
        }
        value.parts.extend(rest.iter().cloned());
        Some((name.to_string(), value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirKind {
    /// `<`
    In,
    /// `>`
    Out,
    /// `>>`
    Append,
    /// `>|`
    Clobber,
    /// `<>`
    ReadWrite,
    /// `<&`
    DupIn,
    /// `>&`
    DupOut,
    /// `<<` and `<<-`
    HereDoc,
    /// `<<<`
    HereString,
    /// `&>`
    OutErr,
    /// `&>>`
    AppendErr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redir {
    pub fd: Option<u32>,
    pub kind: RedirKind,
    /// File, fd, or here-doc delimiter.
    pub target: Word,
    /// Commands inside a here-doc body (unquoted delimiter only).
    pub subs: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assign {
    pub name: String,
    pub value: Word,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleCmd {
    pub assigns: Vec<Assign>,
    pub words: Vec<Word>,
    pub redirs: Vec<Redir>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// `{ ...; }`
    Brace,
    /// `( ... )`
    Subshell,
    If,
    Loop,
    Case,
    /// `name() { ...; }` (the body is analyzed, calls are unknown programs)
    Function,
    /// `(( ... ))` and `for (( ... ))`
    Arith,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Simple(SimpleCmd),
    /// `a | b | c`
    Pipeline(Vec<Node>),
    /// Lists, `&&`/`||` chains: operators don't matter for risk.
    Seq(Vec<Node>),
    Group {
        kind: GroupKind,
        body: Vec<Node>,
        /// Words that are not commands: `for` items, `case` subject and patterns, `(( ))` content.
        words: Vec<Word>,
        redirs: Vec<Redir>,
    },
}
