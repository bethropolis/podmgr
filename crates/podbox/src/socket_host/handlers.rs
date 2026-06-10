use std::io::Write;
use std::os::unix::net::UnixStream;

use crate::config::IntegrationConfig;
use crate::protocol::{write_frame, HostMessage};

/// Handle a `Hello` handshake from the guest.
pub(super) fn handle_hello(
    stream: &mut UnixStream,
    config: &IntegrationConfig,
    protocol_version: u32,
    guest_version: String,
    container: String,
    capabilities: Vec<String>,
) -> anyhow::Result<()> {
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
            crate::protocol::CAP_NOTIFY => config.notify,
            crate::protocol::CAP_XDG_OPEN => config.xdg_open,
            crate::protocol::CAP_CLIPBOARD => config.clipboard,
            crate::protocol::CAP_HOST_EXEC => config.host_exec.enabled,
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
    Ok(())
}

/// Handle a `Notify` message from the guest.
pub(super) fn handle_notify(
    stream: &mut UnixStream,
    summary: String,
    body: String,
    actions: Vec<crate::protocol::NotifyAction>,
) -> anyhow::Result<()> {
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
    Ok(())
}

/// Handle an `XdgOpen` message from the guest.
pub(super) fn handle_xdg_open(
    uri: String,
) -> anyhow::Result<()> {
    if let Some(validated) = validate_uri(&uri) {
        if let Ok(mut child) = std::process::Command::new("xdg-open")
            .arg(&validated)
            .spawn()
        {
            let _ = child.wait();
        }
    }
    Ok(())
}

/// Handle a `ClipboardSet` message from the guest.
pub(super) fn handle_clipboard_set(
    text: String,
) -> anyhow::Result<()> {
    let mut child = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(ref mut stdin) = child.stdin {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    Ok(())
}

/// Handle a `ClipboardGet` message from the guest.
pub(super) fn handle_clipboard_get(
    stream: &mut UnixStream,
) -> anyhow::Result<()> {
    let output = std::process::Command::new("wl-paste").output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let response = HostMessage::ClipboardData {
        text: text.trim().to_string(),
    };
    write_frame(stream, &response)?;
    Ok(())
}

/// Handle a `HostExec` message from the guest.
pub(super) fn handle_host_exec(
    stream: &mut UnixStream,
    config: &IntegrationConfig,
    cmd: String,
    args: Vec<String>,
) -> anyhow::Result<()> {
    if !config.host_exec.enabled {
        write_frame(
            stream,
            &HostMessage::HostExecStderr {
                data: "host-exec is disabled".into(),
            },
        )?;
        write_frame(stream, &HostMessage::HostExecDone { exit_code: 1 })?;
        return Ok(());
    }

    let resolved = match config.host_exec.resolve(&cmd) {
        Some(p) => p,
        None => {
            let allowed = config
                .host_exec
                .allowlist
                .as_ref()
                .map(|m| m.keys().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            write_frame(
                stream,
                &HostMessage::HostExecStderr {
                    data: format!(
                        "Permission denied: '{}' is not in the host-exec allowlist\nAllowed commands: {}",
                        cmd,
                        allowed
                    ),
                },
            )?;
            write_frame(stream, &HostMessage::HostExecDone { exit_code: 1 })?;
            return Ok(());
        }
    };

    if let Err(msg) = validate_host_exec_args(&args) {
        write_frame(
            stream,
            &HostMessage::HostExecStderr {
                data: format!("Security violation: {}", msg),
            },
        )?;
        write_frame(stream, &HostMessage::HostExecDone { exit_code: 1 })?;
        return Ok(());
    }

    match std::process::Command::new(resolved)
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
            let msg = if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "host-exec: '{}' not found in allowlist path or host $PATH",
                    cmd
                )
            } else {
                format!("host-exec: failed to execute '{}': {}", cmd, e)
            };
            write_frame(stream, &HostMessage::HostExecStderr { data: msg })?;
            write_frame(stream, &HostMessage::HostExecDone { exit_code: 1 })?;
        }
    }
    Ok(())
}

/// Validate arguments for host-exec, rejecting shell metacharacters and
/// dangerous flag patterns that could alter the behaviour of a whitelisted
/// binary (e.g. `git --exec-path=…`).
pub(super) fn validate_host_exec_args(args: &[String]) -> Result<(), String> {
    for arg in args {
        if arg.contains(';')
            || arg.contains('|')
            || arg.contains('&')
            || arg.contains('$')
            || arg.contains('`')
            || arg.contains('\n')
            || arg.contains('\r')
        {
            return Err(format!("argument {:?} contains shell metacharacters", arg));
        }
        if arg.contains('<') || arg.contains('>') {
            return Err(format!("argument {:?} contains redirection operators", arg));
        }
        if arg.contains('*') || arg.contains('?') || arg.contains('[') || arg.contains(']')
            || arg.contains('{') || arg.contains('}')
        {
            return Err(format!("argument {:?} contains glob or brace characters", arg));
        }
        if arg.contains('(') || arg.contains(')') || arg.contains('\\') {
            return Err(format!("argument {:?} contains subshell or escape characters", arg));
        }
        let lower = arg.to_ascii_lowercase();
        if lower.starts_with("--exec-path")
            || lower.starts_with("--config")
            || lower.starts_with("--plugin")
            || lower.starts_with("--load")
            || lower.starts_with("--module")
            || lower.starts_with("--remote=")
            || lower == "-o"
        {
            return Err(format!("argument {:?} uses a restricted flag pattern", arg));
        }
    }
    Ok(())
}

/// Validate a URI from inside the container, returning a safe-to-open
/// string (or `None` to refuse).
pub(super) fn validate_uri(uri: &str) -> Option<String> {
    let s = uri.trim();
    if s.is_empty() || s.starts_with('/') || s.starts_with('.') {
        return None;
    }

    match url::Url::parse(s) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            if scheme == "http" || scheme == "https" || scheme == "mailto" {
                Some(s.to_string())
            } else {
                None
            }
        }
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            Some(format!("https://{}", s))
        }
        _ => None,
    }
}
