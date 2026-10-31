//! The fixed tool set (design 5): definitions for MCP and the handlers behind them.
//!
//! No tools come from config, so a server can't change what a tool means after the fact
//! (no "rug pull"). Remote text only ever appears in fields named `untrusted_*`, next to a
//! `notice`; it is never placed in a description or anything else bssh controls.

mod convert;
mod definitions;
mod dispatch;
mod exec;
mod hosts;
mod inputs;
mod names;
mod outputs;
mod read;
mod run_many;
mod run_state;

pub use definitions::definitions;
pub use dispatch::call;
pub use exec::{MAX_COMMAND_BYTES, run_readonly_on};
pub use inputs::{
    ByteRange, ChangeIdArgs, EditFileArgs, HostArgs, ListHostsArgs, NoArgs, ObserveArgs,
    ReadFileArgs, RequestAccessArgs, RunArgs, RunManyArgs, ScopeArg, SearchLogsArgs, ServiceArgs,
    WaitApprovalArgs,
};
pub use names::{ALL_TOOLS, name_str, parse_tool};
pub use outputs::{
    AccessOut, ApplyOut, EditOut, EnforcementOut, ExecOut, FileOut, HostEntry, HostInfoOut,
    HostListOut, NOTICE, ObserveOut, ProposeOut, RedactionOut, RunManyEntry, RunManyOut,
    ServiceOut, StatusOut, UndoOut, WaitApprovalOut,
};
pub use read::{READ_DEFAULT, READ_MAX, log_command};
