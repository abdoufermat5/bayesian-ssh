//! Team billing: the plan summary, the checkout page and the browser helper
//! shared with `bssh org`.

use super::passphrase::{Passphrase, passphrase};
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::billing::{self, ReadOnlyReason};
use crate::services::teams::orgs;
use crate::services::teams::session::{self, Session};
use anyhow::Result;
use bssh_teams_proto::api::{BillingInfo, BillingInterval, Plan};

/// Which page `bssh teams billing` opens instead of printing the plan.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BillingPage {
    /// Subscribe the team's organization, for `seats` at `interval`.
    Checkout,
    /// The customer portal: card, invoices, seats, cancellation.
    Manage,
}

/// The arguments of `bssh teams billing`.
pub(super) struct BillingRequest<'a> {
    pub team: &'a str,
    /// Open a page instead of printing the plan.
    pub open: Option<BillingPage>,
    /// Seats to subscribe for; defaults to the seats in use, at least one above
    /// the free plan.
    pub seats: Option<u32>,
    /// Billing interval for [`BillingPage::Checkout`].
    pub interval: BillingInterval,
}

/// Prints a team's plan and seats, or opens a billing page.
///
/// # Errors
/// Fails when the team cannot be resolved, the billing state cannot be fetched,
/// or the server returns no page URL.
pub(super) async fn execute(
    store: &TeamsStore,
    config: &AppConfig,
    request: BillingRequest<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, request.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    if let Some(page) = request.open {
        let url = match page {
            BillingPage::Manage => s.portal_url(team_id).await?,
            BillingPage::Checkout => {
                let info = s.billing(team_id).await?;
                let seats = request
                    .seats
                    .unwrap_or_else(|| info.seats_used.max(info.free_seats + 1));
                s.checkout_url(team_id, seats, request.interval).await?
            }
        };
        validate_browser_url(&url)?;
        println!(
            "Opening the billing page for '{}' in your browser:",
            source.name
        );
        println!("  {url}");
        open_in_browser(&url);
        return Ok(());
    }
    let info = s.billing(team_id).await?;
    print_billing(
        &source.name,
        &info,
        &format!("bssh teams billing {}", source.name),
    );
    if let Some(org_id) = info.org_id {
        println!();
        println!(
            "   Seats are shared by this team's organization ({}); see `bssh org billing`.",
            orgs::short_id(&org_id)
        );
    }
    Ok(())
}

pub(crate) fn print_billing(label: &str, info: &BillingInfo, hint: &str) {
    if !info.billing_enabled {
        println!(
            "💳 {label}: {} of {} seats used (this server has no billing)",
            info.seats_used, info.seat_limit
        );
        return;
    }
    let plan = match info.plan {
        Plan::Free => format!("Free plan, {} seats", info.free_seats),
        Plan::Team => format!(
            "Team plan, {} seats{}",
            info.paid_seats,
            match info.interval {
                Some(BillingInterval::Year) => ", billed yearly",
                Some(BillingInterval::Month) => ", billed monthly",
                None => "",
            }
        ),
    };
    println!("💳 {label}: {plan}");
    println!(
        "   {} of {} seats used (members and pending invites)",
        info.seats_used, info.seat_limit
    );
    if let Some(end) = info.current_period_end.filter(|_| info.plan == Plan::Team) {
        let date = end.format("%Y-%m-%d");
        if info.cancel_at_period_end {
            println!("   Ends on {date}, then it falls back to the free plan.");
        } else {
            println!("   Renews on {date}");
        }
    }
    match info.status.as_deref() {
        Some("past_due") => println!(
            "   ⚠️  Payment failed, Stripe is retrying: update the card with `{hint} --manage`."
        ),
        Some(status) if !billing::is_live(Some(status)) => {
            println!("   Subscription status: {status}")
        }
        _ => {}
    }
    match billing::read_only_reason(info) {
        Some(ReadOnlyReason::NotEnoughSeats) => {
            println!();
            println!(
                "⚠️  Read-only: more members than paid seats ({} paid).",
                info.paid_seats
            );
            println!(
                "   Members can still sync and connect; changes are refused until an admin adds seats"
            );
            println!("   (`{hint} --manage`) or removes members.");
        }
        Some(ReadOnlyReason::NoSubscription) => {
            println!();
            println!(
                "⚠️  Read-only: no active subscription, and more members than the {} free seats.",
                info.free_seats
            );
            println!(
                "   Members can still sync and connect; changes are refused until an admin subscribes"
            );
            println!("   (`{hint} --checkout`) or removes members down to the free seats.");
        }
        None if info.plan == Plan::Free && info.seats_used >= info.seat_limit => {
            println!();
            println!("   All free seats are taken. More: `{hint} --checkout`.");
        }
        None => {}
    }
}

/// Validates that `url` is a well-formed `https://` URL with a host and no
/// control characters or whitespace before it is handed to the OS browser launcher.
pub(crate) fn validate_browser_url(url: &str) -> Result<reqwest::Url> {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        anyhow::bail!("refusing to open URL containing control characters or whitespace");
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| anyhow::anyhow!("refusing to open invalid billing URL: {e}"))?;
    if parsed.scheme() != "https" || !parsed.has_host() {
        anyhow::bail!("refusing to open non-HTTPS billing URL");
    }
    Ok(parsed)
}

/// Opens a URL in the default browser; the URL is printed first, so this may fail quietly.
pub(crate) fn open_in_browser(url: &str) {
    let Ok(parsed) = validate_browser_url(url) else {
        return;
    };
    let safe_url = parsed.as_str();
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg("--").arg(safe_url);
        c
    } else if cfg!(target_os = "windows") {
        // Avoid `cmd /C start` which interprets shell metacharacters (&|^%<>!").
        let mut c = std::process::Command::new("rundll32.exe");
        c.args(["url.dll,FileProtocolHandler", safe_url]);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg("--").arg(safe_url);
        c
    };
    let _ = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::validate_browser_url;

    #[test]
    fn accepts_valid_https_urls() {
        assert!(
            validate_browser_url("https://checkout.stripe.com/c/pay/cs_test_123?a=1#frag").is_ok()
        );
        assert!(validate_browser_url("https://billing.example.com/portal").is_ok());
    }

    #[test]
    fn rejects_non_https_and_malformed_urls() {
        for bad in [
            "",
            "http://example.com/billing",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "-o",
            "--help",
            "https://example.com/a b",
            "https://example.com/a\nb",
            "https://example.com/a\rb",
            "https://example.com/\x00",
            "https://example.com/\t",
        ] {
            assert!(
                validate_browser_url(bad).is_err(),
                "expected Err for {bad:?}"
            );
        }
    }
}
