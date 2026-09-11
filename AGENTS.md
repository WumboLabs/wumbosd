# wumbosd Agent Guide

## Project

`wumbosd` is the native Rust user-session service for wumbOS.

The current live reference platform is Fedora Linux 44 with Hyprland, Wayland,
and Quickshell.

The sibling shell repository is:

`../wumbo-quickshell`

Do not modify the shell repository unless a milestone explicitly authorizes it.

## To-do list

At the start of each non-trivial task, provide a concise to-do list before performing the work. Use the environment's native task/todo mechanism when available; otherwise provide the list directly in the response. Keep it updated as work progresses and mark items complete as they are finished.

## Architecture

- Rust-first.
- Lean user-session service.
- Event-driven by default.
- No root requirement for normal runtime.
- No package-manager calls from runtime code.
- No unnecessary long-running child processes.
- No polling when an event or signal interface exists.
- Add dependencies only when they materially simplify or improve the design.
- Prefer established system/session APIs over custom protocols.
- Session D-Bus is the preferred initial IPC boundary unless current evidence
  establishes a better fit.

## Distribution boundary

- `deploy/` holds release and lifecycle tooling: `build-release.sh` (build),
  `sign-manifest.sh` and `sign-release-tag.sh` (explicit human signing with
  `--key`), `verify-release-tag.sh`, `wumbosdctl` (client lifecycle tool),
  and the public `trusted-signers` anchor. Build, sign, and publish are
  strictly separated; no helper pushes or publishes.
- The release payload is exactly: `wumbosd`, `wumbosdctl`, `RELEASE`, the
  systemd units and drop-in, and the D-Bus activation file. Release
  manifests are signed with the `wumbos-release` identity (DEP-004);
  automated tests use ephemeral Ed25519 identities only.
- Client-side installs live under
  `${XDG_DATA_HOME:-$HOME/.local/share}/wumbos/components/wumbosd/` with the
  pinned trust anchor outside every release directory; see
  `docs/PORTABLE_DISTRIBUTION.md`.
- `tests/portable/run-tests.sh` and
  `tests/portable/real-binary-proof.sh` are deterministic and touch no
  production host, unit, or key.

## Foundation boundary

Foundation work should establish service lifecycle, IPC, health/status,
versioning, errors, and logging.

Do not add speculative capabilities such as:

- notification routing;
- background jobs;
- approval inbox;
- AI/model orchestration;
- databases;
- plugin systems;
- compute mesh;
- remote control;

unless the current milestone explicitly requests them.

## Portability

Do not hardcode:

- usernames;
- home-directory paths;
- hostnames;
- monitor connectors;
- network interfaces;
- machine-specific hardware;
- personal application rules.

Use standard user/session/runtime directories and discover system state where
required.

## Git and reports

Do not stage, commit, amend, push, tag, reset, clean, or rewrite history unless
the user explicitly authorizes it for the current milestone.

Milestone reports belong under `tmp/<milestone-name>/REPORT.md` and must not be
committed. The final response ends with `REPORT: /absolute/path/to/REPORT.md`.

Before returning from a milestone, any required report must exist and be
nonempty.

## Validation

Use the smallest relevant validation for the current milestone.

For Rust changes, normally include:

- `cargo fmt --check`
- `cargo check`
- `cargo test`

Use Clippy when available and appropriate.

Do not claim unavailable tooling ran.
