pub mod clipboard;
pub mod host_exec;
pub mod notify;
pub mod xdg_open;

use std::os::unix::net::UnixStream;

use crate::protocol::{read_frame, write_frame, GuestMessage, HostMessage};
use crate::socket;

/// Connect directly to the host socket, send a one-shot message.
///
/// Does NOT wait for a response — the host never sends acks for
/// notifications, xdg-open, or clipboard-set frames.
fn send_to_host(msg: &GuestMessage) -> Result<(), crate::error::GuestError> {
    let path = socket::host_socket_path()?;
    let mut stream = UnixStream::connect(&path)?;
    write_frame(&mut stream, msg)?;
    Ok(())
}

/// Connect directly to the host socket, send a message, and read a typed host response.
fn send_to_host_and_read_response(
    msg: &GuestMessage,
) -> Result<HostMessage, crate::error::GuestError> {
    let path = socket::host_socket_path()?;
    let mut stream = UnixStream::connect(&path)?;
    write_frame(&mut stream, msg)?;
    let response = read_frame(&mut stream)?.ok_or_else(|| {
        crate::error::GuestError::HandshakeFailed("host closed connection".into())
    })?;
    let host_msg: HostMessage = serde_json::from_slice(&response)?;
    Ok(host_msg)
}
