# Architecture

## How It Works

A definition TOML is the single source of truth. Everything podmgr generates —
Containerfiles, Quadlet systemd units, lock files, desktop entries — derives
from this one file. The user never writes a raw Containerfile or systemd unit
manually.

```
Definition File (myenv.toml)
        │
        ▼  podmgr build
  ┌──────────────────┐     ┌─────────────────────────────────┐
  │  Containerfile   │     │  Quadlet files                  │
  │  (generated)     │     │  myenv.build                    │
  │                  │     │  myenv.socket                   │
  └──────┬───────────┘     │  myenv.container                │
         │                 └──────────────┬──────────────────┘
         ▼                                │
   podman build                 podmgr enable
         │                                │
         ▼                                ▼
  localhost/podmgr-myenv:latest    systemctl --user daemon-reload
         │                         systemctl --user enable --now myenv
         └──────────────────────────────────┘
                          │
                          ▼  container starts
             catatonit (PID 1, via --init)
                          │
                   podmgr-guest --entry
                    ├── fork → podmgr-guest --daemon
                    │          connects to host socket
                    └── exec → bash / fish (user shell)
```

## Codegen Pipeline

`podmgr build` runs these steps in order. Each codegen step is a **pure function**:
data in, string out, no I/O. Orchestration (file writes, podman invocations) is
separate.

```
Config struct
    │
    ├── codegen::containerfile::generate(config, guest_binary_path) → String
    │
    ├── codegen::quadlet::generate_build(config, containerfile_path) → String
    │
    ├── codegen::quadlet::generate_socket(config) → String
    │
    ├── codegen::quadlet::generate_container(config, host_env, xdg_dirs) → String
    │
    └── lock::write(config_checksum, image_digest) → LockFile

Then (I/O phase):
    write build context to ~/.local/share/podmgr/<name>/
    copy podmgr-guest binary into build context
    podman build -t localhost/podmgr-<name>:latest <context-dir>
    get digest via podman inspect
    write lock file
```

## Generated Containerfile

```dockerfile
FROM fedora:44

# [image.packages]
RUN dnf install -y git gcc ripgrep && dnf clean all

# [image.run] custom steps
RUN dnf clean all

# podmgr integration layer — always last
COPY podmgr-guest /usr/local/bin/podmgr-guest
RUN chmod +x /usr/local/bin/podmgr-guest

ENV PODMGR_CONTAINER=myenv
ENTRYPOINT ["/usr/local/bin/podmgr-guest", "--entry"]
CMD ["/usr/bin/bash"]
```

### Build Context Layout

```
~/.local/share/podmgr/<name>/
├── Containerfile
├── podmgr-guest          # static musl binary from host
```

## Generated Quadlet Files

Three files written to `~/.config/containers/systemd/`.

### `myenv.build`

```ini
[Build]
ImageTag=localhost/podmgr-myenv:latest
File=/home/user/.local/share/podmgr/myenv/Containerfile
```

The `.build` unit makes `myenv.service` depend on the build. Images are only
rebuilt when the Containerfile changes.

### `myenv.socket`

```ini
[Unit]
Description=podmgr host-guest socket — myenv

[Socket]
ListenStream=%t/podmgr/myenv.sock
SocketMode=0600
DirectoryMode=0700

[Install]
WantedBy=sockets.target
```

`%t` is systemd's specifier for `$XDG_RUNTIME_DIR`. The socket is created
before the container starts and persists across restarts.

### `myenv.container`

Key Quadlet settings (see [quadlet.md](quadlet.md) for full list):

| Setting | Value | Purpose |
|---------|-------|---------|
| `UserNS` | `keep-id` | Maps host UID/GID into container |
| `SecurityLabelDisable` | `true` | Required for Wayland socket access |
| `PodmanArgs` | `--init` | catatonit as PID 1 (zombie reaping) |
| `Volume` | `%h/containers/<name>:/root:Z` | Isolated home (never the host home) |
| `Restart` | `on-failure` | Auto-restart on crash |

Volumes for Wayland, audio, D-Bus, XDG dirs, and the host-guest socket are
added conditionally based on the config.

## Host-Guest Socket Protocol

The guest daemon connects to a Unix socket on the host to bridge container
capabilities. Messages are length-prefixed JSON (see [protocol.md](protocol.md)
for the wire format).

```
Container process
    │ runs: notify-send "hello"
    ▼
Interceptor symlink → podmgr-guest (re-exec)
    │ connects to local daemon socket
    │ sends: {"type":"notify","summary":"hello"}
    ▼
podmgr-guest --daemon (event loop)
    │ forwards to host socket
    ▼
Host socket handler → desktop notification appears
```

## Guest Daemon (podmgr-guest)

The guest binary is a static musl binary baked into every built image.
Its behavior is determined by `argv[0]`:

| Invoked as | Mode |
|-----------|------|
| `podmgr-guest --entry` | Fork daemon, exec user shell/command |
| `podmgr-guest --daemon` | Event loop, interceptor setup |
| `notify-send` (symlink) | Parse args, forward to daemon |
| `xdg-open` (symlink) | Parse args, forward to daemon |

### Daemon startup sequence

1. Read `PODMGR_CONTAINER` env → derive socket paths
2. Create `/run/podmgr/bin/` directory
3. Connect to host socket (3 retries × 500ms)
4. Handshake: send capabilities, receive accepted list
5. Install interceptor symlinks in `/run/podmgr/bin/`
6. Prepend `/run/podmgr/bin` to `$PATH` via `/etc/environment.d/podmgr.conf`
7. Enter event loop (poll-based, 0% CPU when idle, 5-min idle timeout)

If the socket is absent at startup, the daemon logs a warning and exits cleanly.
The container continues running without integration — this is intentional.

## UID Mapping

`UserNS=keep-id` + `User=root` creates an idmapped mount that shifts UIDs by 1
inside the container (host UID 1000 → container UID 999). The entrypoint reads
the actual home owner and makes the directory world-writable. No `chown` is
performed on bind-mounted directories — that would corrupt host ownership
through the idmapped mount.

## Runtime Flow (Full Sequence)

```
LOGIN
  │
  ▼
systemd --user starts myenv.socket
  creates: /run/user/1000/podmgr/myenv.sock
  │
  ▼ (autostart=true)
systemd --user starts myenv.service (from myenv.container)
  │
  ▼
podman run --init --name myenv \
  -v ~/containers/myenv:/root:Z \
  -v ~/Documents:/root/Documents:z \
  -v /run/user/1000/wayland-0:/run/user/1000/wayland-0 \
  -v /run/user/1000/podmgr/myenv.sock:/run/user/1000/podmgr/myenv.sock \
  ... localhost/podmgr-myenv:latest
  │
  ▼
catatonit (PID 1) → podmgr-guest --entry
  │
  ├── fork → podmgr-guest --daemon
  │     ├── connect to host socket
  │     ├── handshake
  │     ├── install interceptors
  │     └── event loop
  │
  └── exec → bash (user shell)
        │
        │  user runs: notify-send "build done"
        ▼
      interceptor → daemon → host socket → notification
```

## Project Structure

```
podmgr/
├── Cargo.toml                    # workspace root
├── crates/
│   ├── podmgr/                   # host CLI binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # entry point, dispatch
│   │       ├── cli.rs            # clap CLI definition
│   │       ├── config.rs         # TOML parsing + validation
│   │       ├── build.rs          # build orchestration
│   │       ├── codegen/          # pure string generators
│   │       ├── export.rs         # .desktop + bin shim
│   │       ├── quadlet_install.rs
│   │       ├── socket_host.rs    # host-side socket handler
│   │       ├── podman.rs         # podman subcommand wrappers
│   │       ├── process.rs        # exec_replace, run_piped, spawn
│   │       ├── lock.rs           # build lock file
│   │       ├── env.rs            # host env resolution
│   │       ├── xdg.rs            # XDG dir resolution
│   │       └── error.rs          # error types
│   │
│   └── podmgr-guest/             # static musl sidecar
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # argv[0] dispatch
│           ├── entry.rs          # fork + exec
│           ├── daemon.rs         # event loop
│           ├── socket.rs         # socket I/O
│           ├── protocol.rs       # message types + framing
│           ├── interceptors/     # notify, xdg_open, clipboard
│           └── error.rs
│
├── tests/                        # integration + unit tests
├── scripts/                      # install / uninstall
└── docs/                         # documentation
```

### Key architectural rules

- **Pure codegen:** All `codegen::*` functions are pure — data in, string out.
  No I/O, no env reads, no filesystem access.
- **Boundary separation:** I/O lives only in `build.rs`, `quadlet_install.rs`,
  `socket_host.rs`, `export.rs`.
- **musl static:** `podmgr-guest` must stay statically linkable. No tokio,
  no openssl, no crate that links against glibc.
- **exec_replace for TTY:** `podmgr shell` and `podmgr exec` use
  `CommandExt::exec()` to replace the process — never `spawn_interactive`.
  This preserves the TTY for readline, Ctrl+L, etc.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Configuration error |
| 3 | Container missing |
| 4 | Build or inspect failure |
| 5 | Missing dependency |
