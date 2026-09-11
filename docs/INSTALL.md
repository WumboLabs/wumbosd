# Install

`wumbosd` supports two installation paths: a versioned binary release
(recommended for any host that is not a development checkout) and a source
checkout (development).

## Requirements

- Linux x86_64. The qualified reference platform is the current
  Fedora/wumbOS user-session environment (Fedora 44 at the time of writing).
- A systemd user session with socket activation and a session D-Bus bus.
- `systemctl` and `ssh-keygen` (OpenSSH) on `PATH`.
- `python3` >= 3.12 — required only by the `wumbosdctl` lifecycle tool at
  operator-invocation time, never by the daemon itself.
- `busctl` (optional): used once per install to reload D-Bus activation
  configuration; without it the change defers to the next session.
- A Rust toolchain is required only to build from source.

The daemon has no configuration file and no persistent on-disk state.

## Binary release install (recommended)

Follow [PORTABLE_DISTRIBUTION.md](PORTABLE_DISTRIBUTION.md) for the full
contract. Summary:

```sh
# Obtain the three release files out of band, then:
deploy/wumbosdctl install wumbosd-<v>-linux-x86_64.tar.gz \
    --manifest release-manifest.json --signature release-manifest.json.sig \
    --trust-anchor deploy/trusted-signers        # stops at the human gate
deploy/wumbosdctl install wumbosd-<v>-linux-x86_64.tar.gz \
    --manifest release-manifest.json --signature release-manifest.json.sig \
    --trust-anchor deploy/trusted-signers --seed-trust   # attests and installs
wumbosdctl status
```

On first install the anchor fingerprint must be compared out of band against
the fingerprint published with the release announcement. Updates and
rollbacks then need only the artifact set and one command each; rollback
needs nothing but the local machine.

## Source install (development)

```sh
cargo build --release
```

Then install the units from a checkout you have reviewed:

- `systemd/wumbosd.socket` and `systemd/wumbosd.service` into
  `~/.config/systemd/user/`, with `ExecStart` pointing at your built binary
  (the units use `%h/.local/bin/wumbosd`).
- For production notification ownership, additionally follow
  [NOTIFICATION_INGESTION.md](NOTIFICATION_INGESTION.md) (drop-in and D-Bus
  activation file, including its rollback contract).

Run `systemctl --user daemon-reload`, then
`systemctl --user enable --now wumbosd.socket`.

### Standalone development run

```sh
cargo run
```

Without socket activation, wumbosd creates `$XDG_RUNTIME_DIR/wumbos` (mode
`0700`), binds `wumbos/wumbosd.sock` (mode `0600`), and removes only that
socket on a clean exit.

## Uninstall

There is no uninstall command in v1. Manual, ordered removal:

```sh
systemctl --user disable --now wumbosd.socket
systemctl --user stop wumbosd.service
rm -f ~/.config/systemd/user/wumbosd.service.d/notifications.conf
rm -f ~/.config/systemd/user/wumbosd.service ~/.config/systemd/user/wumbosd.socket
rm -f ~/.local/share/dbus-1/services/fr.emersion.mako.service
rm -f ~/.local/bin/wumbosd
systemctl --user daemon-reload
busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/wumbos/components/wumbosd"
```

Only remove the D-Bus activation file if this contract installed it (see
[NOTIFICATION_INGESTION.md](NOTIFICATION_INGESTION.md) for the takeover
backup rules). Uninstalling does not disturb unrelated `$XDG_RUNTIME_DIR/wumbos`
content belonging to other components.
