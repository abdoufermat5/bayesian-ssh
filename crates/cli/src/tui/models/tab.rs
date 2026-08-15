/// Active tab in the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Connections,
    History,
    Config,
    Files,
    Tunnels,
}

impl Tab {
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Connections => "Connections",
            Tab::History => "History",
            Tab::Config => "Config",
            Tab::Files => "Files",
            Tab::Tunnels => "Tunnels",
        }
    }

    pub fn all() -> &'static [Tab] {
        &[
            Tab::Connections,
            Tab::History,
            Tab::Config,
            Tab::Files,
            Tab::Tunnels,
        ]
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Connections => 0,
            Tab::History => 1,
            Tab::Config => 2,
            Tab::Files => 3,
            Tab::Tunnels => 4,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Connections,
            1 => Tab::History,
            2 => Tab::Config,
            3 => Tab::Files,
            4 => Tab::Tunnels,
            _ => Tab::Connections,
        }
    }

    pub fn next(&self) -> Self {
        Self::from_index((self.index() + 1) % Self::all().len())
    }

    pub fn prev(&self) -> Self {
        let len = Self::all().len();
        Self::from_index((self.index() + len - 1) % len)
    }
}
