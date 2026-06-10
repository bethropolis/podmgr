use std::collections::HashSet;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::error::GuestError;
use crate::protocol::{write_frame, GuestMessage, HostMessage};
use crate::socket;

/// Open a pidfd for a given PID (Linux 5.3+).
fn open_pidfd(pid: i32) -> std::io::Result<OwnedFd> {
    let ret = unsafe { nix::libc::syscall(nix::libc::SYS_pidfd_open, pid, 0) };
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(ret as i32) })
    }
}

struct TrackedProcess {
    _pid: i32,
    fd: OwnedFd,
}

/// Scan /proc for user processes (anything not named podbox-guest/podmgr-guest).
fn scan_user_processes() -> Vec<i32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
                let comm_trimmed = comm.trim();
                if comm_trimmed != "podbox-guest" && comm_trimmed != "podmgr-guest" {
                    if let Ok(pid) = name_str.parse::<i32>() {
                        pids.push(pid);
                    }
                }
            }
        }
    }
    pids
}

pub fn run() -> Result<(), GuestError> {
    let host_socket_path = socket::host_socket_path()?;
    let container_name = socket::container_name()?;
    let bin_dir = PathBuf::from("/run/podbox/bin");

    // 1. Create /run/podbox/bin/
    std::fs::create_dir_all(&bin_dir)?;

    // 2. Connect to host socket with retry
    eprintln!("podbox-guest: connecting to host socket...");
    let mut host_stream = socket::connect_to_host(&host_socket_path)?;

    // 3. Handshake
    let all_caps: Vec<String> = crate::protocol::ALL_CAPABILITIES
        .iter()
        .map(|&s| s.to_string())
        .collect();
    let accepted = socket::handshake(&mut host_stream, &container_name, &all_caps)?;
    let accepted_set: HashSet<String> = accepted.iter().cloned().collect();
    eprintln!("podbox-guest: accepted capabilities: {:?}", accepted);

    // 4. Check version drift
    check_version_drift(&accepted_set, &mut host_stream, &container_name);

    // 5. Install interceptor symlinks for accepted capabilities
    install_interceptors(&accepted_set, &bin_dir)?;

    // 6. Write PATH injection
    write_path_injection(&bin_dir)?;

    // 7. Enter event loop (listen for host messages)
    event_loop(&mut host_stream)?;

    Ok(())
}

fn install_interceptors(
    accepted: &HashSet<String>,
    bin_dir: &std::path::Path,
) -> std::io::Result<()> {
    let self_path = std::env::current_exe()?;
    let self_path_str = self_path.to_string_lossy();

    let symlinks = vec![
        (crate::protocol::CAP_NOTIFY, "notify-send"),
        (crate::protocol::CAP_XDG_OPEN, "xdg-open"),
        (crate::protocol::CAP_CLIPBOARD, "podbox-clipboard"),
        (crate::protocol::CAP_HOST_EXEC, "host-exec"),
    ];

    for (cap, name) in symlinks {
        if accepted.contains(cap) {
            let link = bin_dir.join(name);
            let _ = std::fs::remove_file(&link); // ok if missing — symlink may be new
            std::os::unix::fs::symlink(self_path_str.as_ref(), &link)?;
        }
    }

    Ok(())
}

fn check_version_drift(
    accepted: &HashSet<String>,
    _host_stream: &mut UnixStream,
    container_name: &str,
) {
    let baked_host_version = match std::env::var("PODBOX_HOST_VERSION")
        .or_else(|_| std::env::var("PODMGR_HOST_VERSION"))
    {
        Ok(v) => v,
        Err(_) => return,
    };

    let guest_version = crate::VERSION;

    if baked_host_version == guest_version {
        return;
    }

    let summary = "podbox: container image is outdated";
    let body = format!(
        "Container '{container_name}' was built with podbox {baked_host_version} but host is now {guest_version}. Run `podbox build --rebuild`."
    );

    if accepted.contains(crate::protocol::CAP_NOTIFY) {
        let msg = crate::protocol::GuestMessage::Notify {
            summary: summary.to_string(),
            body,
            urgency: "normal".to_string(),
            actions: vec![],
            app_name: "podbox".to_string(),
        };
        let _ = crate::socket::connect_and_send_oneshot(&msg); // fire-and-forget notification
    } else {
        eprintln!("podbox-guest: image is outdated (built with {baked_host_version}, host is now {guest_version}). Run `podbox build --rebuild`.");
    }
}

fn write_path_injection(bin_dir: &std::path::Path) -> std::io::Result<()> {
    // POSIX shells (bash, zsh, sh)
    let conf_dir = std::path::PathBuf::from("/etc/profile.d");
    std::fs::create_dir_all(&conf_dir)?;
    let conf_path = conf_dir.join("podbox.sh");
    let content = format!("export PATH={}:$PATH\n", bin_dir.to_string_lossy());
    std::fs::write(conf_path, content)?;

    // Fish shell
    let fish_dir = std::path::PathBuf::from("/etc/fish/conf.d");
    if fish_dir.is_dir() || std::fs::create_dir_all(&fish_dir).is_ok() {
        let fish_path = fish_dir.join("podbox.fish");
        let fish_content = format!("fish_add_path -m {}\n", bin_dir.to_string_lossy());
        let _ = std::fs::write(fish_path, fish_content); // best-effort; fish may not be installed
    }

    Ok(())
}

fn event_loop(host_stream: &mut UnixStream) -> Result<(), GuestError> {
    let mut tracked: Vec<TrackedProcess> = Vec::new();

    loop {
        // Scope the poll set so borrows on host_stream/tracked are released
        // before we mutably access them below.
        let host_revents: PollFlags;
        let pid_revents: Vec<PollFlags>;

        {
            let mut fds: Vec<PollFd> = Vec::with_capacity(1 + tracked.len());
            fds.push(PollFd::new(host_stream.as_fd(), PollFlags::POLLIN));
            for proc in &tracked {
                fds.push(PollFd::new(proc.fd.as_fd(), PollFlags::POLLIN));
            }

            match poll(&mut fds, PollTimeout::from(None::<u16>)) {
                Ok(_nfds) => {
                    host_revents = fds[0].revents().unwrap_or(PollFlags::empty());
                    pid_revents = fds[1..]
                        .iter()
                        .map(|f| f.revents().unwrap_or(PollFlags::empty()))
                        .collect();
                }
                Err(nix::errno::Errno::EINTR) => continue,
                Err(e) => return Err(GuestError::Io(e.into())),
            }
        } // fds dropped — borrows of host_stream and tracked are released

        // ── Host socket events ──
        if host_revents.contains(PollFlags::POLLHUP) || host_revents.contains(PollFlags::POLLERR) {
            eprintln!("podbox-guest: host socket hung up.");
            return Ok(());
        }

        if host_revents.contains(PollFlags::POLLIN) {
            match socket::read_host_message(host_stream) {
                Ok(Some(HostMessage::Shutdown)) => {
                    eprintln!("podbox-guest: received shutdown, exiting.");
                    return Ok(());
                }
                Ok(Some(HostMessage::Ping)) => {}
                Ok(Some(HostMessage::HelloAck { .. })) => {}
                Ok(Some(HostMessage::ClipboardData { .. })) => {}
                Ok(Some(HostMessage::HostExecStdout { .. })) => {}
                Ok(Some(HostMessage::HostExecStderr { .. })) => {}
                Ok(Some(HostMessage::HostExecDone { .. })) => {}
                Ok(Some(HostMessage::NotifyActionResult { .. })) => {}
                Ok(Some(HostMessage::CheckIdle)) => {
                    let active = scan_user_processes();
                    if active.is_empty() {
                        let _ = write_frame(host_stream, &GuestMessage::IdleTimeout);
                    } else {
                        tracked.clear();
                        for pid in active {
                            if let Ok(fd) = open_pidfd(pid) {
                                tracked.push(TrackedProcess { _pid: pid, fd });
                            }
                        }
                        let _ = write_frame(host_stream, &GuestMessage::Busy);
                    }
                }
                Ok(None) => {
                    eprintln!("podbox-guest: host disconnected.");
                    return Ok(());
                }
                Err(e) => {
                    if !e.to_string().contains("WouldBlock") {
                        return Err(e);
                    }
                }
            }
        }

        // ── pidfd events (tracked process exits) ──
        let mut exited: Vec<usize> = Vec::new();
        for (i, rev) in pid_revents.iter().enumerate() {
            if rev.contains(PollFlags::POLLIN)
                || rev.contains(PollFlags::POLLHUP)
                || rev.contains(PollFlags::POLLERR)
            {
                exited.push(i);
            }
        }

        for &i in exited.iter().rev() {
            tracked.remove(i);
        }

        if !exited.is_empty() && tracked.is_empty() {
            let active = scan_user_processes();
            if active.is_empty() {
                let _ = write_frame(host_stream, &GuestMessage::IdleTimeout);
            } else {
                tracked.clear();
                for pid in active {
                    if let Ok(fd) = open_pidfd(pid) {
                        tracked.push(TrackedProcess { _pid: pid, fd });
                    }
                }
            }
        }
    }
}
