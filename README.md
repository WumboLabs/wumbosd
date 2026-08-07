# wumbosd

`wumbosd` is the native Rust user-session service for wumbOS.

The project provides the small event-driven service layer underneath the
Quickshell-based wumbOS desktop.

Initial scope is intentionally narrow:

- clean service lifecycle;
- local session IPC;
- health and status reporting;
- version/build information;
- structured errors and logging;
- graceful operation when optional capabilities are absent.

Higher-level wumbOS capabilities will be added as separate milestones after the
foundation is stable.
