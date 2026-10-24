//! Shell command analyzer: POSIX AST, wrapper unwrapping, rule database, risk tags.
//!
//! Owner: W2. Stub from the scaffold; replace freely.

use super::types::{Analysis, Analyzer, RiskTag};

#[derive(Debug, Default, Clone)]
pub struct ShellAnalyzer;

impl ShellAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Analyzer for ShellAnalyzer {
    fn analyze(&self, command: &str) -> Analysis {
        let _ = command;
        Analysis {
            commands: Vec::new(),
            tags: [RiskTag::Unknown].into_iter().collect(),
            unknown_reason: Some("analyzer not implemented yet".into()),
        }
    }
}
