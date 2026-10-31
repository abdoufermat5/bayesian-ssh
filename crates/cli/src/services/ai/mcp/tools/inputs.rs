//! Argument structs of the tools, as the agent sends them.

use crate::services::ai::observe::What;
use crate::services::ai::plan::ServiceActionArg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListHostsArgs {
    /// Only hosts carrying this tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Substring of name, address or tag.
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostArgs {
    /// Connection name from list_hosts.
    pub host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObserveArgs {
    pub host: String,
    /// Which fact to collect.
    pub what: What,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ByteRange {
    /// First byte to return.
    pub offset: u64,
    /// Bytes to return (default 65536, max 262144).
    pub length: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFileArgs {
    pub host: String,
    /// Absolute path.
    pub path: String,
    #[serde(default)]
    pub range: Option<ByteRange>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchLogsArgs {
    pub host: String,
    /// A systemd unit (`nginx.service`), `journal` for the whole journal, or an absolute log
    /// file path (`/var/log/nginx/error.log`).
    pub source: String,
    /// Extended regular expression; matching lines only.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Journal only: `2h ago`, `2026-10-24 10:00:00`, `yesterday`.
    #[serde(default)]
    pub since: Option<String>,
    /// Last N lines (default 200, max 1000).
    #[serde(default)]
    pub lines: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunArgs {
    pub host: String,
    pub command: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunManyArgs {
    /// Every in-scope host with this tag.
    pub tag: String,
    pub command: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeIdArgs {
    pub change_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditFileArgs {
    pub host: String,
    /// Absolute path of an existing text file.
    pub path: String,
    /// Unified diff (`--- a/x`, `+++ b/x`, `@@` hunks). Must apply cleanly.
    pub patch: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceArgs {
    pub host: String,
    /// systemd unit name.
    pub unit: String,
    pub action: ServiceActionArg,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeArg {
    /// Host tags to add.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Host names to add.
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestAccessArgs {
    pub scope: ScopeArg,
    /// Why you need it. Shown to the approver labelled "the agent says".
    pub reason: String,
    /// How long, e.g. `30m` (max 8h).
    pub duration: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitApprovalArgs {
    /// The request_id from a `{status: "pending"}` result.
    pub request_id: String,
}
