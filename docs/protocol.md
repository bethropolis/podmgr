# Host-Guest Socket Protocol

## Wire Format

Length-prefixed JSON over a Unix stream socket:

```
┌─────────────────────────┬──────────────────────────────┐
│  4 bytes (big-endian)   │  N bytes (UTF-8 JSON)        │
│  payload length = N     │                              │
└─────────────────────────┴──────────────────────────────┘
```

---

## Socket Location

| Socket | Path | Created by |
|--------|------|------------|
| Host socket | `$XDG_RUNTIME_DIR/podbox/<name>.sock` | `.socket` Quadlet unit |
| Local guest socket | `/run/podbox/guest-<name>.sock` | `podbox-guest --daemon` |

The host socket is created by systemd before the container starts and persists across restarts. The guest socket is used by interceptor processes to communicate with the local daemon.

---

## Handshake

**Guest sends:**

```json
{
  "type": "hello",
  "version": "0.1.0",
  "container": "myenv",
  "capabilities": ["notify", "xdg_open", "clipboard", "host_exec"]
}
```

**Host responds:**

```json
{
  "type": "hello_ack",
  "accepted": ["notify", "xdg_open"],
  "rejected": ["clipboard", "host_exec"]
}
```

The handshake establishes which capabilities the host allows. The guest daemon only installs interceptor symlinks for accepted capabilities.

---

## Message Types

### Guest → Host

| Type | Fields |
|------|--------|
| `hello` | `version`, `container`, `capabilities` |
| `notify` | `summary`, `body`, `urgency`, `actions` (optional), `app_name` (optional) |
| `notify_action_result` | `key` |
| `xdg_open` | `uri` |
| `clipboard_set` | `text` |
| `clipboard_get` | — |
| `host_exec` | `command` |
| `host_exec_stdout` | `data` |
| `host_exec_stderr` | `data` |
| `host_exec_done` | `exit_code` |

### Host → Guest

| Type | Fields |
|------|--------|
| `hello_ack` | `accepted`, `rejected` |
| `clipboard_data` | `text` |
| `ping` | — |
| `shutdown` | — |

---

## Notify Actions

When present, `actions` is an array of objects with a `key` and `label`:

```json
{
  "type": "notify",
  "summary": "Build complete",
  "body": "Exit code: 0",
  "actions": [
    { "key": "open", "label": "Open project" },
    { "key": "dismiss", "label": "Dismiss" }
  ]
}
```

The guest sends `notify_action_result` with the user-selected `key` back to the host.

!!! info ""
    The `actions` and `app_name` fields use `#[serde(default)]` for backward compatibility with older guest binaries that do not send them.

---

## Capabilities

Each capability corresponds to an interceptor symlink installed by the guest daemon:

| Capability | Interceptor | Description |
|------------|-------------|-------------|
| `notify` | `notify-send` | Desktop notification forwarding |
| `xdg_open` | `xdg-open` | URI opening via host |
| `clipboard` | `podbox-clipboard` | Clipboard sharing |
| `host_exec` | `host-exec` | Execute commands on host |

!!! info ""
    Capabilities not accepted during handshake are silently skipped — no symlink is created and the guest does not attempt to use them.
