//! AST walker: finds every simple command, peels wrappers (`sudo`, `env`, `sh -c`...),
//! and asks the rule table what each one does.

mod command;
mod exec;
mod opts;
mod privilege;
mod state;
mod tree;
mod words;
mod wrappers;

pub use state::walk;
