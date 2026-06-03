<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="images/podbox-logo.svg">
    <img src="images/podbox-logo.svg" alt="podbox">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/bethropolis/podbox/releases"><img src="https://img.shields.io/github/v/tag/bethropolis/podbox?label=Version&style=for-the-badge&logo=github&color=3b82f6&labelColor=1e293b&logoColor=white" alt="Version"></a>
  <a href="https://github.com/bethropolis/podbox/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/bethropolis/podbox/ci.yml?label=CI&style=for-the-badge&logo=githubactions&labelColor=1e293b&logoColor=white" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-8b5cf6?style=for-the-badge&logo=opensourceinitiative&labelColor=1e293b&logoColor=white" alt="License"></a>
  <img src="https://img.shields.io/badge/Podman-Native-6366f1?style=for-the-badge&logo=podman&labelColor=1e293b&logoColor=white" alt="Podman Native">
  <img src="https://img.shields.io/badge/Platform-Linux-6e40c9?style=for-the-badge&logoColor=white&labelColor=1e293b" alt="Platform">
</p>

<p align="center">
  <em>Define once. Run anywhere. No daemon.</em>
</p>

## Quick Start

```bash
# 1. Install via pre-built binary
curl -fsSL https://raw.githubusercontent.com/bethropolis/podbox/main/scripts/install-online.sh | sh

podbox create fedora    # pulls, builds, enables, starts
podbox shell            # you're in
```

Also available from source: `git clone https://github.com/bethropolis/podbox && cd podbox && scripts/install.sh`.

## Why podbox?

Unlike other desktop sandboxing tools, `podbox` translates a single TOML config directly into native systemd Quadlet units — no daemon, no persistent orchestrator.

| | podbox | Distrobox / Toolbox | Flatpak | Raw `podman run` |
|---|---|---|---|---|
| **Daemonless** | Yes (systemd units) | Yes (shell shims) | Yes (systemd backend) | No |
| **Sandbox** | Strict (declared dirs only) | Weak (full `$HOME` mount) | Tight (portal-gated) | Custom per invocation |
| **D-Bus** | Filtered via `xdg-dbus-proxy` | Unfiltered session bus | Portal-limited | Unfiltered |
| **Config** | Declarative TOML | Imperative CLI params | Flatpak manifest | Shell flags |

## How It Works

A single TOML definition is your single source of truth. `podbox build` processes it into OCI images and systemd Quadlet units — no manual Containerfile or systemd editing.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="images/architecture.svg">
    <img src="images/architecture.svg" alt="podbox architecture" width="100%" style="max-width: 820px;">
  </picture>
</p>

---
## Configuration

Config files are loaded from `~/.config/podbox/<name>.toml`, a local `./.podbox.toml`, or built-in profiles. See [the config reference](docs/config.md) for all keys.

```toml
# ~/.config/podbox/cachy.toml
[image]
base = "ghcr.io/cachyos/cachyos-rootfs:latest"
name = "myenv"

[image.packages]
install = ["firefox", "pipewire-alsa", "mesa-dri-drivers"]

[container]
name = "myenv"
home = "~/containers/myenv"
shell = "bash"

[container.mounts]
extra = ["~/Projects:/home/user/Projects:z"]

[integration]
wayland    = true
audio      = true
gpu        = "auto"
dbus       = true
notify     = true
xdg_open   = true
clipboard  = true
sync_fonts = true

[integration.xdg_dirs]
documents = true
downloads = true

[integration.export]
apps = ["firefox"]
bins = ["rg"]

[dbus]
talk = ["org.freedesktop.Notifications"]
```

## Usage

**First container:**
```bash
podbox create cachy                 # Arch-based, gaming-ready
podbox create fedora --name dev     # Fedora, custom name
podbox create ghcr.io/user/img      # any OCI image
```

**Run things:**

```bash
podbox shell                        # interactive shell
podbox exec -- htop                 # run a command
podbox run firefox                  # GUI app, detached
```

**Export to your host:**

```bash
podbox export app firefox           # "Firefox (cachy)" in your launcher
podbox export bin rg                # ripgrep available in any terminal
```

## Install

**Online (pre-built binary):**

```bash
curl -fsSL https://raw.githubusercontent.com/bethropolis/podbox/main/scripts/install-online.sh | sh
```

**Local source build:**

```bash
scripts/install.sh                       # ~/.local/bin
scripts/install.sh --system              # /usr/local (sudo)
```

Supports `linux/x86_64`. Uninstall with `scripts/uninstall.sh`.

## Requirements

- Podman >= 4.6 with `pasta` or `slirp4netns`
- systemd (user session)
- Linux with Wayland (X11 apps run via Xwayland)
- Kernel: `kernel.unprivileged_userns_clone=1`, subuid/subgid configured
- `xdg-dbus-proxy` (for filtered D-Bus access)

## Troubleshooting

### D-Bus proxy fails or container hangs

Missing `xdg-dbus-proxy` on the host. Install it or set `dbus = false` under `[integration]`.

### UID mismatch inside mounts

`UserNS=keep-id` + `User=root` maps host UID 1000 to container UID 999. Don't `chown` inside mounts — it will corrupt host ownership.

### Desktop shims or interceptors not working

The `podbox-guest` daemon relies on `/run/podbox/bin` being in the guest `$PATH`. Verify the systemd socket unit is active: `systemctl --user status <name>.socket`.

## Command Reference

| Command | Description |
|---------|-------------|
| `podbox create <profile\|image>` | Init → build → enable → start in one command |
| `podbox init <profile>` | Scaffold a config file from a built-in profile |
| `podbox list` | List podbox-managed containers |
| `podbox build [--rebuild]` | Build or rebuild the container image |
| `podbox enable` | Install Quadlet systemd files |
| `podbox disable` | Remove Quadlet files |
| `podbox start` / `podbox stop` | Start / stop the container |
| `podbox shell` | Open interactive shell |
| `podbox enter <name>` | Enter a named container (auto-starts) |
| `podbox exec -- <cmd>` | Execute a command interactively |
| `podbox run <app>` | Run a GUI app detached |
| `podbox status` | Show container state |
| `podbox logs [-f]` | Show container logs |
| `podbox doctor [--fix]` | Run diagnostic checks, optionally auto-fix |
| `podbox export app <name>` | Export .desktop file to host launcher |
| `podbox export bin <name>` | Export bin shim to `~/.local/bin` |
| `podbox export clean` | Remove all exported shims and .desktop files |
| `podbox remove [--all]` | Remove the container (and home volume with `--all`) |
| `podbox translate-path --to-container <path>` | Translate host path to container path |
| `podbox translate-path --to-host <path>` | Translate container path to host path |
| `podbox completions <shell>` | Generate shell completions |

All commands support `--dry-run` to preview without executing.

## Environment

- `PODBOX_CONTAINER` — set inside the container to the active container name
- `PODBOX_VERSION` — the running launcher version

## Documentation

| Doc | Description |
|-----|-------------|
| [Configuration Reference](docs/config.md) | All TOML keys, defaults, examples |
| [Architecture Overview](docs/architecture.md) | How podbox works end-to-end |
| [Desktop Integration](docs/export.md) | Exporting apps and binaries |
| [Container Integration](docs/guest.md) | Guest daemon and interceptors |
| [D-Bus Proxy](docs/dbus-proxy.md) | Filtered D-Bus access via xdg-dbus-proxy |
| [Quadlet Reference](docs/quadlet.md) | Generated systemd units |
| [Host-Guest Protocol](docs/protocol.md) | Wire format and message types |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
