/// Sort field for connection list
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortField {
    Name,
    Host,
    LastUsed,
    Created,
}

impl SortField {
    pub fn label(&self) -> &'static str {
        match self {
            SortField::Name => "Name",
            SortField::Host => "Host",
            SortField::LastUsed => "Last Used",
            SortField::Created => "Created",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SortField::Name => SortField::Host,
            SortField::Host => SortField::LastUsed,
            SortField::LastUsed => SortField::Created,
            SortField::Created => SortField::Name,
        }
    }
}

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn toggle(&self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }

    pub fn arrow(&self) -> &'static str {
        match self {
            SortDirection::Asc => "↑",
            SortDirection::Desc => "↓",
        }
    }
}
