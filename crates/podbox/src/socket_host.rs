use std::io::Write;
use std::os::unix::io::{FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use crate::config::IntegrationConfig;
use crate::protocol::{read_frame, write_frame, GuestMessage, HostMessage};

/// Max number of concurrent host threads handling guest connections.
///
/// 4 is a deliberate, conservative cap: each thread runs the full
/// read/respond loop for a single client, and the only expensive
/// thing it does is spawn a process for `host-exec` or block on
/// `notify-rust`'s action signal.  In practice, a single container
/// only ever has the daemon and 0-2 ephemeral interceptors
/// (notify-send, host-exec, xdg-open, clipboard) — 4 is plenty of
/// headroom for a busy desktop session.
const MAX_CONCURRENT: usize = 4;

/// How often the host sends a keepalive `Ping` to a connected guest.
///
/// The guest's read-loop currently has a 5-minute idle timeout, so a
/// 60s ping interval stays well under that and prevents silent
/// integration failures for long-lived containers.
const PING_INTERVAL: Duration = Duration::from_secs(60);

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
                // Drain finished handles every iteration, not just when
                // we hit the cap.  This prevents the accept loop from
                // stalling on a slow/blocked client whose thread is
                // still alive but no longer productive.
                handles.retain_mut(|h| !h.is_finished());

                if handles.len() >= MAX_CONCURRENT {
                    eprintln!(
                        "podbox: dropping connection: {} concurrent clients already in flight",
                        handles.len()
                    );
                    continue;
                }

                let cfg = config.clone();
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
    // Bound reads so we can periodically ping the guest and prevent
    // its 5-minute idle timeout from killing the daemon.
    stream.set_read_timeout(Some(PING_INTERVAL))?;
    let mut last_ping = std::time::Instant::now();

    loop {
        let msg_bytes = match read_frame(stream) {
            Ok(Some(b)) => b,
            Ok(None) => return Ok(()),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if last_ping.elapsed() >= PING_INTERVAL {
                    if write_frame(stream, &HostMessage::Ping).is_err() {
                        return Ok(());
                    }
                    last_ping = std::time::Instant::now();
                }
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        last_ping = std::time::Instant::now();
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
                        "host_exec" => config.host_exec.enabled,
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
                if !config.host_exec.enabled {
                    write_frame(
                        stream,
                        &HostMessage::HostExecStderr {
                            data: "host-exec is disabled".into(),
                        },
                    )?;
                    write_frame(stream, &HostMessage::HostExecDone { exit_code: 1 })?;
                    continue;
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
                        continue;
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
                    continue;
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
            }
        }
    }
}

/// Validate arguments for host-exec, rejecting shell metacharacters and
/// dangerous flag patterns that could alter the behaviour of a whitelisted
/// binary (e.g. `git --exec-path=…`).
fn validate_host_exec_args(args: &[String]) -> Result<(), String> {
    for arg in args {
        // Shell metacharacters (defence in depth – Command::new avoids a shell,
        // but a whitelisted binary might interpret them unsafely).
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
        // Redirection operators — a whitelisted binary might write files
        // when given `<` / `>` / `>>` arguments.
        if arg.contains('<') || arg.contains('>') {
            return Err(format!("argument {:?} contains redirection operators", arg));
        }
        // Shell glob / brace expansion wildcards.
        if arg.contains('*') || arg.contains('?') || arg.contains('[') || arg.contains(']')
            || arg.contains('{') || arg.contains('}')
        {
            return Err(format!("argument {:?} contains glob or brace characters", arg));
        }
        // Subshell / escape characters.
        if arg.contains('(') || arg.contains(')') || arg.contains('\\') {
            return Err(format!("argument {:?} contains subshell or escape characters", arg));
        }
        // Dangerous flag prefixes that can subvert a whitelisted binary.
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
///
/// Allowed schemes are `http`, `https`, and `mailto`. Bare domains are
/// auto-prefixed with `https://` so a user typing `example.com` works.
fn validate_uri(uri: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::validate_uri;
    use super::validate_host_exec_args;

    // ── validate_uri tests ──

    #[test]
    fn allows_http_https_mailto() {
        assert_eq!(
            validate_uri("https://example.com"),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            validate_uri("http://example.com"),
            Some("http://example.com".to_string())
        );
        assert_eq!(
            validate_uri("mailto:user@host"),
            Some("mailto:user@host".to_string())
        );
    }

    #[test]
    fn refuses_path_traversal() {
        assert_eq!(validate_uri("/etc/passwd"), None);
        assert_eq!(validate_uri("../foo"), None);
        assert_eq!(validate_uri(""), None);
    }

    #[test]
    fn refuses_unknown_alphabetic_schemes() {
        assert_eq!(validate_uri("javascript:alert(1)"), None);
        assert_eq!(validate_uri("file:///etc/passwd"), None);
    }

    #[test]
    fn wraps_bare_domain() {
        assert_eq!(
            validate_uri("example.com"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(
            validate_uri("  https://example.com  "),
            Some("https://example.com".to_string())
        );
    }

    // ── validate_host_exec_args tests ──

    #[test]
    fn accepts_plain_args() {
        assert!(validate_host_exec_args(&["ls".into()]).is_ok());
        assert!(validate_host_exec_args(&["ls".into(), "-la".into(), "/tmp".into()]).is_ok());
        assert!(validate_host_exec_args(&["git".into(), "log".into(), "--oneline".into()]).is_ok());
    }

    #[test]
    fn rejects_shell_metacharacters() {
        assert!(validate_host_exec_args(&["echo".into(), "foo;bar".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), "foo|bar".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), "foo&bar".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), "$PATH".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), "`ls`".into()]).is_err());
    }

    #[test]
    fn rejects_redirection_operators() {
        assert!(validate_host_exec_args(&["cat".into(), "<file".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), ">file".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), ">>file".into()]).is_err());
    }

    #[test]
    fn rejects_glob_and_brace_chars() {
        assert!(validate_host_exec_args(&["ls".into(), "*.rs".into()]).is_err());
        assert!(validate_host_exec_args(&["ls".into(), "file?".into()]).is_err());
        assert!(validate_host_exec_args(&["ls".into(), "[abc]".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), "{a,b}".into()]).is_err());
    }

    #[test]
    fn rejects_subshell_and_escape_chars() {
        assert!(validate_host_exec_args(&["echo".into(), "$(whoami)".into()]).is_err());
        assert!(validate_host_exec_args(&["echo".into(), "line1\nline2".into()]).is_err());
    }

    #[test]
    fn rejects_restricted_flag_patterns() {
        assert!(validate_host_exec_args(&["git".into(), "--exec-path=/tmp".into()]).is_err());
        assert!(validate_host_exec_args(&["git".into(), "--config=user.name".into()]).is_err());
        assert!(validate_host_exec_args(&["vim".into(), "--plugin=malicious".into()]).is_err());
        assert!(validate_host_exec_args(&["python".into(), "--load=malicious".into()]).is_err());
        assert!(validate_host_exec_args(&["python".into(), "--module=malicious".into()]).is_err());
        assert!(validate_host_exec_args(&["git".into(), "--remote=evil".into()]).is_err());
        assert!(validate_host_exec_args(&["ssh".into(), "-o".into(), "StrictHostKeyChecking=no".into()]).is_err());
    }

    #[test]
    fn restricted_flag_detection_is_case_insensitive() {
        assert!(validate_host_exec_args(&["git".into(), "--EXEC-PATH=/tmp".into()]).is_err());
        assert!(validate_host_exec_args(&["GIT".into(), "--Config=evil".into()]).is_err());
    }

    #[test]
    fn does_not_restrict_safe_flags() {
        assert!(validate_host_exec_args(&["git".into(), "--exec".into()]).is_ok());
        assert!(validate_host_exec_args(&["git".into(), "--exec-path-is-ok".into()]).is_err(), "--exec-path prefix still blocked");
        assert!(validate_host_exec_args(&["ls".into(), "--color=auto".into()]).is_ok());
        assert!(validate_host_exec_args(&["cargo".into(), "--offline".into()]).is_ok());
    }

    #[test]
    fn rejects_empty_args_gracefully() {
        assert!(validate_host_exec_args(&[String::new()]).is_ok(), "empty string is not a metachar");
    }

    #[test]
    fn ascii_lowercase_only() {
        // Unicode characters should NOT be lowercased (to_ascii_lowercase is a no-op for non-ASCII)
        assert!(validate_host_exec_args(&["git".into(), "--EXEC-PATH=".into()]).is_err());
        assert!(validate_host_exec_args(&["git".into(), "--İ".into()]).is_ok(), "Turkish İ is non-ASCII");
    }
}
