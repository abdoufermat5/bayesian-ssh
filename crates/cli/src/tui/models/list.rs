use std::collections::HashSet;

/// Grouping mode for connections
#[derive(Debug, Clone, PartialEq)]
pub enum GroupMode {
    None,
    ByTag,
}

/// Multi-select state
#[derive(Debug, Clone, Default)]
pub struct MultiSelectState {
    /// Indices of selected items in the filtered list
    pub selected: HashSet<usize>,
    /// Whether multi-select mode is active
    pub active: bool,
}

impl MultiSelectState {
    pub fn toggle(&mut self, index: usize) {
        if self.selected.contains(&index) {
            self.selected.remove(&index);
        } else {
            self.selected.insert(index);
        }
        self.active = !self.selected.is_empty();
    }

    pub fn select_all(&mut self, count: usize) {
        self.selected = (0..count).collect();
        self.active = count > 0;
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.active = false;
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }
}

/// Connection ping status
#[derive(Debug, Clone, PartialEq)]
pub enum PingStatus {
    Checking,
    Reachable(std::time::Duration),
    Unreachable,
}
