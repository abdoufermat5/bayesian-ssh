//! `bssh ai policy`: show, write and test agent policies.

mod evaluate;
mod render;
mod run;
mod set;
mod table;
mod test_file;
#[cfg(test)]
mod test_support;

pub use run::execute;
