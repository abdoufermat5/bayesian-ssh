//! Billing and organization requests, and the hints for server errors.

use super::handle::Session;
use crate::services::teams::client::TeamsError;
use anyhow::{Result, anyhow};
use bssh_teams_proto::api::{BillingInfo, BillingInterval, CheckoutRequest, ErrorCode, OrgInfo};
use uuid::Uuid;

/// Keep the server's wording for billing and organization requests, `Forbidden` and
/// `Conflict` included: those messages name the rule ("only an owner of the
/// organization can manage billing", "the organization must keep at least one
/// owner"), which the generic hints of [`explain`] would hide. Credentials and
/// version errors keep their hint.
pub(super) fn org_message(e: TeamsError) -> anyhow::Error {
    match &e {
        TeamsError::Api { error, .. }
            if !matches!(
                e.code(),
                Some(ErrorCode::Unauthorized | ErrorCode::ClientTooOld)
            ) =>
        {
            anyhow!("{}", error.message)
        }
        _ => e.into(),
    }
}

/// Map a server error to a short, actionable message for the CLI. Billing
/// and provider errors (`PaymentRequired`, `SeatLimit`, `Internal`) get no
/// hint: the server's own message names the exact fix and is shown as is.
pub fn explain(err: &anyhow::Error) -> Option<&'static str> {
    let code = err.downcast_ref::<TeamsError>()?.code()?;
    Some(match code {
        ErrorCode::Unauthorized => "the server rejected this device's credentials",
        ErrorCode::Forbidden => "you don't have permission for that in this team",
        ErrorCode::Conflict => "that already exists or was already used",
        ErrorCode::ClientTooOld => "the server needs a newer bssh; please upgrade",
        _ => return None,
    })
}

impl<'a> Session<'a> {
    /// Plan and seats of a team (any member).
    pub async fn billing(&self, team_id: Uuid) -> Result<BillingInfo> {
        self.team_info(team_id).await?;
        Ok(self.client.billing(team_id).await?)
    }

    /// A Stripe Checkout page to subscribe the team for `seats` seats (admin).
    pub async fn checkout_url(
        &self,
        team_id: Uuid,
        seats: u32,
        interval: BillingInterval,
    ) -> Result<String> {
        // No client-side role check: billing belongs to the team's organization,
        // and only the server knows who owns it (a team admin may not be an owner).
        let redirect = self
            .client
            .billing_checkout(team_id, &CheckoutRequest { seats, interval })
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    /// The Stripe Customer Portal of the team's organization: card, invoices,
    /// seats, cancellation (organization owner).
    pub async fn portal_url(&self, team_id: Uuid) -> Result<String> {
        let redirect = self
            .client
            .billing_portal(team_id)
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    /// Organizations this account owns or has a team in.
    pub async fn list_orgs(&self) -> Result<Vec<OrgInfo>> {
        Ok(self.client.list_orgs().await?)
    }

    /// Create an empty organization owned by this account. The free plan
    /// covers one per account: a second one needs a subscription.
    pub async fn create_org(&self) -> Result<OrgInfo> {
        match self.client.create_org().await {
            Ok(org) => Ok(org),
            Err(e) if e.code() == Some(ErrorCode::PaymentRequired) => Err(anyhow!(e).context(
                "one organization is free per account: subscribe to your existing one \
                 (`bssh org billing <org> --checkout`) to create another, or create teams in it",
            )),
            Err(e) => Err(org_message(e)),
        }
    }

    pub async fn org_info(&self, org_id: Uuid) -> Result<OrgInfo> {
        Ok(self.client.org(org_id).await?)
    }

    /// Add an owner (the server requires them to be in one of its teams).
    pub async fn add_org_owner(&self, org_id: Uuid, email: &str) -> Result<OrgInfo> {
        let email = email.trim().to_lowercase();
        self.client
            .add_org_owner(org_id, &email)
            .await
            .map_err(org_message)
    }

    /// Remove an owner, resolving the email to an account id from the
    /// organization's owner list (the server takes an account id).
    pub async fn remove_org_owner(&self, org_id: Uuid, email: &str) -> Result<OrgInfo> {
        let email = email.trim().to_lowercase();
        let org = self.client.org(org_id).await?;
        let account_id = org
            .owners
            .iter()
            .find(|o| o.email == email)
            .map(|o| o.account_id)
            .ok_or_else(|| anyhow!("{email} is not an owner of this organization"))?;
        self.client
            .remove_org_owner(org_id, account_id)
            .await
            .map_err(org_message)?;
        // As it is now: callers label the organization by its remaining owners.
        Ok(self.client.org(org_id).await?)
    }

    /// Plan and seats of an organization (any member).
    pub async fn org_billing(&self, org_id: Uuid) -> Result<BillingInfo> {
        Ok(self.client.org_billing(org_id).await?)
    }

    /// A provider checkout page for the organization's subscription (owner).
    pub async fn org_checkout_url(
        &self,
        org_id: Uuid,
        seats: u32,
        interval: BillingInterval,
    ) -> Result<String> {
        let redirect = self
            .client
            .org_billing_checkout(org_id, &CheckoutRequest { seats, interval })
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }

    /// The organization's customer portal: card, invoices, seats, cancellation (owner).
    pub async fn org_portal_url(&self, org_id: Uuid) -> Result<String> {
        let redirect = self
            .client
            .org_billing_portal(org_id)
            .await
            .map_err(org_message)?;
        Ok(redirect.url)
    }
}
