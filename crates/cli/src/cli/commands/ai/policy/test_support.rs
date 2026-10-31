//! Shared fixtures for the policy command tests.

use super::test_file::TestOutcome;
use crate::services::ai::types::{Decision, Tier};

pub(super) fn allow(reason: &str) -> Decision {
    Decision::Allow {
        reason: reason.to_string(),
    }
}

pub(super) fn ask(reason: &str) -> Decision {
    Decision::Ask {
        tier: Tier::OutOfBand,
        reason: reason.to_string(),
    }
}

pub(super) fn deny(reason: &str) -> Decision {
    Decision::Deny {
        reason: reason.to_string(),
    }
}

pub(super) fn outcome(
    line_no: usize,
    command: &str,
    expect: Option<super::test_file::ExpectedDecision>,
    got: Decision,
) -> TestOutcome {
    TestOutcome {
        line_no,
        command: command.to_string(),
        expect,
        got,
    }
}
