# Configuration Reference

`podmgr` searches for a definition file in this order:

1. `./.podmgr.toml` (project-local)
2. `~/.config/podmgr/*.toml` (first file, sorted by name)
3. Embedded default (`fedora:44`, name `podmgr`)

---

## `[image]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `base` | string | *required* | Base container image (e.g. `"fedora:41"`) |
| `name` | string | *required* | Image tag name (e.g. `"myenv"`) |

### `[image.packages]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `install` | string[] | `[]` | Packages to install via `dnf install` |
| `remove` | string[] | `[]` | Packages to remove via `dnf remove` |

### `[image.run]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `commands` | string[] | `[]` | Extra `RUN` commands in the Containerfile |

```toml
[image]
base = "fedora:41"
name = "myenv"

[image.packages]
install = ["git", "gcc", "ripgrep"]
remove = ["vim-minimal"]

[image.run]
commands = ["dnf clean all"]
```

---

## `[container]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *required* | Container name (used for systemd unit names, socket paths) |
| `home` | string | *required* | Host path for isolated home (`~` expands) |
| `shell` | string | `"bash"` | Default login shell inside the container |

### `[container.mounts]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `extra` | string[] | `[]` | Extra `Volume=` lines (e.g. `"~/Work:/home/user/Work:z"`) |

### `[container.env]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `*` | string | — | Arbitrary environment variables passed to the container |

```toml
[container]
name = "myenv"
home = "~/containers/myenv"
shell = "zsh"

[container.mounts]
extra = ["~/Projects:/home/user/Projects:z"]

[container.env]
EDITOR = "nvim"
TERM = "xterm-256color"
```

---

## `[integration]`

Controls which host resources are shared with the container.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `wayland` | bool | `true` | Share Wayland socket for GUI apps |
| `audio` | bool | `true` | Share PipeWire/PulseAudio sockets |
| `gpu` | string/bool | `"auto"` | GPU passthrough (`true`, `false`, `"auto"`, `"nvidia"`) |
| `dbus` | bool | `true` | Enable D-Bus session bus access |
| `notify` | bool | `false` | Desktop notification forwarding |
| `xdg_open` | bool | `false` | URI opening via host (`xdg-open`) |
| `clipboard` | bool | `false` | Clipboard sharing |
| `sync_fonts` | bool | `false` | Bind-mount `~/.fonts` (read-only). Only top-level dirs to keep `.local`/`.config` writable |
| `sync_icons` | bool | `false` | Bind-mount `~/.icons` (read-only) |
| `sync_themes` | bool | `false` | Bind-mount `~/.themes` (read-only). Only top-level dirs to keep `.local`/`.config` writable |

### `GpuMode` values

| TOML value | Meaning |
|------------|---------|
| `"auto"` (default) | Detect available GPU devices at runtime |
| `true` | Enable `/dev/dri` (Intel/AMD) |
| `false` | Disable all GPU passthrough |
| `"nvidia"` | Enable `/dev/dri` + NVIDIA device nodes |

### `[integration.xdg_dirs]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `documents` | bool | `false` | Mount host `~/Documents` |
| `downloads` | bool | `false` | Mount host `~/Downloads` |
| `pictures` | bool | `false` | Mount host `~/Pictures` |
| `music` | bool | `false` | Mount host `~/Music` |
| `videos` | bool | `false` | Mount host `~/Videos` |
| `desktop` | bool | `false` | Mount host `~/Desktop` |
| `projects` | bool | `false` | Mount host `~/Projects` |

### `[integration.export]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `apps` | string[] | `[]` | App `.desktop` files to export (without `.desktop` suffix) |
| `bins` | string[] | `[]` | Binary shims to generate in `~/.local/bin` |

```toml
[integration]
wayland    = true
audio      = true
gpu        = "auto"
dbus       = true
notify     = true
xdg_open   = true
clipboard  = true
sync_fonts = true
sync_icons = true
sync_themes = true

[integration.xdg_dirs]
documents = true
downloads = true
projects = true

[integration.export]
apps = ["gedit", "nautilus"]
bins = ["rg", "gcc"]
```

---

## `[lifecycle]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `quadlet` | bool | `false` | Generate Quadlet systemd files on `podmgr enable` |
| `autostart` | bool | `false` | Start container on user login (`WantedBy=default.target`) |
| `on_stop` | string | `"keep"` | Container behavior on stop (`"keep"` or `"remove"`) |
| `auto_update` | bool | `false` | Add `Label=io.containers.autoupdate=registry` for auto-updates |

```toml
[lifecycle]
quadlet     = true
autostart   = true
on_stop     = "keep"
auto_update = true
```

---

## `[systemd]`

Custom systemd unit dependencies for the generated Quadlet.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `requires` | string[] | `[]` | Units that must be active before the container (`Requires=`) |
| `after` | string[] | `[]` | Units the container should start after (`After=`) |

```toml
[systemd]
requires = ["postgres.service", "redis.service"]
after    = ["network-online.target"]
```

---

## `[dbus]`

D-Bus access control via `xdg-dbus-proxy`. Requires `integration.dbus = true`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `talk` | string[] | `[]` | D-Bus services the container can call (two-way) |
| `own` | string[] | `[]` | D-Bus services the container can register on the host bus |

```toml
[dbus]
talk = [
    "org.freedesktop.Notifications",
    "org.mpris.MediaPlayer2.*",
]
own = [
    "org.mpris.MediaPlayer2.podmgr_app",
]
```

See [dbus-proxy.md](dbus-proxy.md) for details.

### Behavior matrix

| `integration.dbus` | `[dbus]` talk/own | Result |
|--------------------|-------------------|--------|
| `false` | any | No D-Bus access |
| `true` | empty (default) | Unfiltered `Volume=%t/bus` |
| `true` | populated | Proxy socket via `xdg-dbus-proxy` |

---

## Full Example

```toml
[image]
base = "fedora:41"
name = "myenv"

[image.packages]
install = ["git", "gcc", "ripgrep"]
remove = []

[image.run]
commands = ["dnf clean all"]

[container]
name = "myenv"
home = "~/containers/myenv"
shell = "bash"

[container.mounts]
extra = ["~/Work:/home/user/Work:z"]

[container.env]
EDITOR = "nvim"
TERM   = "xterm-256color"

[integration]
wayland   = true
audio     = true
gpu       = "auto"
dbus      = true
notify    = true
xdg_open  = true
clipboard = true

[integration.xdg_dirs]
documents = true
downloads = true
projects = true

[integration.export]
apps = ["gedit", "nautilus"]
bins = ["rg", "gcc"]

[lifecycle]
quadlet     = true
autostart   = true
on_stop     = "keep"
auto_update = true

[systemd]
requires = ["postgres.service"]
after    = ["network-online.target"]

[dbus]
talk = ["org.freedesktop.Notifications"]
```
