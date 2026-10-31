//! Effect vocabulary: what a command does, as verbs over targets.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    Creates,
    /// Creates directories (`mkdir`, `install -d`); worded "creates".
    CreatesDirs,
    Moves,
    Copies,
    Deletes,
    Edits,
    /// Overwrites a block or character device (`dd of=/dev/sda`).
    WritesDevice,
    /// Permissions or ownership.
    Changes,
    Restarts,
    Reloads,
    Starts,
    Stops,
    Enables,
    Disables,
    Mounts,
    Unmounts,
    Installs,
    /// Package removal.
    Removes,
    /// A read that needs approval (a file outside `read_paths`).
    Reads,
    /// Anything bssh can't put a better word on.
    Runs,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Creates | Verb::CreatesDirs => "creates",
            Verb::Moves => "moves",
            Verb::Copies => "copies",
            Verb::Deletes => "deletes",
            Verb::Edits => "edits",
            Verb::WritesDevice => "writes",
            Verb::Changes => "changes",
            Verb::Restarts => "restarts",
            Verb::Reloads => "reloads",
            Verb::Starts => "starts",
            Verb::Stops => "stops",
            Verb::Enables => "enables",
            Verb::Disables => "disables",
            Verb::Mounts => "mounts",
            Verb::Unmounts => "unmounts",
            Verb::Installs => "installs",
            Verb::Removes => "removes",
            Verb::Reads => "reads",
            Verb::Runs => "runs",
        }
    }
}

/// One thing a command does: `moves /var/www/releases/2026-08-14 → /var/tmp/x/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub verb: Verb,
    /// Paths, units or packages, as written (or as expanded by `effects_for`).
    pub targets: Vec<String>,
    /// Where moves and copies go.
    pub dest: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// In command order; read-only commands are left out.
    pub items: Vec<Effect>,
    /// True whenever anything is deleted, or deleting can't be ruled out.
    pub deletes: bool,
    pub sudo: bool,
    pub network: bool,
    /// Set when the analyzer couldn't parse everything: the summary says so.
    pub not_understood: Option<String>,
}
