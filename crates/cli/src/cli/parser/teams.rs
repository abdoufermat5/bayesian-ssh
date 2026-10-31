//! `bssh teams` subcommands.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum TeamsCommands {
    /// Set up this machine: create your account and this device's keys (verifies your email when asked)
    Init {
        /// Sync server URL (https; http only for localhost)
        #[arg(long, value_name = "URL")]
        server: String,
        /// Your email address (invites are sent to it)
        #[arg(long, value_name = "EMAIL")]
        email: String,
        /// Name for this device, shown to team admins (default: hostname)
        #[arg(long, value_name = "NAME")]
        device_name: Option<String>,
        /// Invite token from your admin (servers that only accept invited accounts need it)
        #[arg(long, value_name = "TOKEN")]
        invite: Option<String>,
        /// Verification code the server emailed you (when it asks to verify your email)
        #[arg(long, value_name = "CODE")]
        email_code: Option<String>,
    },
    /// Show this machine's account, device and fingerprint
    Whoami,
    /// Create a team; you become its admin
    Create {
        /// Team name, also used as its source name in `bssh list`
        name: String,
        /// Organization to create it in: an id prefix or one of its team names
        /// (default: the oldest organization you own, or a new one)
        #[arg(long, value_name = "ORG")]
        org: Option<String>,
    },
    /// List the teams on this machine
    List,
    /// Invite someone by email (admin); the server mails the token when it can
    Invite {
        team: String,
        email: String,
        /// Make them an admin instead of a member
        #[arg(long)]
        admin: bool,
    },
    /// Revoke a pending invite (admin): its token stops working and its seat is free again
    Uninvite { team: String, email: String },
    /// Join a team with an invite token and the admin's device fingerprint
    Join {
        /// Invite token (single use)
        #[arg(required_unless_present = "team")]
        token: Option<String>,
        /// Finish a join whose fingerprint check failed, by team id
        #[arg(long, value_name = "TEAM_ID", conflicts_with = "token")]
        team: Option<uuid::Uuid>,
        /// The admin's fingerprint, received with the invite
        #[arg(long, value_name = "SHA256:...")]
        fingerprint: String,
    },
    /// Give the team key to a waiting device (admin); lists them without --fingerprint
    Grant {
        team: String,
        /// Fingerprint the member sent you after joining
        #[arg(long, value_name = "SHA256:...")]
        fingerprint: Option<String>,
    },
    /// Sync a team, or every team
    Sync {
        /// Team to sync (default: all)
        team: Option<String>,
    },
    /// Share a saved connection with a team (or push your edits to a team connection)
    Share {
        /// Connection name or alias
        connection: String,
        #[arg(long)]
        team: String,
    },
    /// Delete a team connection for everyone in the team
    Unshare {
        /// Connection name or alias
        connection: String,
    },
    /// List a team's members and their devices
    Members { team: String },
    /// Show the plan and seats of a team's organization; subscribe or manage the subscription (organization owner)
    #[command(after_help = "Examples:\n\
      bssh teams billing platform                       plan, seats, renewal\n\
      bssh teams billing platform --checkout --seats 8  subscribe for 8 seats (monthly)\n\
      bssh teams billing platform --checkout --yearly   yearly, as many seats as in use\n\
      bssh teams billing platform --manage              card, invoices, seats, cancel")]
    Billing {
        team: String,
        /// Open a checkout page to subscribe the team's organization (owner)
        #[arg(long, conflicts_with = "manage")]
        checkout: bool,
        /// Seats to subscribe for (default: the seats in use, at least one above the free plan)
        #[arg(long, requires = "checkout")]
        seats: Option<u32>,
        /// Pay yearly instead of monthly
        #[arg(long, requires = "checkout")]
        yearly: bool,
        /// Open the billing portal: card, invoices, seats, cancellation (owner)
        #[arg(long)]
        manage: bool,
    },
    /// Remove a member and rotate the team key (admin)
    Remove { team: String, email: String },
    /// Show and verify the team's audit log (admin). Exits non-zero if anything fails to verify
    Audit {
        team: String,
        /// Only events for this team connection
        #[arg(long, value_name = "CONNECTION")]
        host: Option<String>,
        /// Only events by this member
        #[arg(long, value_name = "EMAIL")]
        member: Option<String>,
        /// Only events received since this date (YYYY-MM-DD or RFC 3339)
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// How many of the newest events to show (default: 100). The whole log is verified either way
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
        /// Show every matching event instead of the newest ones
        #[arg(long, conflicts_with = "limit")]
        all: bool,
    },
    /// Devices of your account, or of a team's members (admin): revoke one
    Device {
        #[command(subcommand)]
        command: TeamsDeviceCommands,
    },
    /// Agent identities for unattended AI agents (admin): `bssh ai serve` runs them
    Agent {
        #[command(subcommand)]
        command: TeamsAgentCommands,
    },
}

#[derive(Subcommand)]
pub enum TeamsDeviceCommands {
    /// Revoke a device for good: it is locked out at once and every team its account is in needs a new team key
    #[command(
        after_help = "The device id is shown by `bssh teams whoami` (this device) and\n\
      `bssh teams members <team>` (a member's devices, admin).\n\
      Allowed for the device's own account, and for an admin of a team its account is in."
    )]
    Revoke {
        /// Id of the device (a full UUID)
        device_id: uuid::Uuid,
    },
}

#[derive(Subcommand)]
pub enum TeamsAgentCommands {
    /// Create an agent: its own device keys, a seat, a signed policy and a credential file
    Add {
        team: String,
        name: String,
        /// Policy from `ai.policies` in config.json, signed for the agent (default: `default`)
        #[arg(long, value_name = "NAME")]
        policy: Option<String>,
    },
    /// List a team's agents
    List { team: String },
    /// Remove an agent: frees its seat and rotates the team key
    Remove { team: String, name: String },
    /// Re-sign an agent's policy from config.json
    Policy {
        team: String,
        name: String,
        #[arg(long, value_name = "NAME", default_value = "default")]
        policy: String,
    },
    /// List approval requests from agents waiting for you
    Pending { team: String },
    /// Approve a pending request (a prefix of its id is enough)
    Approve {
        team: String,
        request: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Deny a pending request
    Deny {
        team: String,
        request: String,
        #[arg(long)]
        reason: Option<String>,
    },
}
