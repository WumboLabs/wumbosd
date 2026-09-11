# Portable distribution

This is the operator and maintainer contract for distributing `wumbosd` as a
versioned binary component: installing it on a normal host without a Rust
toolchain or source checkout, updating it with one command, and rolling back
offline. The qualified target is Linux x86_64 on the current Fedora/wumbOS
user-session environment; no universal Linux portability is claimed.

Status: the canonical repository is public at
https://github.com/WumboLabs/wumbosd. Official releases follow the signed
release process (see [RELEASE_PROCESS.md](RELEASE_PROCESS.md)).

## Release artifact and trust chain

An official release is exactly three files:

```
wumbosd-<version>-linux-x86_64.tar.gz   the artifact (binary + runtime units)
release-manifest.json                   binds version, commit, hash, platform
release-manifest.json.sig               detached SSHSIG signature of the manifest
```

The payload contains only what the runtime needs: the `wumbosd` binary, the
`wumbosdctl` lifecycle tool, the two systemd user units plus the
`notifications.conf` drop-in, the `fr.emersion.mako.service` D-Bus activation
override, and a `RELEASE` metadata file. No source tree, build tree, logs, or
state.

Authenticity is fail-closed at every stage:

1. A signed annotated Git tag authenticates the source history (DEP-004,
   [release process](RELEASE_PROCESS.md)).
2. The pinned WumboLabs release identity (`wumbos-release`, the same
   organizational Ed25519 root the Shell uses, anchor in
   `deploy/trusted-signers`) verifies the manifest's detached SSHSIG
   signature, namespace `wumbosd-release`.
3. The verified manifest binds the artifact SHA-256.
4. The verified SHA-256 authenticates the extracted payload, whose member
   set and modes are validated against an exact allowlist.

A signed tag alone does NOT authenticate uploaded binaries; the signed
manifest does. GitHub transport is never trusted by itself, checksums are
never trusted without the authenticated manifest, and the signing private key
never exists on client hosts.

## Install layout and ownership

Everything lives under XDG paths:

```
${XDG_DATA_HOME:-$HOME/.local/share}/wumbos/components/wumbosd/
    releases/<version>/   materialized, immutable release trees
    current               symlink -> releases/<version>   (active release)
    previous              symlink -> releases/<version>   (rollback target)
    trust/trusted-signers PINNED client trust anchor
    state.json            wumbosdctl's own install state

$HOME/.local/bin/wumbosd                 symlink -> .../current/wumbosd
                                         (stable activation path; the units
                                         keep using %h/.local/bin/wumbosd)
$XDG_CONFIG_HOME/systemd/user/wumbosd.service
$XDG_CONFIG_HOME/systemd/user/wumbosd.socket
$XDG_CONFIG_HOME/systemd/user/wumbosd.service.d/notifications.conf
$XDG_DATA_HOME/dbus-1/services/fr.emersion.mako.service
```

Ownership classification:

| Class | Contents | Update/rollback behavior |
| --- | --- | --- |
| RELEASE-OWNED | `releases/<version>/`; the deployed unit, drop-in, D-Bus activation, and activation-symlink files listed above | change with the selected release |
| USER/MACHINE STATE | nothing today: the daemon keeps all event and notification history in memory only | n/a; wumbosdctl never touches state it does not own |
| EPHEMERAL | `$XDG_RUNTIME_DIR/wumbos/wumbosd.sock`, in-memory history | never required to reconstruct the install |

Old releases are never deleted automatically. Mutable daemon state does not
exist inside release directories.

## Pinned trust and rotation

The anchor is pinned at `trust/trusted-signers`, outside every candidate
release directory. Candidates are verified ONLY against the pinned anchor (or,
at bootstrap, against an explicitly supplied anchor that the human attests).
A candidate artifact cannot carry, replace, or weaken the anchor it is
verified with; the payload allowlist rejects any smuggled trust material, and
re-seeding over an existing anchor is refused. Key rotation is out of scope
for v1 and requires an explicit human-authorized trust-rotation procedure.

## Commands

`wumbosdctl` is a local user-level tool. It runs nothing in the background,
never polls, performs no network I/O of any kind: the operator obtains the
three release files out of band (e.g. from the GitHub Release). All commands
honor `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and `HOME`.

### Bootstrap and install

```sh
wumbosdctl install wumbosd-<v>-linux-x86_64.tar.gz \
    --manifest release-manifest.json --signature release-manifest.json.sig \
    --trust-anchor <allowed-signers-file>
```

The first install verifies the manifest against the supplied anchor and then
STOPS (exit 3) at a mandatory human gate: compare the printed fingerprint
against the fingerprint published with the release announcement out of band.
Completing the install requires attesting it:

```sh
wumbosdctl install ... --trust-anchor <anchor-file> --seed-trust
```

`--seed-trust` pins the anchor and completes the install: materialize the
release, adopt-or-refuse external targets, activate `current`, deploy the
units, `daemon-reload`, enable the socket, best-effort reload of the D-Bus
activation configuration, and post-install validation.

An existing `~/.local/bin/wumbosd`, unit, drop-in, or D-Bus activation file
that this contract does not own is never seized: the install refuses and
reports exactly what exists. Only files byte-identical to the new payload may
be taken over, and only with explicit `--adopt`.

### Update

```sh
wumbosdctl update wumbosd-<v+1>-linux-x86_64.tar.gz \
    --manifest release-manifest.json --signature release-manifest.json.sig
```

Verifies against the pinned anchor, materializes the new release separately,
validates it, activates it atomically, reconciles units if they changed,
reloads systemd, and restarts the service only if it is currently running
(socket activation otherwise picks the new binary up on next trigger). The
old release becomes `previous` and is retained. Any failure before
activation leaves the active release untouched; a failure after it is
recoverable with `rollback`. Updating to a version that is already active is
a no-op; the artifact bytes must match the ones originally installed for that
version or the update refuses.

### Rollback

```sh
wumbosdctl rollback
```

Swaps `current` and `previous` locally. No network, no artifact, no rebuild.
Units from the restored release are redeployed if needed, systemd is
reloaded, and only the wumbosd service is restarted if it was running.
Running rollback again returns to the release you rolled back from; the
newer release is never deleted.

### Status and preflight

```sh
wumbosdctl status      # current/previous versions, release identity,
                       # pinned fingerprint, materialized releases, unit
                       # paths and systemd state, expected listener
wumbosdctl preflight   # read-only host readiness report (platform, python,
                       # required commands, writability, foreign installs)
wumbosdctl verify-manifest --manifest M --signature S --artifact A \
    --trust-anchor F   # standalone chain verification (outsider flow)
```

## systemd and D-Bus semantics

The existing architecture is preserved: the socket unit owns
`$XDG_RUNTIME_DIR/wumbos/wumbosd.sock` and activates the foreground service;
the service adopts fd 3 and never unlinks the systemd-owned socket. The
stable activation symlink means units never encode release-version paths and
no broken-executable interval exists during updates (symlink swaps are
atomic renames). The notification drop-in and D-Bus activation file behave
exactly as documented in
[NOTIFICATION_INGESTION.md](NOTIFICATION_INGESTION.md); the bus reload after
install is best-effort and defers to the next session on failure — the
session bus is never restarted.

Versioned unit changes ship inside the release payload and are deployed
transactionally on update/rollback (staged, then atomically moved), keeping
rollback able to restore the matching units.

## What is proven, and what is not

Deterministic qualification (`tests/portable/run-tests.sh`, no network, no
production host touched) covers fresh install, update, rollback, re-update,
tampered artifact, tampered manifest, unsigned manifest, untrusted signer,
candidate trust-root substitution, invalid artifact/extraction, foreign
existing installs, state preservation through A→B→A→B, offline rollback, and
status content. `tests/portable/real-binary-proof.sh` qualifies a real
cargo-release artifact (structure, modes, chain verification, extraction).

Not proven until a live second host: a real systemd user manager accepting
the units, real socket activation, and reboot persistence. Those are the
explicit human/live gates at deployment time (see
[RELEASE_PROCESS.md](RELEASE_PROCESS.md)).

## Qualification seams

`WUMBOSD_COMPONENT_ROOT`, `WUMBOSD_SYSTEMCTL`, and `WUMBOSD_BUSCTL` override
the component root and the systemctl/busctl binaries for disposable test
environments. Defaults are the real commands; the seams exist for the test
suite and for qualification roots, not for production redirection.
