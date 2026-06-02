<p align="center">
  <img src="https://img.shields.io/github/v/tag/bethropolis/podmgr?label=version" alt="Version">
  <img src="https://img.shields.io/github/actions/workflow/status/bethropolis/podmgr/ci.yml?label=CI" alt="CI">
  <img src="https://img.shields.io/github/license/bethropolis/podmgr" alt="License">
  <br>
  <em>Define once. Run anywhere. No daemon.</em>
</p>

# podmgr

A Podman-native tool that turns a single TOML config into a fully integrated,
systemd-managed container — with Wayland, audio, GPU passthrough, XDG directory
sharing, and GUI app export. No daemon, no privilege escalation, no mounting
your whole home directory.

## Quick Start

```bash
git clone https://github.com/bethropolis/podmgr && cd podmgr
scripts/install.sh
podmgr create fedora    # pull, build, enable, start in one command
podmgr shell            # jump into the shell
```

## What It Does

| You give it... | podmgr gives you... |
|---|---|
| A TOML profile or OCI image reference | A systemd-managed Podman container |
| A package name (`firefox`, `htop`, ...) | The app baked into the image |
| `podmgr export app firefox` | `"Firefox (cachy)"` in your app launcher |

## Usage

```bash
# Create containers
podmgr create cachy                      # Arch-based CachyOS
podmgr create fedora --name dev          # custom name
podmgr create ghcr.io/user/img           # any image

# Run GUI apps
podmgr run firefox                       # detached, Wayland/GPU/pulse
podmgr exec -- htop                      # interactive command
podmgr shell                             # drop into a shell

# Export to your launcher
podmgr export app firefox                # desktop entry
podmgr export bin firefox                # terminal shim

# Customize
# Edit ~/.config/podmgr/cachy.toml:
#   [image.packages]
#   install = ["firefox", "htop"]
podmgr build --rebuild

# Diagnostics
podmgr doctor                            # health check
podmgr doctor --fix                      # auto-repair

# Tear down
podmgr stop && podmgr remove --all
```

## Install

**Option 1 — Online (pre-built binary, no Rust needed):**

```bash
curl -fsSL https://raw.githubusercontent.com/bethropolis/podmgr/main/scripts/install-online.sh | sh
```

**Option 2 — Local source build:**

```bash
scripts/install.sh                       # ~/.local/bin
scripts/install.sh --system              # /usr/local (requires sudo)
```

Supports `linux/amd64`.

**Uninstall:** `scripts/uninstall.sh` (binaries + completions only; `--all` also removes config, data, Quadlets)

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
