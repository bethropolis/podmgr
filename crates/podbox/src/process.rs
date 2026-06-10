use std::ffi::OsString;
use std::io::{self, IoSlice, IoSliceMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Output};

use nix::sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags};

/// Build a `Vec<OsString>` from a slice of `&str`/`&String` literals.
pub fn args<S: AsRef<str>>(items: &[S]) -> Vec<OsString> {
    items.iter().map(|s| OsString::from(s.as_ref())).collect()
}

/// Replace the current process with the given binary and arguments.
///
/// Uses `CommandExt::exec()` so the shell gets a real TTY.
/// On success this function never returns; on failure it returns an error.
pub fn exec_replace(bin: &str, args: &[OsString]) -> anyhow::Error {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    let err = cmd.exec();
    anyhow::Error::from(err).context(format!("failed to exec {}", bin))
}

/// Run a command, capturing stdout and stderr.
pub fn run_piped(bin: &str, args: &[OsString]) -> anyhow::Result<Output> {
    let output = Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?
        .wait_with_output()?;
    Ok(output)
}

/// Spawn a command attached to the current terminal.
pub fn spawn_interactive(bin: &str, args: &[OsString]) -> anyhow::Result<ExitStatus> {
    let status = Command::new(bin).args(args).status()?;
    Ok(status)
}

/// Open a pidfd for a given PID (Linux 5.3+).
///
/// Returns `Err` on old kernels or when the PID does not exist.
pub fn open_pidfd(pid: i32) -> io::Result<OwnedFd> {
    let ret = unsafe { nix::libc::syscall(nix::libc::SYS_pidfd_open, pid, 0) };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(ret as i32) })
    }
}

/// Send a raw file descriptor over a connected Unix stream via `SCM_RIGHTS`.
///
/// Sends one dummy byte alongside the descriptor so the receiver can detect EOF.
pub fn send_fd(stream: &UnixStream, fd: RawFd) -> io::Result<()> {
    let raw_fd = stream.as_raw_fd();
    let cmsg = ControlMessage::ScmRights(&[fd]);
    let iov = [IoSlice::new(&[0u8])];
    sendmsg::<()>(raw_fd, &iov, &[cmsg], MsgFlags::empty(), None)?;
    Ok(())
}

/// Receive a raw file descriptor from a connected Unix stream via `SCM_RIGHTS`.
///
/// Returns `None` when the sender has closed the connection (EOF).
pub fn recv_fd(stream: &UnixStream) -> io::Result<Option<RawFd>> {
    let raw_fd = stream.as_raw_fd();
    let mut buf = [0u8; 1];
    let mut iov = [IoSliceMut::new(&mut buf)];
    let mut cmsg_buf = vec![0u8; 256];
    let msg = recvmsg::<()>(raw_fd, &mut iov, Some(&mut cmsg_buf), MsgFlags::empty())?;

    if msg.bytes == 0 {
        return Ok(None);
    }

    if let Ok(cmsgs) = msg.cmsgs() {
        for cmsg in cmsgs {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if let Some(&fd) = fds.first() {
                    return Ok(Some(fd));
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_builds_osstring_vec() {
        let v = args(&["foo", "bar", "baz"]);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], "foo");
        assert_eq!(v[1], "bar");
        assert_eq!(v[2], "baz");
    }

    #[test]
    fn args_accepts_mixed_types() {
        let s = String::from("hello");
        let v = args(&["a", &s, "c"]);
        assert_eq!(v[1], "hello");
    }
}
