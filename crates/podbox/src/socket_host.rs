use std::io::Write;
use std::os::unix::io::{FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use crate::config::IntegrationConfig;
use crate::protocol::{read_frame, write_frame, GuestMessage, HostMessage};

/// Max number of concurrent host threads handling guest connections.
const MAX_CONCURRENT: usize = 4;

/// Run the host socket server for a container.
///
/// Uses systemd socket activation when `LISTEN_FDS` is set (FD 3).
/// Falls back to creating the socket at `socket_path` directly.
pub fn run(socket_path: &Path, config: &IntegrationConfig) -> anyhow::Result<()> {
    let path = socket_path.to_path_buf();
    let config = config.clone();

    let listener = match listen_fd() {
        Some(fd) => unsafe { UnixListener::from_raw_fd(fd) },
        None => {
            let _ = std::fs::remove_file(&path);
            UnixListener::bind(&path)?
        }
    };

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();

    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let cfg = config.clone();
                if handles.len() >= MAX_CONCURRENT {
                    let mut active_handles = Vec::new();
                    for h in std::mem::take(&mut handles) {
                        if h.is_finished() {
                            let _ = h.join();
                        } else {
                            active_handles.push(h);
                        }
                    }
                    handles = active_handles;
                }
                let handle = std::thread::spawn(move || {
                    if let Err(e) = handle_connection(&mut stream, &cfg) {
                        eprintln!("Error handling guest connection: {}", e);
                    }
                });
                handles.push(handle);
            }
            Err(e) => {
                eprintln!("Socket accept failed: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn listen_fd() -> Option<RawFd> {
    let pid = std::env::var("LISTEN_PID").ok()?.parse::<u32>().ok()?;
    if pid != std::process::id() {
        return None;
    }
    let fds = std::env::var("LISTEN_FDS").ok()?.parse::<u32>().ok()?;
    if fds == 0 {
        return None;
    }
    Some(3)
}

fn handle_connection(stream: &mut UnixStream, config: &IntegrationConfig) -> anyhow::Result<()> {
    while let Some(msg_bytes) = read_frame(stream)? {
        let msg: GuestMessage = serde_json::from_slice(&msg_bytes)?;

        match msg {
            GuestMessage::Hello {
                protocol_version,
                guest_version,
                container,
                capabilities,
            } => {
                if protocol_version != crate::protocol::PROTOCOL_VERSION {
                    eprintln!(
                        "Host: protocol mismatch — got v{}, expected v{}",
                        protocol_version,
                        crate::protocol::PROTOCOL_VERSION
                    );
                    write_frame(stream, &HostMessage::Shutdown)?;
                    return Ok(());
                }
                eprintln!(
                    "Host: Guest hello (v{}, container: {}, caps: {:?})",
                    guest_version, container, capabilities
                );
                let mut accepted = Vec::new();
                let mut rejected = Vec::new();
                for cap in capabilities {
                    let enabled = match cap.as_str() {
                        "notify" => config.notify,
                        "xdg_open" => config.xdg_open,
                        "clipboard" => config.clipboard,
                        "host_exec" => config.host_exec,
                        _ => false,
                    };
                    if enabled {
                        accepted.push(cap);
                    } else {
                        rejected.push(cap);
                    }
                }
                let response = HostMessage::HelloAck { accepted, rejected };
                write_frame(stream, &response)?;
            }
            GuestMessage::Notify {
                summary,
                body,
                urgency: _,
                actions,
                app_name: _,
            } => {
                if actions.is_empty() {
                    let _ = notify_rust::Notification::new()
                        .summary(&summary)
                        .body(&body)
                        .show();
                } else {
                    let mut notif = notify_rust::Notification::new();
                    notif.summary(&summary).body(&body);
                    for action in &actions {
                        notif.action(&action.key, &action.label);
                    }
                    let handle = match notif.show() {
                        Ok(h) => h,
                        Err(_) => {
                            let _ = write_frame(
                                stream,
                                &HostMessage::NotifyActionResult {
                                    notification_id: 0,
                                    action_key: String::new(),
                                },
                            );
                            return Ok(());
                        }
                    };
                    let mut chosen_key = String::new();
                    handle.wait_for_action(|action| {
                        chosen_key = action.to_string();
                    });
                    let _ = write_frame(
                        stream,
                        &HostMessage::NotifyActionResult {
                            notification_id: 0,
                            action_key: chosen_key,
                        },
                    );
                }
            }
            GuestMessage::XdgOpen { uri } => {
                if let Some(validated) = validate_uri(&uri) {
                    if let Ok(mut child) = std::process::Command::new("xdg-open")
                        .arg(&validated)
                        .spawn()
                    {
                        let _ = child.wait();
                    }
                }
            }
            GuestMessage::ClipboardSet { text } => {
                let mut child = std::process::Command::new("wl-copy")
                    .stdin(std::process::Stdio::piped())
                    .spawn()?;
                if let Some(ref mut stdin) = child.stdin {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
            }
            GuestMessage::ClipboardGet => {
                let output = std::process::Command::new("wl-paste").output()?;
                let text = String::from_utf8_lossy(&output.stdout);
                let response = HostMessage::ClipboardData {
                    text: text.trim().to_string(),
                };
                write_frame(stream, &response)?;
            }
            GuestMessage::HostExec { cmd, args } => {
                if !config.host_exec {
                    write_frame(stream, &HostMessage::Shutdown)?;
                    return Ok(());
                }

                match std::process::Command::new(&cmd)
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                {
                    Ok(child) => {
                        let output = child.wait_with_output()?;
                        if !output.stdout.is_empty() {
                            write_frame(
                                stream,
                                &HostMessage::HostExecStdout {
                                    data: String::from_utf8_lossy(&output.stdout).to_string(),
                                },
                            )?;
                        }
                        if !output.stderr.is_empty() {
                            write_frame(
                                stream,
                                &HostMessage::HostExecStderr {
                                    data: String::from_utf8_lossy(&output.stderr).to_string(),
                                },
                            )?;
                        }
                        let code = output.status.code().unwrap_or(1);
                        write_frame(stream, &HostMessage::HostExecDone { exit_code: code })?;
                    }
                    Err(e) => {
                        write_frame(
                            stream,
                            &HostMessage::HostExecStderr {
                                data: format!("host-exec: failed to execute '{}': {}", cmd, e),
                            },
                        )?;
                        write_frame(stream, &HostMessage::HostExecDone { exit_code: 1 })?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_uri(uri: &str) -> Option<String> {
    let allowed_schemes = ["http", "https", "mailto"];
    let trimmed = uri.trim();
    if trimmed.starts_with('/') || trimmed.starts_with('.') {
        return None;
    }
    if let Some(idx) = trimmed.find("://") {
        let scheme = &trimmed[..idx];
        if allowed_schemes.contains(&scheme) {
            return Some(trimmed.to_string());
        }
        return None;
    }
    if trimmed.starts_with("mailto:") {
        return Some(trimmed.to_string());
    }
    if let Some(colon_idx) = trimmed.find(':') {
        let scheme = &trimmed[..colon_idx];
        if allowed_schemes.contains(&scheme) {
            return Some(trimmed.to_string());
        }
        if scheme.chars().all(|c| c.is_alphabetic()) {
            return None;
        }
    }
    if !trimmed.is_empty() {
        Some(format!("https://{}", trimmed))
    } else {
        None
    }
}
