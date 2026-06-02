use std::ffi::CString;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use nix::unistd::{execv, execvp, fork, ForkResult};

/// Fork a daemon process, then exec the user command.
///
/// At the start, if running as root and host user info is available,
/// a matching system user is created and privileges are dropped.
///
/// Parent: execs the user shell/command (replaces this process) or
///         loops with a sleep if in background mode (no TTY, no cmd).
/// Child:  redirects stdio to /dev/null, drops privileges, then
///         execs `podmgr-guest --daemon`.
pub fn run(cmd: &[String]) -> ! {
    let host_user = std::env::var("HOST_USER").ok();
    let host_uid = std::env::var("HOST_UID").ok().and_then(|s| s.parse::<u32>().ok());
    let host_gid = std::env::var("HOST_GID").ok().and_then(|s| s.parse::<u32>().ok());

    // If running as root and host info is available, create the user and drop privileges.
    let drop = if let (Some(ref user), Some(uid), Some(gid)) = (&host_user, host_uid, host_gid) {
        let is_root = nix::unistd::getuid().is_root();
        if is_root {
            setup_user(user, uid, gid);
            Some((uid, gid, user.clone()))
        } else {
            None
        }
    } else {
        None
    };

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Child: become the daemon
            let dev_null_r = std::fs::File::open("/dev/null").unwrap_or_else(|_| std::process::exit(1));
            let dev_null_w = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .unwrap_or_else(|_| std::process::exit(1));

            let _ = nix::unistd::dup2(dev_null_r.as_raw_fd(), 0);
            let _ = nix::unistd::dup2(dev_null_w.as_raw_fd(), 1);
            let _ = nix::unistd::dup2(dev_null_w.as_raw_fd(), 2);

            let program = CString::new("/usr/local/bin/podmgr-guest").unwrap();
            let arg = CString::new("--daemon").unwrap();
            match execv(&program, &[&program, &arg]) {
                Ok(_) => unreachable!(),
                Err(e) => {
                    eprintln!("podmgr-guest: failed to execute daemon /usr/local/bin/podmgr-guest: {}", e);
                    std::process::exit(1)
                }
            }
        }
        Ok(ForkResult::Parent { .. }) => {
            // Parent: set env vars for the user command (stays as root)
            if let Some((_uid, _gid, ref user)) = drop {
                std::env::set_var("HOME", format!("/home/{}", user));
                std::env::set_var("USER", user);
                std::env::set_var("LOGNAME", user);
            }

            let is_tty = nix::unistd::isatty(0).unwrap_or(false);

            if is_tty && !cmd.is_empty() {
                // Interactive: exec the requested command
                let args: Vec<CString> = cmd
                    .iter()
                    .map(|s| CString::new(s.as_bytes()).unwrap_or_else(|_| {
                        eprintln!("podmgr-guest: command argument contains null byte");
                        std::process::exit(1)
                    }))
                    .collect();
                let args_refs: Vec<&CString> = args.iter().collect();
                match execvp(args_refs[0], &args_refs) {
                    Ok(_) => unreachable!(),
                    Err(e) => {
                        eprintln!("podmgr-guest: failed to execute command: {}", e);
                        std::process::exit(1);
                    }
                }
            } else if is_tty {
                // Interactive with no explicit CMD: start a login shell
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/usr/bin/fish".into());
                let program = CString::new(shell.as_bytes()).unwrap();
                let arg0 = CString::new(format!("-{}", shell)).unwrap();
                match execv(&program, &[&arg0]) {
                    Ok(_) => unreachable!(),
                    Err(e) => {
                        eprintln!("podmgr-guest: failed to execute shell {}: {}", shell, e);
                        std::process::exit(1);
                    }
                }
            } else {
                // Background (e.g. systemd): keep PID 1 alive
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
        }
        Err(_) => {
            std::process::exit(1)
        }
    }
}

/// Create a system user matching the host UID/GID, set up passwordless sudo,
/// and ensure runtime directories are owned by the user.
///
/// When the home directory already exists (e.g. bind-mounted), its actual
/// owner UID from the filesystem is used instead of HOST_UID, because
/// UserNS=keep-id idmapped mounts shift UIDs.  The chown step is skipped
/// entirely for pre-existing directories to avoid corrupting host ownership.
///
/// All operations are idempotent — safe to call on every container start.
fn setup_user(user: &str, uid: u32, gid: u32) {
    // 1. Group
    let group_exists = std::fs::read_to_string("/etc/group")
        .map(|c| c.lines().any(|l| l.starts_with(&format!("{}:", user))))
        .unwrap_or(false);
    if !group_exists {
        let status = std::process::Command::new("groupadd")
            .args(["-g", &gid.to_string(), user])
            .status();
        if status.is_err() || !status.unwrap().success() {
            // fallback for Alpine/busybox
            let _ = std::process::Command::new("addgroup")
                .args(["-g", &gid.to_string(), user])
                .status();
        }
    }

    // 2. User
    let user_exists = std::fs::read_to_string("/etc/passwd")
        .map(|c| c.lines().any(|l| l.starts_with(&format!("{}:", user))))
        .unwrap_or(false);
    let home_dir = Path::new("/home").join(user);

    if !user_exists {
        let status = std::process::Command::new("useradd")
            .args([
                "-u", &uid.to_string(),
                "-g", &gid.to_string(),
                "-d", &home_dir.to_string_lossy(),
                "-s", "/usr/bin/fish",
                "-m", user,
            ])
            .status();
        if status.is_err() || !status.unwrap().success() {
            let _ = std::process::Command::new("adduser")
                .args([
                    "-u", &uid.to_string(),
                    "-D",
                    "-G", user,
                    "-h", &home_dir.to_string_lossy(),
                    "-s", "/usr/bin/fish",
                    user,
                ])
                .status();
        }
    }

    // Make the home directory writable by all users inside the container.
    // With UserNS=keep-id the idmapped mount shifts UIDs, so the dynamic
    // user's UID may not match the directory owner.  chmod is safe because
    // it does NOT change ownership through the idmapped mount.
    let _ = std::process::Command::new("chmod")
        .args(["777", &home_dir.to_string_lossy()])
        .status();

    // 3. Supplementary groups
    for grp in ["wheel", "sudo", "video", "audio", "render"] {
        let _ = std::process::Command::new("usermod")
            .args(["-aG", grp, user])
            .status();
    }

    // 4. Passwordless sudo
    let sudoers_dir = Path::new("/etc/sudoers.d");
    if sudoers_dir.exists() {
        let sudo_file = sudoers_dir.join("podmgr");
        let _ = std::fs::write(&sudo_file, format!("{} ALL=(ALL) NOPASSWD: ALL\n", user));
        let _ = std::fs::set_permissions(&sudo_file, PermissionsExt::from_mode(0o440));
    }

    // 5. Make runtime directory writable by all
    //    Needed so glib/dconf/Wayland proxy can create sockets at runtime.
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", uid));
    let _ = std::process::Command::new("chmod")
        .args(["777", &runtime_dir])
        .status();

    // 6. dconf subdirectory
    let dconf_dir = Path::new(&runtime_dir).join("dconf");
    let _ = std::fs::create_dir_all(&dconf_dir);
    let _ = std::process::Command::new("chown")
        .args([uid.to_string().as_str(), gid.to_string().as_str(), dconf_dir.to_str().unwrap_or_default()])
        .status();
    let _ = std::process::Command::new("chmod")
        .args(["700", &dconf_dir.to_string_lossy()])
        .status();
}


