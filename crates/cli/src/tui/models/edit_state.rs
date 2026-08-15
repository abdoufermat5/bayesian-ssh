use crate::models::Connection;

/// State for inline editing (edit or add)
#[derive(Debug, Clone)]
pub struct EditState {
    /// Working copy of the connection being edited
    pub connection: Connection,
    /// Original connection name (for DB update lookup); empty for new connections
    pub original_name: String,
    /// Whether this is a new connection (Add mode)
    pub is_new: bool,
    /// Which field is currently selected (0-8)
    pub field_index: usize,
    /// Current input buffer for the active field
    pub field_value: String,
}

impl EditState {
    pub const FIELD_COUNT: usize = 9;

    pub fn field_label(index: usize) -> &'static str {
        match index {
            0 => "Name",
            1 => "Host",
            2 => "User",
            3 => "Port",
            4 => "Bastion",
            5 => "Bastion User",
            6 => "Key Path",
            7 => "Kerberos",
            8 => "Tags",
            _ => "",
        }
    }

    pub fn field_value_str(&self, index: usize) -> String {
        match index {
            0 => self.connection.name.clone(),
            1 => self.connection.host.clone(),
            2 => self.connection.user.clone(),
            3 => self.connection.port.to_string(),
            4 => self.connection.bastion.clone().unwrap_or_default(),
            5 => self.connection.bastion_user.clone().unwrap_or_default(),
            6 => self.connection.key_path.clone().unwrap_or_default(),
            7 => {
                if self.connection.use_kerberos {
                    "yes".into()
                } else {
                    "no".into()
                }
            }
            8 => self.connection.tags.join(", "),
            _ => String::new(),
        }
    }

    /// Apply the current field_value buffer into the connection struct
    pub fn apply_field(&mut self) {
        let val = self.field_value.trim().to_string();
        match self.field_index {
            0 => self.connection.name = val,
            1 => self.connection.host = val,
            2 => self.connection.user = val,
            3 => {
                if let Ok(p) = val.parse::<u16>() {
                    self.connection.port = p;
                }
            }
            4 => {
                self.connection.bastion = if val.is_empty() { None } else { Some(val) };
            }
            5 => {
                self.connection.bastion_user = if val.is_empty() { None } else { Some(val) };
            }
            6 => {
                self.connection.key_path = if val.is_empty() { None } else { Some(val) };
            }
            7 => {
                self.connection.use_kerberos =
                    matches!(val.to_lowercase().as_str(), "yes" | "y" | "true" | "1");
            }
            8 => {
                self.connection.tags = val
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {}
        }
    }

    /// Load the current field value from the connection into the buffer
    pub fn load_field(&mut self) {
        self.field_value = self.field_value_str(self.field_index);
    }

    /// Validate required fields for saving
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.connection.name.trim().is_empty() {
            return Err("Name is required");
        }
        if self.connection.host.trim().is_empty() {
            return Err("Host is required");
        }
        if self.connection.user.trim().is_empty() {
            return Err("User is required");
        }
        Ok(())
    }
}
