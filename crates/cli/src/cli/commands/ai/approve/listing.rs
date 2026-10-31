//! One aligned line per pending request, for `--list` and the picker.

use super::canvas::Canvas;
use super::words::{expiry_text, kind_word, request_host, request_title};
use crate::services::ai::control_client::ControlClient;
use crate::services::ai::types::ApprovalRequest;
use crate::services::ai::ui::{self, Glyph, Tone};
use chrono::{DateTime, Utc};

/// The pending requests and the runs they came from.
#[derive(Clone, Copy)]
pub(super) struct Pending<'a> {
    pub(super) clients: &'a [ControlClient],
    pub(super) all: &'a [(usize, ApprovalRequest)],
}

/// One aligned line per pending request: mark, short id, kind, host, title, client, expiry.
pub(super) fn summary_lines(
    canvas: &Canvas,
    pending: Pending<'_>,
    now: DateTime<Utc>,
) -> Vec<String> {
    let rows: Vec<[String; 6]> = pending
        .all
        .iter()
        .map(|(ci, r)| {
            let client = if r.client.is_empty() {
                pending.clients[*ci].hello.client.clone()
            } else {
                r.client.clone()
            };
            [
                ui::short_id(&r.request_id),
                kind_word(r).to_string(),
                request_host(r),
                request_title(r),
                client,
                expiry_text(r, now),
            ]
        })
        .collect();
    summary_rows(canvas, &rows)
}

fn summary_rows(canvas: &Canvas, rows: &[[String; 6]]) -> Vec<String> {
    let style = canvas.style;
    let w = canvas.width;
    let col = |i: usize| {
        rows.iter()
            .map(|r| ui::display_width(&r[i]))
            .max()
            .unwrap_or(0)
    };
    let (kindw, hostw, clientw, expw) = (col(1), col(2), col(4), col(5));
    let fixed = 2 + 8 + 2 + kindw + 2 + hostw + 2 + 2 + clientw + 2 + expw;
    let titlew = w.saturating_sub(fixed).max(10);
    rows.iter()
        .map(|r| {
            format!(
                "{} {}  {}  {}  {}  {}  {}",
                style.mark(Glyph::Ask),
                style.paint(Tone::Dim, &r[0]),
                style.paint(Tone::Dim, &ui::pad(&r[1], kindw)),
                ui::pad(&r[2], hostw),
                ui::pad(&ui::truncate(&r[3], titlew), titlew),
                style.paint(Tone::Dim, &ui::pad(&r[4], clientw)),
                style.paint(Tone::Warn, &r[5]),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{demo_plan_request, now, service_request};
    use super::*;
    use crate::services::ai::types::Tier;
    use crate::services::ai::ui::Style;

    #[test]
    fn listing_is_one_aligned_line_per_request() {
        let rows: Vec<[String; 6]> = [demo_plan_request(), service_request(Tier::OutOfBand)]
            .iter()
            .map(|r| {
                [
                    ui::short_id(&r.request_id),
                    kind_word(r).to_string(),
                    request_host(r),
                    request_title(r),
                    r.client.clone(),
                    expiry_text(r, now()),
                ]
            })
            .collect();
        let style = Style::plain();
        let canvas = Canvas::new(&style, 150);
        let lines = summary_rows(&canvas, &rows);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| !l.contains('\n')));
        assert!(
            lines.iter().all(|l| ui::display_width(l) <= 150),
            "{lines:?}"
        );
        assert!(lines[0].starts_with("◆ 440947ca  plan"));
        assert!(lines[0].contains("web-1"));
        assert!(lines[1].contains("sudo -n systemctl restart -- nginx"));
        assert!(lines[0].ends_with("expires in 4:41"));
        // Columns line up.
        assert_eq!(lines[0].find("opencode"), lines[1].find("opencode"));
    }
}
