//! The formats a report can be written in, and the entry points that render them.

use crate::services::ai::report::RunReport;
use crate::services::ai::ui;
use anyhow::Result;
use chrono::{DateTime, Local};

use super::doc::{build, local};
use super::html::to_html;
use super::markdown::to_markdown;
use super::text::{to_pdf, to_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Text,
    Markdown,
    Html,
    Pdf,
    Json,
}

impl ReportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Json => "json",
        }
    }

    /// Name for a file picker filter.
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
            Self::Pdf => "PDF",
            Self::Json => "JSON",
        }
    }
}

impl std::str::FromStr for ReportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "txt" | "text" => Ok(Self::Text),
            "md" | "markdown" => Ok(Self::Markdown),
            "html" | "htm" => Ok(Self::Html),
            "pdf" => Ok(Self::Pdf),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "unknown report format '{s}' (use txt, md, html, pdf or json)"
            )),
        }
    }
}

/// `bssh-run-dddbee19-20261024-1723.pdf`.
pub fn file_name(report: &RunReport, format: ReportFormat) -> String {
    let when = report
        .started
        .as_deref()
        .and_then(local)
        .map(|t| format!("-{}", t.format("%Y%m%d-%H%M")))
        .unwrap_or_default();
    let id: String = ui::short_id(&report.run_id)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    format!("bssh-run-{id}{when}.{}", format.extension())
}

/// The report as the bytes of a file in `format`.
pub fn render(
    report: &RunReport,
    format: ReportFormat,
    generated: DateTime<Local>,
) -> Result<Vec<u8>> {
    if format == ReportFormat::Json {
        let mut s = report.to_json()?;
        s.push('\n');
        return Ok(s.into_bytes());
    }
    let doc = build(report, generated);
    Ok(match format {
        ReportFormat::Text => to_text(&doc).into_bytes(),
        ReportFormat::Markdown => to_markdown(&doc).into_bytes(),
        ReportFormat::Html => to_html(&doc).into_bytes(),
        ReportFormat::Pdf => to_pdf(&doc, report, generated),
        ReportFormat::Json => unreachable!(),
    })
}
