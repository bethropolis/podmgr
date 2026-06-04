---
description: Common podbox issues — container startup, D-Bus proxy, Wayland, interceptors, UID mapping, SSH agent forwarding, build failures, and shell hangs.
---

# Troubleshooting

Running `podbox doctor` first is recommended — it checks the most common issues automatically and explains what to fix.

---

### Container won't start

```bash
systemctl --user status <name>.service   # check the unit status
podbox logs                              # check container output
```

If the unit failed immediately after `podbox enable`, the Quadlet file may be malformed. Run `podbox enable --dry-run` to inspect the generated files without writing them.

---

### D-Bus proxy fails or container hangs on startup

`xdg-dbus-proxy` is missing or not on `PATH`. Install it from your distro's package manager (`xdg-dbus-proxy` on most distros). Alternatively, set `dbus = false` under `[integration]` if you don't need D-Bus access.

```bash
which xdg-dbus-proxy   # should return a path
```

---

### GUI apps don't appear / Wayland socket errors

Verify `$WAYLAND_DISPLAY` is set on the host before starting the container. The socket is resolved at `podbox enable` time — if it changed after a reboot, re-run `podbox enable` to regenerate the Quadlet with the correct socket path.

```bash
echo $WAYLAND_DISPLAY                    # should be wayland-0 or similar
podbox disable && podbox enable          # regenerate Quadlets
podbox stop && podbox start
```

---

### Interceptors not working (notify-send, xdg-open, clipboard, host-exec)

The `podbox-guest` daemon connects to the host socket on container startup. If it can't connect, interceptors are silently skipped.

```bash
systemctl --user status <name>.socket           # check the socket unit
podbox exec -- ps aux | grep podbox-guest       # check daemon running
podbox exec -- echo $PATH                       # check /run/podbox/bin
```

If the socket unit is inactive, run `podbox enable` and `podbox start` again. If the daemon is running but `PATH` is wrong, the `/etc/environment.d/podbox.conf` file may not have been written — check with `podbox exec -- cat /etc/environment.d/podbox.conf`.

---

### UID mismatch or permission errors inside bind mounts

`UserNS=keep-id` maps your host UID into the container with a shift of 1 (host UID 1000 → container UID 999). Do not run `chown` on bind-mounted directories from inside the container — it will change ownership on the host through the idmapped mount.

If files appear owned by `nobody` inside the container, the mount was created before the UID mapping was set up. Stop the container, check the volume path exists on the host with the correct ownership, then start again.

---

### SSH agent not forwarding

SSH agent forwarding requires Podman >= 5.6 and `ssh_agent = true` in `[integration]`. Verify both:

```bash
podman --version                         # must be >= 5.6
grep ssh_agent ~/.config/podbox/<name>.toml
```

If you're on Podman 5.5, the socket path is baked at `podbox enable` time. If `$SSH_AUTH_SOCK` changed since then (e.g. new login session), re-run `podbox disable && podbox enable`.

---

### Build fails or produces a stale image

Run `podbox build --rebuild` to force a full rebuild from scratch, bypassing the lock file. If the build context is corrupted:

```bash
rm -rf ~/.local/share/podbox/<name>/     # clear build context
podbox build --rebuild
```

---

### Container starts but `podbox shell` hangs

The shell binary specified in `container.shell` may not be installed in the image. Check your `image.packages.install` list includes the shell package, then run `podbox build --rebuild`.
