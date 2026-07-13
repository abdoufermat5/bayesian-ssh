use super::AgentStatus;
use std::process::Command;

#[tauri::command]
pub fn get_agent_status() -> Result<AgentStatus, String> {
    let socket = std::env::var("SSH_AUTH_SOCK").ok();
    let active = socket.is_some();

    let mut keys = Vec::new();
    if active {
        // Run ssh-add -l to list loaded keys
        let output = Command::new("ssh-add").arg("-l").output();

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
    if std::env::var("SSH_AUTH_SOCK").is_ok() {
        return get_agent_status();
    }

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
    let output = Command::new("ssh-add")
        .arg(&key_path)
        .output()
        .map_err(|e| format!("Failed to execute ssh-add: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
        if err_msg.is_empty() {
            Err(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(err_msg)
        }
    }
}
