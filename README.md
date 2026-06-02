# podmgr

A Podman-native tool that turns a single TOML definition into a fully integrated,
systemd-managed container. One command to create, build, and start a container
with Wayland/audio/GPU passthrough, XDG directory sharing, and GUI app export —
no daemon, no mounting your whole home directory.

## Quick Start

```bash
git clone https://github.com/bethropolis/podmgr && cd podmgr
scripts/install.sh                       # install to ~/.local/bin
podmgr create fedora                     # pull, build, enable, start
podmgr shell                             # jump into the shell
```

## Essential Workflows

```bash
# Create a container from a built-in profile or any OCI image
podmgr create cachy                      # Arch-based CachyOS
podmgr create fedora --name dev          # custom name
podmgr create ghcr.io/user/img           # or any image reference

# Run apps
podmgr run firefox                       # detached GUI app
podmgr exec -- htop                      # interactive command
podmgr shell                             # open a shell

# Export apps to your launcher
podmgr export app firefox                # "Firefox (cachy)" in app menu
podmgr export bin firefox                # "firefox" in any terminal

# Add packages to your image
# Edit ~/.config/podmgr/cachy.toml:
#   [image.packages]
#   manager = "pacman"
#   install = ["firefox", "htop"]
podmgr build --rebuild                   # bakes them into the image

# Diagnose issues
podmgr doctor                            # check everything is healthy
podmgr doctor --fix                      # auto-repair

# Remove
podmgr stop && podmgr remove --all
```

## Install

```bash
scripts/install.sh                       # ~/.local/bin (recommended)
scripts/install.sh --system              # /usr/local (requires sudo)
```

**Uninstall:** `scripts/uninstall.sh` (binaries + completions only; `--all` also removes config, data, Quadlets)

**Manual build:** `cargo build --release -p podmgr && cargo build --release --target x86_64-unknown-linux-musl -p podmgr-guest`

## Command Reference

| Command | Description |
|---------|-------------|
| `podmgr create <profile\|image>` | Init → build → enable → start in one command |
| `podmgr init <profile>` | Scaffold a config file from a built-in profile |
| `podmgr list` | List podmgr-managed containers |
| `podmgr build [--rebuild]` | Build or rebuild the container image |
| `podmgr enable` | Install Quadlet systemd files |
| `podmgr disable` | Remove Quadlet files |
| `podmgr start` / `podmgr stop` | Start / stop the container |
| `podmgr shell` | Open interactive shell |
| `podmgr enter <name>` | Enter a named container (auto-starts) |
| `podmgr exec -- <cmd>` | Execute a command interactively |
| `podmgr run <app>` | Run a GUI app detached |
| `podmgr status` | Show container state |
| `podmgr logs [-f]` | Show container logs |
| `podmgr doctor [--fix]` | Run diagnostic checks, optionally auto-fix |
| `podmgr export app <name>` | Export .desktop file to host launcher |
| `podmgr export bin <name>` | Export bin shim to `~/.local/bin` |
| `podmgr export clean` | Remove all exported shims and .desktop files |
| `podmgr remove [--all]` | Remove the container (and home volume with `--all`) |
| `podmgr translate-path --to-container <path>` | Translate host path to container path |
| `podmgr translate-path --to-host <path>` | Translate container path to host path |
| `podmgr completions <shell>` | Generate shell completions |

All commands support `--dry-run` to preview without executing.

## Documentation

| Doc | Description |
|-----|-------------|
| [Configuration Reference](docs/config.md) | All TOML keys, defaults, examples |
| [Architecture Overview](docs/architecture.md) | How podmgr works end-to-end |
| [Desktop Integration](docs/export.md) | Exporting apps and binaries |
| [Container Integration](docs/guest.md) | Guest daemon and interceptors |
| [D-Bus Proxy](docs/dbus-proxy.md) | Filtered D-Bus access via xdg-dbus-proxy |
| [Quadlet Reference](docs/quadlet.md) | Generated systemd units |
| [Host-Guest Protocol](docs/protocol.md) | Wire format and message types |

## Requirements

- Podman >= 4.6
- systemd (user session)
- Linux with Wayland (for GUI passthrough)
- xdg-user-dirs
- Rust toolchain (to build)

## License

MIT
