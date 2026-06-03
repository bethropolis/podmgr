# Desktop Integration (Export)

`podbox` can expose applications and binaries from inside the container to the host desktop — generating `.desktop` files, extracting icons, and creating shell shims.

---

## Commands

| Command | Description |
|---------|-------------|
| `podbox export app <name>` | Export a `.desktop` application |
| `podbox export bin <name>` | Create a binary shim in `~/.local/bin` |

Applications and binaries are declared in the config under `[integration.export]`:

```toml
[integration.export]
apps = ["gedit", "nautilus"]
bins = ["rg", "gcc"]
```

---

## App Export

`podbox export app` extracts a desktop application from the container and makes it launchable from the host.

### Step-by-step

1. **Read the `.desktop` file** from the container at `/usr/share/applications/<name>.desktop` via `podman exec`.

2. **Rewrite the `Exec=` line** so launching the desktop entry runs through `podbox exec` inside the container:

    ```ini
    Exec=gedit %F
    ```
    becomes:
    ```
    Exec=podbox --container myenv exec -- gedit %F
    ```

    All other keys (`Name=`, `Icon=`, `MimeType=`, etc.) are preserved unchanged.

3. **Extract the icon** by trying common paths inside the container:

    ```
    /usr/share/icons/hicolor/{48,64,128,256}x{48,64,128,256}/apps/<name>.png
    /usr/share/icons/hicolor/scalable/apps/<name>.svg
    ```

    The first match is copied to:

    ```
    ~/.local/share/icons/podbox/<container>/<name>.<ext>
    ```

4. **Write the `.desktop` file** to:

    ```
    ~/.local/share/applications/podbox-<container>-<name>.desktop
    ```

5. **Run `update-desktop-database`** on the applications directory (failure is non-fatal; a warning is printed).

### MIME type handling

`MimeType=` lines in the original `.desktop` file are preserved as-is. The host desktop environment registers the container app as a handler for those MIME types. When a user opens a file of that type, the rewritten `Exec=` line dispatches through `podbox exec`.

!!! info ""
    MIME registration is handled entirely by the host desktop environment via the standard `.desktop` file mechanism — no additional configuration is needed.

---

## Binary Export

`podbox export bin` creates a shell shim so a container binary appears on the host `PATH`.

### Generated shim

A script is written to `~/.local/bin/<name>`:

```sh
#!/bin/sh
exec podbox --container "<name>" exec -- "<bin>" "$@"
```

The shim is executable (`chmod 755`). If `~/.local/bin` is on the user's `PATH` — which most distributions add by default — the binary appears as if installed locally.

---

## Cleanup

Remove exported files for a container by calling:

```rust
podbox::export::unexport_all(container_name)
```

This removes:

- All `~/.local/share/applications/podbox-<container>-*.desktop` files
- The `~/.local/share/icons/podbox/<container>/` directory tree
- Any shims in `~/.local/bin/` whose content references the container name

!!! warning ""
    `podbox remove` does **not** automatically call unexport. Run `podbox export` commands or call `unexport_all` separately before removing the container.
