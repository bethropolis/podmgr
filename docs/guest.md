---
description: podbox-guest daemon internals — startup sequence, event loop, socket protocol, and interceptors for notifications, clipboard, xdg-open, and host-exec.
---

# Guest Daemon Architecture

The guest daemon (`podbox-guest`) runs inside the container. It bridges
container capabilities (notifications, URI opening, clipboard, host execution)
to the host via a Unix socket connection.

---

## Entry Point (`entry.rs`)

The container starts with `podbox-guest --entry [<command>...]`.

1. **`fork()`** splits into two processes:

   - **Child** (daemon process): redirects stdio to `/dev/null`, then
     execs `podbox-guest --daemon` (re-exec). Runs the event loop.

   - **Parent** (shell/command process): if a command was given, execs it
     via `execv`. If empty, execs a login shell (`$SHELL` or `/bin/bash`,
     with `argv[0]` prefixed by `-` for login mode).

2. The parent **replaces itself** with the shell/command. The child runs
   independently as a background daemon with a 5-minute idle timeout.

---

## Daemon Lifecycle (`daemon.rs`)

### Startup sequence

1. **Create `/run/podbox/bin/`** — directory for interceptor symlinks
2. **Check version drift** — compare `PODBOX_HOST_VERSION` env var against
   `podbox-guest` version; warn on mismatch
3. **Connect to host socket** — `$XDG_RUNTIME_DIR/podbox/<container>.sock`
   with poll-based retry (3 attempts, 500ms interval, zero CPU)
4. **Handshake** — sends capability list (`notify`, `xdg_open`, `clipboard`,
   `host_exec`) to host; host responds with accepted subset
5. **Install interceptors** — creates symlinks in `/run/podbox/bin/` for
   each accepted capability
6. **PATH injection** — writes `/etc/environment.d/podbox.conf` that
   prepends `/run/podbox/bin` to `PATH`
7. **Event loop** — polls the host socket for messages

### Event loop

The event loop is `poll()`-based on a single file descriptor (the host
socket connection). It uses a **5-minute idle timeout** — if no message
arrives, the daemon exits gracefully.

| Event | Action |
|-------|--------|
| `Shutdown` message | Exit daemon |
| `Ping` message | No-op (keepalive) |
| `None` / EOF | Host disconnected; exit |
| `POLLHUP` / `POLLERR` | Host socket hung up; exit |
| Idle timeout (5 min) | No messages received; exit |
| `EINTR` | Retry `poll()` |

The daemon consumes **0% CPU** when idle — it is parked in the kernel by
`poll()`.

---

## Socket Protocol

The daemon connects to the host socket at
`$XDG_RUNTIME_DIR/podbox/<container>.sock`. Messages are length-prefixed
JSON over a Unix stream socket (see [protocol.md](protocol.md) for the wire
format).

### Handshake

```
→ {"type":"hello","version":"0.1.0","container":"myenv","capabilities":["notify","xdg_open","clipboard","host_exec"]}
← {"type":"hello_ack","accepted":["notify","xdg_open"],"rejected":["clipboard","host_exec"]}
```

---

## Interceptors

### Symlink dispatch

The daemon creates symlinks in `/run/podbox/bin/` pointing to the
`podbox-guest` binary:

| Symlink | Target | Capability |
|---------|--------|------------|
| `/run/podbox/bin/notify-send` | `podbox-guest` | `notify` |
| `/run/podbox/bin/xdg-open` | `podbox-guest` | `xdg_open` |
| `/run/podbox/bin/podbox-clipboard` | `podbox-guest` | `clipboard` |
| `/run/podbox/bin/host-exec` | `podbox-guest` | `host_exec` |

The binary detects which name was used to invoke it via `argv[0]` and
dispatches to the appropriate interceptor module (`main.rs`).

### PATH injection

`/etc/environment.d/podbox.conf` is written with:

```
PATH=/run/podbox/bin:$PATH
```

This ensures the interceptor symlinks take precedence over system-installed
binaries.

### Interceptor types

| Interceptor | File | What it does |
|-------------|------|-------------|
| `notify-send` | `interceptors/notify.rs` | Parses CLI args, sends `GuestMessage::Notify` to host. Supports `--action`/`-A` for action buttons. Waits for `NotifyActionResult` response when actions are present. |
| `xdg-open` | `interceptors/xdg_open.rs` | Sends URI in `GuestMessage::XdgOpen` to host |
| `podbox-clipboard` | `interceptors/clipboard.rs` | `set`: reads stdin, sends `ClipboardSet`; `get`: sends `ClipboardGet`, writes response to stdout |
| `host-exec` | `interceptors/host_exec.rs` | Connects to host socket, sends `HostExec` with command, then relays `HostExecStdout`/`HostExecStderr`/`HostExecDone` responses to stdout/stderr and exits with the remote exit code |

Each interceptor opens a **direct, ephemeral** Unix socket connection to
the host socket (not the daemon's persistent connection), sends its
message, and waits for acknowledgement before exiting.

### Host-exec security

Host-exec is **disabled by default**. When enabled via `[integration.host_exec]`, the host validates every command:

1. **Allowlist** — If configured, only commands whose aliases appear in the map may be run. The mapped host path is used (guest `$PATH` is ignored). Example error:
   ```
   Permission denied: 'ls' is not in the host-exec allowlist
   Allowed commands: systemctl, git
   ```

2. **Shell metacharacters** — Arguments containing `;`, `|`, `&`, `$`, or `` ` `` are rejected:
   ```
   host-exec: failed to execute 'echo $HOME': No such file or directory
   ```

3. **Dangerous flag patterns** — Arguments matching `--exec-path`, `--config`, `-o`, etc. are blocked:
   ```
   Security violation: argument "--exec-path=/tmp/x" uses a restricted flag pattern
   ```

4. **Absolute path bypass** — Using `/usr/bin/git` when the allowlist key is `git` is rejected:
   ```
   Permission denied: '/usr/bin/git' is not in the host-exec allowlist
   Allowed commands: systemctl, git
   ```
