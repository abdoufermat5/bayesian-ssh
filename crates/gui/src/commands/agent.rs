use super::AgentStatus;
use std::path::PathBuf;
use std::process::Command;

fn expand_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

fn resolve_active_socket() -> Option<String> {
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        if crate::is_valid_socket(&sock) {
            return Some(sock);
        }
        std::env::remove_var("SSH_AUTH_SOCK");
    }
    if let Some(discovered) = crate::find_ssh_agent_socket() {
        std::env::set_var("SSH_AUTH_SOCK", &discovered);
        Some(discovered)
    } else {
        None
    }
}

#[tauri::command]
pub fn get_agent_status() -> Result<AgentStatus, String> {
    let socket = resolve_active_socket();
    let active = socket.is_some();

    let mut keys = Vec::new();
    if let Some(ref sock) = socket {
        // Run ssh-add -l to list loaded keys
        let output = Command::new("ssh-add")
            .env("SSH_AUTH_SOCK", sock)
            .arg("-l")
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    if !line.trim().is_empty() {
                        keys.push(line.to_string());
                    }
                }
            }
        }
    }

    Ok(AgentStatus {
        active,
        socket_path: socket,
        keys,
    })
}

#[tauri::command]
pub fn start_agent() -> Result<AgentStatus, String> {
    if let Some(sock) = resolve_active_socket() {
        let test_cmd = Command::new("ssh-add")
            .env("SSH_AUTH_SOCK", &sock)
            .arg("-l")
            .output();
        if let Ok(out) = test_cmd {
            if out.status.code() == Some(0) || out.status.code() == Some(1) {
                return get_agent_status();
            }
        }
    }

    std::env::remove_var("SSH_AUTH_SOCK");

    let output = Command::new("ssh-agent")
        .arg("-s")
        .output()
        .map_err(|e| format!("Failed to start ssh-agent: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut socket_path = None;

    for line in stdout.lines() {
        // e.g. SSH_AUTH_SOCK=/tmp/ssh-XXXXXX/agent.XXXX; export SSH_AUTH_SOCK;
        if line.starts_with("SSH_AUTH_SOCK=") {
            if let Some(end) = line.find(';') {
                let val = &line["SSH_AUTH_SOCK=".len()..end];
                socket_path = Some(val.to_string());
                std::env::set_var("SSH_AUTH_SOCK", val);
            }
        }
        // e.g. SSH_AGENT_PID=XXXXX; export SSH_AGENT_PID;
        if line.starts_with("SSH_AGENT_PID=") {
            if let Some(end) = line.find(';') {
                let val = &line["SSH_AGENT_PID=".len()..end];
                std::env::set_var("SSH_AGENT_PID", val);
            }
        }
    }

    if socket_path.is_none() {
        return Err("Failed to parse ssh-agent environment variables".to_string());
    }

    get_agent_status()
}

#[tauri::command]
pub fn add_key_to_agent(key_path: String) -> Result<String, String> {
    let resolved_key = expand_path(&key_path);

    if !resolved_key.exists() {
        return Err(format!("Key file does not exist: {}", resolved_key.display()));
    }

    let socket = match resolve_active_socket() {
        Some(s) => s,
        None => {
            let status = start_agent()?;
            status.socket_path.ok_or_else(|| "Failed to auto-start SSH agent".to_string())?
        }
    };

    let output = Command::new("ssh-add")
        .env("SSH_AUTH_SOCK", &socket)
        .arg(&resolved_key)
        .output()
        .map_err(|e| format!("Failed to execute ssh-add: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            Ok(format!("Successfully added key {}", resolved_key.display()))
        } else {
            Ok(stdout)
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let err_msg = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "ssh-add exited with an error".to_string()
        };

        // If adding key to current socket failed due to agent connection issues,
        // attempt to start a fresh ssh-agent and add the key to it as fallback!
        if err_msg.contains("Error connecting to agent")
            || err_msg.contains("No such file")
            || err_msg.contains("refused")
        {
            std::env::remove_var("SSH_AUTH_SOCK");
            if let Ok(new_status) = start_agent() {
                if let Some(new_sock) = new_status.socket_path {
                    if let Ok(new_output) = Command::new("ssh-add")
                        .env("SSH_AUTH_SOCK", &new_sock)
                        .arg(&resolved_key)
                        .output()
                    {
                        if new_output.status.success() {
                            let out_str = String::from_utf8_lossy(&new_output.stdout).trim().to_string();
                            if out_str.is_empty() {
                                return Ok(format!(
                                    "Successfully added key {} (started new ssh-agent)",
                                    resolved_key.display()
                                ));
                            } else {
                                return Ok(out_str);
                            }
                        }
                    }
                }
            }
        }

        Err(err_msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path_tilde() {
        let home = dirs::home_dir().expect("Home dir must exist");
        let expanded = expand_path("~/.ssh/id_rsa");
        assert_eq!(expanded, home.join(".ssh/id_rsa"));
    }

    #[test]
    fn test_expand_path_absolute() {
        let path = "/etc/ssh/ssh_config";
        let expanded = expand_path(path);
        assert_eq!(expanded, PathBuf::from(path));
    }

    #[test]
    fn test_add_key_nonexistent_file() {
        let res = add_key_to_agent("/path/to/definitely_nonexistent_key_12345".to_string());
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Key file does not exist"));
    }
}


