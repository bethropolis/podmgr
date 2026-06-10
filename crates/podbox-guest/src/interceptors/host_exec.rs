use std::os::unix::net::UnixStream;

use crate::socket::host_socket_path;
use crate::protocol::{read_frame, write_frame, GuestMessage, HostMessage};

pub fn run(args: &[String]) {
    if args.len() < 2 {
        eprintln!("host-exec: usage: host-exec <command> [args...]");
        std::process::exit(1);
    }

    let msg = GuestMessage::HostExec {
        cmd: args[1].clone(),
        args: args[2..].to_vec(),
    };

    let path = host_socket_path().unwrap_or_else(|e| {
        eprintln!("host-exec: {}", e);
        std::process::exit(1);
    });
    let mut stream = UnixStream::connect(&path).unwrap_or_else(|e| {
        eprintln!("host-exec: connect failed: {}", e);
        std::process::exit(1);
    });

    write_frame(&mut stream, &msg).unwrap();

    let mut exit_code = 0i32;
    while let Ok(Some(bytes)) = read_frame(&mut stream) {
        match serde_json::from_slice::<HostMessage>(&bytes) {
            Ok(HostMessage::HostExecStdout { data }) => print!("{}", data),
            Ok(HostMessage::HostExecStderr { data }) => eprint!("{}", data),
            Ok(HostMessage::HostExecDone { exit_code: code }) => {
                exit_code = code;
                break;
            }
            Ok(HostMessage::Shutdown) => break,
            _ => break,
        }
    }
    std::process::exit(exit_code);
}
