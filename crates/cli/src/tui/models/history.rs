/// Sort field for history tab
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistorySortField {
    Date,
    Name,
    Duration,
    Status,
}

impl HistorySortField {
    pub fn label(&self) -> &'static str {
        match self {
            HistorySortField::Date => "Date",
            HistorySortField::Name => "Name",
            HistorySortField::Duration => "Duration",
            HistorySortField::Status => "Status",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            HistorySortField::Date => HistorySortField::Name,
            HistorySortField::Name => HistorySortField::Duration,
            HistorySortField::Duration => HistorySortField::Status,
            HistorySortField::Status => HistorySortField::Date,
        }
    }
}
