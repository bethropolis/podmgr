use std::os::fd::RawFd;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::IntegrationConfig;
use crate::process;
use crate::protocol::{read_frame, write_frame, GuestMessage, HostMessage};

mod handlers;

/// Max number of concurrent host threads handling guest connections.
const MAX_CONCURRENT: usize = 4;

/// How often the host sends a keepalive `Ping` to a connected guest.
const PING_INTERVAL: Duration = Duration::from_secs(60);

/// Shared mutable state between all connections and PID monitor threads.
struct SharedState {
    /// The first (and only) guest-daemon stream — used to send CheckIdle.
    daemon_stream: Mutex<Option<UnixStream>>,
    /// Number of active terminal sessions tracked via pidfd.
    session_count: AtomicU32,
    /// Container name, for `systemctl stop` on idle timeout.
    container_name: String,
}

/// Run the host socket server for a container.
pub fn run(
    socket_path: &Path,
    config: &IntegrationConfig,
    container_name: &str,
) -> anyhow::Result<()> {
    let path = socket_path.to_path_buf();
    let config = config.clone();

    let listener = match listen_fd() {
        Some(fd) => unsafe { UnixListener::from_raw_fd(fd) },
        None => {
            let _ = std::fs::remove_file(&path);
            UnixListener::bind(&path)?
        }
    };

    let state = Arc::new(SharedState {
        daemon_stream: Mutex::new(None),
        session_count: AtomicU32::new(0),
        container_name: container_name.to_string(),
    });

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();

    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                handles.retain_mut(|h| !h.is_finished());

                if handles.len() >= MAX_CONCURRENT {
                    eprintln!(
                        "podbox: dropping connection: {} concurrent clients already in flight",
                        handles.len()
                    );
                    continue;
                }

                let cfg = config.clone();
                let state = Arc::clone(&state);
                let handle = std::thread::spawn(move || {
                    if let Err(e) = handle_connection(&mut stream, &cfg, &state) {
                        eprintln!("Error handling connection: {}", e);
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

fn handle_connection(
    stream: &mut UnixStream,
    config: &IntegrationConfig,
    state: &Arc<SharedState>,
) -> anyhow::Result<()> {
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
                // Store the daemon stream so we can send CheckIdle later
                if let Ok(clone) = stream.try_clone() {
                    *state.daemon_stream.lock().unwrap() = Some(clone);
                }
                handlers::handle_hello(
                    stream,
                    config,
                    protocol_version,
                    guest_version,
                    container,
                    capabilities,
                )?;
            }
            GuestMessage::RegisterSession => {
                // Receive the pidfd via SCM_RIGHTS
                let fd = match process::recv_fd(stream) {
                    Ok(Some(fd)) => fd,
                    Ok(None) => return Ok(()),
                    Err(_) => return Ok(()),
                };
                state.session_count.fetch_add(1, Ordering::SeqCst);
                let s = Arc::clone(state);
                std::thread::spawn(move || monitor_pidfd(fd, s));
                // Return immediately — the CLI closes the connection after
                // sending RegisterSession + pidfd.
                return Ok(());
            }
            GuestMessage::Busy => {}
            GuestMessage::IdleTimeout => {
                let name = &state.container_name;
                eprintln!("podbox: container '{}' idle — stopping", name);
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "stop", &format!("{}.service", name)])
                    .status();
            }
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
            GuestMessage::HostExec { cmd, args } => {
                handlers::handle_host_exec(stream, config, cmd, args)?
            }
        }
    }
}

/// Block until `fd` (a pidfd) becomes readable, then decrement the session
/// counter.  If the count reaches zero, send `CheckIdle` to the guest daemon.
fn monitor_pidfd(raw_fd: RawFd, state: Arc<SharedState>) {
    let mut pfd = nix::libc::pollfd {
        fd: raw_fd,
        events: nix::libc::POLLIN,
        revents: 0,
    };

    loop {
        let ret = unsafe { nix::libc::poll(&mut pfd, 1, -1) };
        if ret < 0 {
            let errno = unsafe { *nix::libc::__errno_location() };
            if errno == nix::libc::EINTR {
                continue;
            }
            break;
        }
        if pfd.revents & (nix::libc::POLLIN | nix::libc::POLLHUP | nix::libc::POLLERR) != 0 {
            break;
        }
    }

    unsafe { nix::libc::close(raw_fd) };

    let prev = state.session_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        let mut daemon = state.daemon_stream.lock().unwrap();
        if let Some(ref mut daemon) = *daemon {
            let _ = write_frame(daemon, &HostMessage::CheckIdle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::handlers::{validate_host_exec_args, validate_uri};

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
        assert!(validate_host_exec_args(&[
            "ssh".into(),
            "-o".into(),
            "StrictHostKeyChecking=no".into()
        ])
        .is_err());
    }

    #[test]
    fn restricted_flag_detection_is_case_insensitive() {
        assert!(validate_host_exec_args(&["git".into(), "--EXEC-PATH=/tmp".into()]).is_err());
        assert!(validate_host_exec_args(&["GIT".into(), "--Config=evil".into()]).is_err());
    }

    #[test]
    fn does_not_restrict_safe_flags() {
        assert!(validate_host_exec_args(&["git".into(), "--exec".into()]).is_ok());
        assert!(
            validate_host_exec_args(&["git".into(), "--exec-path-is-ok".into()]).is_err(),
            "--exec-path prefix still blocked"
        );
        assert!(validate_host_exec_args(&["ls".into(), "--color=auto".into()]).is_ok());
        assert!(validate_host_exec_args(&["cargo".into(), "--offline".into()]).is_ok());
    }

    #[test]
    fn rejects_empty_args_gracefully() {
        assert!(
            validate_host_exec_args(&[String::new()]).is_ok(),
            "empty string is not a metachar"
        );
    }

    #[test]
    fn ascii_lowercase_only() {
        assert!(validate_host_exec_args(&["git".into(), "--EXEC-PATH=".into()]).is_err());
        assert!(
            validate_host_exec_args(&["git".into(), "--\u{0130}".into()]).is_ok(),
            "Turkish \u{0130} is non-ASCII"
        );
    }
}
