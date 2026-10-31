//! Policy engine: scope, deny > ask > allow, tiers, grants, budgets.
//!
//! Order of evaluation in [`DefaultPolicyEngine::decide`](crate::services::ai::types::PolicyEngine::decide) (first match wins, so deny always
//! beats ask and ask beats allow):
//!
//! 1. host out of scope, or the call has no host when it needs one: deny (counts as probing)
//! 2. budgets (`calls_per_min`, `changes_per_run`, `run_ttl`): deny
//! 3. profile `observe` and a write tool: deny
//! 4. `deny_paths`: the tool's path, then every path the command names as written — arguments
//!    (absolute, relative, globbed, `~`/`$HOME`, `--opt=` and attached-option values) and
//!    redirection targets read from or written to, with relative arguments resolved against a
//!    literal `cd`: a certain match denies
//! 5. `deny` patterns on the analyzed commands: deny
//! 6. a path `deny_paths` could not settle (a glob, or a path only known at run time): ask
//! 7. `write_paths` set and `edit_file` outside it: deny
//! 8. an active grant matches: allow
//! 9. read tools: allow (`read_file` only under `read_paths`, else ask);
//!    `run_readonly` / `run_many`: allow when the jail runs as the low-privilege agent
//!    principal (so file permissions enforce `deny_paths`), or the analysis is read-only,
//!    else ask
//! 10. write tools: ask, with the tier from the approval rules
//!
//! Every reason starts with the rule that produced it (`scope:`, `budget.calls_per_min:`...).
//!
//! # Which tags count
//!
//! A tag can relax a decision (`scope.tags`, `approval.in_client`) only when the policy
//! trusts it: the connection was added by hand (`source_id` is `None`), or its inventory
//! source is named in `ai.trusted_tag_sources`. Teams vaults and cloud inventories hold tags
//! anyone with write access there can change, so by default they can only tighten: every tag
//! counts for `approval.two_person` and `approval.two_person_hosts`. Host globs
//! (`scope.hosts`) are unaffected.
//!
//! # Layout
//!
//! - [`engine`]: [`DefaultPolicyEngine`] and the order of evaluation
//! - [`profile`]: built-in profiles and policy resolution
//! - [`scope`]: which hosts the agent sees and which tags count
//! - [`glob`], [`deny_paths`], [`candidates`]: path and glob matching for `deny_paths`
//! - [`commands`]: text matching of `deny` patterns
//! - [`tier`], [`grants`], [`budget`]: approval tiers, grants, budgets and the breaker

mod budget;
mod candidates;
mod commands;
mod deny_paths;
mod engine;
mod glob;
mod grants;
mod profile;
mod scope;
mod tier;
mod validation;

#[cfg(test)]
mod tests;

pub use budget::{circuit_breaker, parse_duration, PROBE_LIMIT, PROBE_WINDOW};
pub use engine::DefaultPolicyEngine;
pub use profile::{builtin_profile, resolve};
pub use scope::{connection_facts, host_in_scope, tags_trusted, TagTrust, ALL_HOSTS};
pub use tier::tier_for_call;
pub use validation::{validation_warnings, validation_warnings_in};
