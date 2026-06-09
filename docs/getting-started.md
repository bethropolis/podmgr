---
description: Get started with podbox — prebuilt profiles, custom builds, and daily usage.
---

# Getting Started

## Installation

```bash
curl -fsSL https://bethropolis.github.io/podbox/install.sh | sh
```

Or from source: `git clone https://github.com/bethropolis/podbox && cd podbox && scripts/install.sh`.

See the [README](../README.md#requirements) for system requirements.

---

## Two Ways to Create a Container

podbox supports two workflows. Pick the one that fits:

| Method | Use case | How it works |
|--------|----------|-------------|
| **Prebuilt** | Quick start, gaming, reproducible | Pull a ready-made image from a registry |
| **Custom** | Full control, specific packages | Build from a distro base image |

---

## Prebuilt Method (Quick Start)

Prebuilt profiles come with Wayland, audio, GPU, and common packages ready to go.

### Non-interactive

```bash
# Create a gaming-ready container
podbox create cachy

# Or a Fedora-based one with a custom name
podbox create fedora --name dev
```

### Interactive

```bash
podbox init -i
# Select "CachyOS", "Fedora", or "Gaming" from the list
# Review and confirm the config
podbox create dev
```

### What happens

1. `podbox init` scaffolds a config file (`~/.config/podbox/<name>.toml`)
2. `podbox create` pulls the prebuilt image, installs Quadlet systemd units, and starts the container
3. `podbox enter <name>` drops you into an interactive shell

---

## Custom Method (Build from Base)

Build a container from a plain distro image with your own packages and configuration.

### Non-interactive

```bash
# Create a config from a base image
podbox init fedora:44 --name myenv

# Or in one step — build and start
podbox build
podbox start
podbox enter myenv
```

Or in one step with `create`:

```bash
# podbox create works with any OCI image reference
podbox create myenv                     # uses local config
podbox create ubuntu:24.04 --name dev   # pull + configure + enable + start
```

### Interactive

```bash
podbox init -i
# Select "Custom (from scratch)" at the top of the list
# Enter: base image, packages, extra RUN commands
# Complete the wizard (shell, XDG dirs, GPU, lifecycle)
podbox create myenv
```

### Custom config example

```toml
# ~/.config/podbox/myenv.toml
[image]
base = "fedora:44"
name = "myenv"

[image.packages]
install = ["fish", "git", "neovim", "gcc", "ripgrep"]

[container]
name = "myenv"
home = "~/containers/myenv"
shell = "/usr/bin/fish"

[integration]
wayland = true
audio = true
gpu = "auto"

[integration.xdg_dirs]
documents = true
downloads = true
projects = true
```

Empty or default sections (`[lifecycle]`, `[dbus]`, `[container.env]`, etc.) are omitted automatically — the generated TOML stays concise (~25 lines).

### Container naming

When `podbox init <image>` is called without `--name`, the container name is derived from the image tag:

| Image ref | Container name |
|-----------|---------------|
| `fedora:44` | `fedora-44` |
| `fedora:latest` | `fedora` |
| `ubuntu:24.04` | `ubuntu-24-04` |
| `ghcr.io/user/img:v1` | `img-v1` |

This avoids name conflicts when creating containers from different tags of the same base image. Use `--name` to override explicitly.

### What happens

1. `podbox init` creates the TOML config
2. `podbox build` auto-generates a Containerfile, copies in the guest binary, and runs `podman build`
3. `podbox enable` installs Quadlet systemd units (socket + container + host service)
4. `podbox start` starts the container via `podman start`
5. `podbox enter <name>` opens an interactive shell

---

## Daily Usage

Use `podbox use` to set an active context — then all commands target that container without needing `-C` or a name:

```bash
# Set active context
podbox use myenv

# All commands now target myenv by default
podbox status           # "myenv [running]"
podbox logs             # shows journalctl output
podbox exec -- htop     # runs inside myenv
podbox stop
podbox start

# Other containers still work via explicit name
podbox status fedora
podbox enter fedora
```

Alternatively, pass the name directly on each command:

```bash
# Open a shell
podbox enter myenv

# Run a command
podbox exec myenv -- htop

# Launch a GUI app
podbox run firefox

# Export an app to your host launcher
podbox export app firefox

# Export a binary to ~/.local/bin
podbox export bin rg

# Run a command on the host from inside the container (requires host_exec = true)
podbox exec -- host-exec echo "hello from host"

# Check container status
podbox status myenv

# View logs
podbox logs myenv -f

# Stop and start
podbox stop myenv
podbox start myenv
```

---

## Commands at a Glance

| Command | Description |
|---------|-------------|
| `podbox init` | List available profiles |
| `podbox init <image>` | Scaffold a custom config from a base image |
| `podbox init -i` | Interactive wizard (custom or profile) |
| `podbox init --profile <name>` | Scaffold from a prebuilt profile |
| `podbox create <name>` | Init → build → enable → start in one step |
| `podbox create <image> --name <n>` | Pull + create config + enable + start |
| `podbox build [<name>]` | Build the container image |
| `podbox enable [<name>]` | Install Quadlet systemd files |
| `podbox disable [<name>] [--force]` | Remove Quadlet files (`--force` bypasses config loading) |
| `podbox remove [<name>] [--all] [--force]` | Remove container (add `--all` for home dir too) |
| `podbox remove --stale [--force]` | Clean up orphaned/failed containers |
| `podbox start [<name>]` | Start the container |
| `podbox stop [<name>]` | Stop the container |
| `podbox enter [<name>]` | Enter a running container (auto-starts) |
| `podbox shell [<name>]` | Open interactive shell (auto-detect) |
| `podbox exec [<name>] -- <cmd>` | Run a command |
| `podbox run <app>` | Launch a GUI app |
| `podbox status [<name>]` | Show container state |
| `podbox logs [<name>] [-f] [--since <time>]` | Show container logs |
| `podbox diff [<name>]` | Compare installed packages against config |
| `podbox pull <name>` | Pull a prebuilt image without building |
| `podbox use [<name>] [--clear]` | Manage active context |
| `podbox find-definition [<name>]` | Print path to the matching config TOML |
| `podbox export app / bin` | Export to host |
| `podbox doctor` | Diagnose common issues |

All commands support `--dry-run` to preview without side effects.

---

## Next Steps

- [Configuration Reference](config.md) — all TOML keys
- [Architecture Overview](architecture.md) — how podbox works
- [Desktop Integration](export.md) — exporting apps and binaries
- [Troubleshooting Guide](troubleshooting.md) — common issues
