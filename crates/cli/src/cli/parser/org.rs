//! `bssh org` subcommands.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum OrgCommands {
    /// List the organizations you can see
    List,
    /// Show an organization's owners, teams and seats
    Show {
        /// Organization id prefix or one of its team names (default: your only organization)
        org: Option<String>,
    },
    /// Create an empty organization; one is free per account
    Create,
    /// Manage an organization's owners (owner)
    Owners {
        #[command(subcommand)]
        command: OrgOwnerCommands,
    },
    /// Show an organization's plan and seats; subscribe or manage the subscription (owner)
    #[command(after_help = "Examples:\n\
      bssh org billing                                  plan, seats, renewal\n\
      bssh org billing --checkout --seats 8             subscribe for 8 seats (monthly)\n\
      bssh org billing --checkout --yearly              yearly, as many seats as in use\n\
      bssh org billing --manage                         card, invoices, seats, cancel")]
    Billing {
        /// Organization id prefix or one of its team names (default: your only organization)
        org: Option<String>,
        /// Open a checkout page to subscribe the organization (owner)
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
}

#[derive(Subcommand)]
pub enum OrgOwnerCommands {
    /// Make a member of one of its teams an owner (owner)
    Add { org: String, email: String },
    /// Remove an owner (owner); the last owner can't be removed
    Remove { org: String, email: String },
}
