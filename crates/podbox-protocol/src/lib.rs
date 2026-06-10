use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// Increment on breaking wire-format changes. Backwards-compatible
/// additions (new optional message types) do NOT increment this.
pub const PROTOCOL_VERSION: u32 = 1;

/// Guest protocol capability identifiers.
///
/// Single source of truth — always use these constants in match arms,
/// construction, and capability negotiation rather than inline strings.
pub const CAP_NOTIFY: &str = "notify";
pub const CAP_XDG_OPEN: &str = "xdg_open";
pub const CAP_CLIPBOARD: &str = "clipboard";
pub const CAP_HOST_EXEC: &str = "host_exec";

/// All known capabilities in negotiation order.
pub const ALL_CAPABILITIES: &[&str] = &[CAP_NOTIFY, CAP_XDG_OPEN, CAP_CLIPBOARD, CAP_HOST_EXEC];

/// Messages sent from guest to host.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuestMessage {
    Hello {
        protocol_version: u32,
        guest_version: String,
        container: String,
        capabilities: Vec<String>,
    },
    Notify {
        summary: String,
        body: String,
        urgency: String,
        #[serde(default)]
        actions: Vec<NotifyAction>,
        #[serde(default)]
        app_name: String,
    },
    XdgOpen {
        uri: String,
    },
    ClipboardSet {
        text: String,
    },
    ClipboardGet,
    HostExec {
        cmd: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotifyAction {
    pub key: String,
    pub label: String,
}

/// Messages sent from host to guest.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    HelloAck {
        accepted: Vec<String>,
        rejected: Vec<String>,
    },
    ClipboardData {
        text: String,
    },
    HostExecStdout {
        data: String,
    },
    HostExecStderr {
        data: String,
    },
    HostExecDone {
        exit_code: i32,
    },
    NotifyActionResult {
        notification_id: u32,
        action_key: String,
    },
    Ping,
    Shutdown,
}

/// Write a length-prefixed JSON frame.
pub fn write_frame<W: Write>(w: &mut W, msg: &impl Serialize) -> io::Result<()> {
    let json = serde_json::to_vec(msg)?;
    let len = (json.len() as u32).to_be_bytes();
    w.write_all(&len)?;
    w.write_all(&json)?;
    w.flush()?;
    Ok(())
}

/// Maximum frame size: 16 MiB.
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Read a length-prefixed JSON frame.
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame too large: {} bytes (max {})", len, MAX_FRAME_SIZE),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_serializes_with_type_tag() {
        let msg = GuestMessage::Hello {
            protocol_version: 1,
            guest_version: "0.2.0".into(),
            container: "myenv".into(),
            capabilities: vec!["notify".into()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"hello\""));
    }

    #[test]
    fn frame_length_prefix_matches_payload() {
        let msg = GuestMessage::ClipboardGet;
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
        assert_eq!(len, buf[4..].len());
    }

    #[test]
    fn roundtrip_notify_message() {
        let msg = GuestMessage::Notify {
            summary: "hello".into(),
            body: "world".into(),
            urgency: "normal".into(),
            actions: vec![],
            app_name: String::new(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();

        let payload = read_frame(&mut &buf[..]).unwrap().unwrap();
        let decoded: GuestMessage = serde_json::from_slice(&payload).unwrap();
        match decoded {
            GuestMessage::Notify {
                summary,
                body,
                urgency,
                actions,
                app_name: _,
            } => {
                assert_eq!(summary, "hello");
                assert_eq!(body, "world");
                assert_eq!(urgency, "normal");
                assert!(actions.is_empty());
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn roundtrip_clipboard_set() {
        let msg = GuestMessage::ClipboardSet {
            text: "clipboard content".into(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();

        let payload = read_frame(&mut &buf[..]).unwrap().unwrap();
        let decoded: GuestMessage = serde_json::from_slice(&payload).unwrap();
        match decoded {
            GuestMessage::ClipboardSet { text } => {
                assert_eq!(text, "clipboard content");
            }
            _ => panic!("wrong message type"),
        }
    }
}
