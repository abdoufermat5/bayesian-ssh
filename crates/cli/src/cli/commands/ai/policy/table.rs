//! The aligned result table of `bssh ai policy test` and its summary.

use super::test_file::{
    count_mismatches, decision_kind, decision_reason, expected_label, is_mismatch,
    ExpectedDecision, TestOutcome,
};
use crate::services::ai::types::Decision;
use crate::services::ai::ui::{self, Glyph, Style, Tone};

fn test_badge(style: &Style, expect: Option<ExpectedDecision>, got: &Decision) -> String {
    match expect {
        Some(_) => {
            if is_mismatch(expect, got) {
                style.paint(Tone::Bad, &format!("{} fail", style.glyph(Glyph::Bad)))
            } else {
                style.paint(Tone::Ok, &format!("{} pass", style.glyph(Glyph::Ok)))
            }
        }
        None => style.paint(Tone::Dim, "-"),
    }
}

/// Aligned table of the evaluated lines and a final summary.
pub(super) fn render_test_table(style: &Style, rows: &[TestOutcome]) -> String {
    let line_width = rows
        .iter()
        .map(|row| row.line_no.to_string().len())
        .max()
        .unwrap_or(1)
        .max(1);

    let term_width = ui::width();
    let badge_width = 6;
    let expect_width = 6;
    let got_width = 5;

    let fixed_overhead = line_width + 2 + badge_width + 2 + expect_width + 2 + got_width + 2 + 25;
    let max_cmd_w = term_width.saturating_sub(fixed_overhead).clamp(25, 70);

    let command_width = rows
        .iter()
        .map(|row| ui::display_width(&ui::truncate(&row.command, max_cmd_w)))
        .max()
        .unwrap_or(0)
        .max(7);

    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}  {}  {}  {}  {}\n",
        ui::pad(&style.label("#"), line_width),
        ui::pad(&style.label("result"), badge_width),
        ui::pad(&style.label("expect"), expect_width),
        ui::pad(&style.label("got"), got_width),
        ui::pad(&style.label("command"), command_width),
        style.label("reason")
    ));

    for row in rows {
        let badge = test_badge(style, row.expect, &row.got);
        let cmd = ui::truncate(&row.command, max_cmd_w);
        let reason = style.paint(Tone::Dim, &format!("({})", decision_reason(&row.got)));
        out.push_str(&format!(
            "{}  {}  {}  {}  {}  {}\n",
            ui::pad(&row.line_no.to_string(), line_width),
            ui::pad(&badge, badge_width),
            ui::pad(expected_label(row.expect), expect_width),
            ui::pad(decision_kind(&row.got), got_width),
            ui::pad(&cmd, command_width),
            reason,
        ));
    }

    let mismatches = count_mismatches(rows);
    let summary = if mismatches == 0 {
        format!(
            "{} {} commands, 0 mismatches\n",
            style.mark(Glyph::Ok),
            rows.len()
        )
    } else {
        format!(
            "{} {} commands, {} mismatch(es)\n",
            style.mark(Glyph::Bad),
            rows.len(),
            mismatches
        )
    };
    out.push_str(&summary);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::ai::policy::test_support::{allow, ask, deny, outcome};

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(ui::truncate("abc", 3), "abc");
        assert_eq!(ui::truncate("abcd", 3), "ab…");
        assert_eq!(
            ui::truncate(&"é".repeat(61), 60),
            format!("{}…", "é".repeat(59))
        );
    }

    #[test]
    fn truncate_keeps_exactly_sixty() {
        let command = "x".repeat(60);
        assert_eq!(ui::truncate(&command, 60), command);
    }

    #[test]
    fn table_has_aligned_columns() {
        let rows = vec![
            outcome(
                1,
                "ls -la",
                Some(ExpectedDecision::Allow),
                allow("read only"),
            ),
            outcome(
                2,
                "systemctl restart nginx",
                Some(ExpectedDecision::Deny),
                ask("needs approval"),
            ),
            outcome(3, "rm -rf /", None, deny("destructive")),
        ];
        let text = render_test_table(&Style::plain(), &rows);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].starts_with("#  RESULT  EXPECT  GOT    COMMAND"));
        assert!(lines[0].ends_with("REASON"));
        assert!(lines[1].starts_with("1  ✓ pass  allow   allow  ls -la"));
        assert!(lines[2].starts_with("2  ✗ fail  deny    ask    systemctl restart nginx"));
        assert!(lines[3].starts_with("3  -       -       deny   rm -rf /"));
        assert_eq!(lines[4], "✗ 3 commands, 1 mismatch(es)");

        let reason_col = ui::display_width(&lines[1][..lines[1].rfind('(').unwrap()]);
        for line in &lines[1..4] {
            assert_eq!(
                ui::display_width(&line[..line.rfind('(').unwrap()]),
                reason_col
            );
        }
    }

    #[test]
    fn table_shows_reasons() {
        let rows = vec![outcome(1, "ls", None, allow("within read paths"))];
        let text = render_test_table(&Style::plain(), &rows);
        assert!(text.contains("(within read paths)"));
        assert!(text.contains("1 commands, 0 mismatches"));
    }

    #[test]
    fn table_handles_no_rows() {
        let text = render_test_table(&Style::plain(), &[]);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("#  RESULT  EXPECT  GOT"));
        assert_eq!(lines[1], "✓ 0 commands, 0 mismatches");
    }

    #[test]
    fn table_truncates_long_commands() {
        let command = "a".repeat(120);
        let rows = vec![outcome(1, &command, None, allow("r"))];
        let text = render_test_table(&Style::plain(), &rows);
        assert!(text.contains('…'));
        assert!(!text.contains(&"a".repeat(100)));
    }

    #[test]
    fn table_colored_has_ansi_escapes() {
        let rows = vec![
            outcome(1, "ls", Some(ExpectedDecision::Allow), allow("ok")),
            outcome(2, "rm", Some(ExpectedDecision::Allow), deny("denied")),
        ];
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_test_table(&style, &rows);
        assert!(text.contains("\x1b[32m✓ pass\x1b[0m"));
        assert!(text.contains("\x1b[31m✗ fail\x1b[0m"));
    }
}
