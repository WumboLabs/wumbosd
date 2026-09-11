# Support

`wumbosd` is alpha software and a component of the wumbOS desktop shell
system.

## Supported

- The binary release lifecycle on the qualified platform: Linux x86_64 on a
  current Fedora user session (systemd user manager, session D-Bus,
  Hyprland/Wayland class desktop), installed and managed with `wumbosdctl`
  per [PORTABLE_DISTRIBUTION.md](PORTABLE_DISTRIBUTION.md).
- Install, explicit update, offline rollback, and status reporting for the
  versioned component layout.
- The documented D-Bus and socket APIs
  ([DBUS_API.md](DBUS_API.md), [ATTENTION_API.md](ATTENTION_API.md),
  [SOCKET_API.md](SOCKET_API.md),
  [NOTIFICATION_INGESTION.md](NOTIFICATION_INGESTION.md)); breaking API
  changes require new versioned interfaces, per those contracts.

## Not supported

- Other architectures, non-systemd user sessions, or other distributions:
  untested, undescribed, and not claimed to work.
- Background or automatic updates: all lifecycle changes are explicit
  operator invocations.
- Notification features the freedesktop implementation deliberately does not
  provide: automatic expiry, action icons, markup, images, sound, and
  persistence (see [NOTIFICATION_INGESTION.md](NOTIFICATION_INGESTION.md)).
- Trust rotation of the pinned release identity (out of scope for v1).

## Reporting problems

Use the GitHub Issues at `https://github.com/WumboLabs/wumbosd`. Include the
output of `wumbosdctl status`, the exact command run, and relevant
`journalctl --user -u wumbosd.service` excerpts. Do not include private
notification content or secrets; the daemon logs contain none.

Security-relevant findings must not go through public issues. Use GitHub
private vulnerability reporting on `WumboLabs/wumbosd` (Security tab,
"Report a vulnerability") once it is enabled for the repository; until then,
coordinate privately with the WumboLabs maintainers, whose security contact
is published with the release announcement.

## What support does not include

wumbosd is one component of a wumbOS installation. Shell presentation
issues, Hyprland configuration, or other components belong to their own
repositories and support boundaries.
