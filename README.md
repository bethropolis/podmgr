<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/podbox-logo.svg">
    <img src="docs/assets/podbox-logo.svg" alt="podbox">
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
# Install via pre-built binary
curl -fsSL https://bethropolis.github.io/podbox/install.sh | sh

podbox create fedora    # pulls, builds, enables, starts
podbox shell            # you're in
```

Also available from source: `git clone https://github.com/bethropolis/podbox && cd podbox && scripts/install.sh`.

## Why podbox?

Most desktop container tools make a trade-off: full integration means mounting your entire home directory into the container. podbox doesn't. You declare exactly what the container can see — directories, devices, and services — and nothing else is shared.

| | podbox | Distrobox / Toolbox | Raw `podman run` |
|---|---|---|---|
| **Home directory** | Isolated volume, opt-in sharing | Full `$HOME` mounted by default | Manual `-v` flags |
| **Config** | Declarative TOML, version-controllable | Imperative CLI flags | Shell flags per run |
| **Lifecycle** | systemd Quadlet units | Shell shims | Manual |
| **D-Bus** | Filtered via `xdg-dbus-proxy` | Unfiltered session bus | Unfiltered |
| **Wayland / audio** | Opt-out (on by default) | Always on | Manual |
| **GPU** | `auto` / `nvidia` / off | `--nvidia` flag | Manual device flags |
| **Notifications** | Guest interceptor → host | Via shared D-Bus | Not supported |
| **Clipboard** | Guest interceptor → host | Via shared home | Not supported |
| **Host commands** | `host-exec` interceptor | `distrobox-host-exec` | Not supported |
| **SSH agent** | Socket forward (opt-in) | Auto-mounted | Not supported |
| **Baked images** | Yes — packages in image, not runtime | No — packages reinstalled on rebuild | N/A |
| **Reproducibility** | Full — TOML → image → unit | Partial — image only | None |
| **Runtime** | Podman only | Podman / Docker / lilipod | Any OCI runtime |

> podbox is not a distrobox replacement. Distrobox optimises for maximum host integration and is excellent at that. podbox optimises for declared, reproducible environments where you control exactly what is shared.

## How It Works

A single TOML definition is your single source of truth. `podbox build` processes it into OCI images and systemd Quadlet units — no manual Containerfile or systemd editing.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/architecture.svg">
    <img src="docs/assets/architecture.svg" alt="podbox architecture" width="100%" style="max-width: 820px;">
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
host_exec  = true
ssh_agent  = true
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
curl -fsSL https://bethropolis.github.io/podbox/install.sh | sh
```

**Local source build:**

```bash
scripts/install.sh                       # ~/.local/bin
scripts/install.sh --system              # /usr/local (sudo)
```

Supports `linux/x86_64`. Uninstall with `scripts/uninstall.sh`.

## Requirements

- Podman >= 5.5 (SSH agent requires >= 5.6)
- systemd (user session)
- Linux with Wayland (X11 apps run via Xwayland)
- Kernel: `kernel.unprivileged_userns_clone=1`, subuid/subgid configured
- `xdg-dbus-proxy` (for filtered D-Bus access)

## Troubleshooting

Run `podbox doctor` first — it checks the most common issues automatically and explains what to fix.

For detailed guides on specific issues (container won't start, D-Bus proxy, Wayland socket errors, interceptors, UID mapping, SSH agent forwarding, build failures, shell hangs), see the [Troubleshooting Guide](docs/troubleshooting.md).

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
- `PODBOX_HOST_VERSION` — set inside the container at build time; checked at guest daemon startup

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
| [Troubleshooting Guide](docs/troubleshooting.md) | Common issues and fixes |

## Contributing

See [Contributing](https://github.com/bethropolis/podbox?tab=contributing-ov-file).

## License

MIT
