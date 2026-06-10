use std::os::unix::io::{FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use crate::config::IntegrationConfig;
use crate::protocol::{read_frame, write_frame, GuestMessage, HostMessage};

mod handlers;

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
            } => handlers::handle_hello(stream, config, protocol_version, guest_version, container, capabilities)?,
            GuestMessage::Notify {
                summary,
                body,
                urgency: _,
                actions,
                app_name: _,
            } => handlers::handle_notify(stream, summary, body, actions)?,
            GuestMessage::XdgOpen { uri } => handlers::handle_xdg_open(uri)?,
            GuestMessage::ClipboardSet { text } => handlers::handle_clipboard_set(text)?,
            GuestMessage::ClipboardGet => handlers::handle_clipboard_get(stream)?,
            GuestMessage::HostExec { cmd, args } => handlers::handle_host_exec(stream, config, cmd, args)?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::handlers::{validate_uri, validate_host_exec_args};

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
