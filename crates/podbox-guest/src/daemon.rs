use std::collections::HashSet;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::error::GuestError;
use crate::protocol::HostMessage;
use crate::socket;

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
    let all_caps = vec![
        "notify".to_string(),
        "xdg_open".to_string(),
        "clipboard".to_string(),
        "host_exec".to_string(),
    ];
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
        ("notify", "notify-send"),
        ("xdg_open", "xdg-open"),
        ("clipboard", "podbox-clipboard"),
        ("host_exec", "host-exec"),
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

    if accepted.contains("notify") {
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
    loop {
        let mut fds = [PollFd::new(
            host_stream.as_fd(),
            PollFlags::POLLIN,
        )];

        match poll(&mut fds, PollTimeout::from(None::<u16>)) {
            Ok(_) => {
                let revents = fds[0].revents().unwrap_or(PollFlags::empty());
                if revents.contains(PollFlags::POLLHUP) || revents.contains(PollFlags::POLLERR) {
                    eprintln!("podbox-guest: host socket hung up.");
                    return Ok(());
                }
                if revents.contains(PollFlags::POLLIN) {
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
            }
            Err(nix::errno::Errno::EINTR) => continue,
            Err(e) => return Err(GuestError::Io(e.into())),
        }
    }
}
