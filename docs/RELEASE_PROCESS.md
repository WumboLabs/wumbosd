# Release process

This is the maintainer process for turning a validated `wumbosd` state into
an official, verifiable release and publishing it. It separates BUILD, SIGN,
and PUBLISH at every step; no helper ever pushes or publishes.

## Release boundaries

- Public `main` represents an installable public state.
- Active development stays on focused feature branches. Validate an accepted
  candidate before promoting it to `main` with guarded fast-forward
  semantics (`git update-ref` compare-and-swap for important local refs; no
  force-pushes of accepted public history).
- Push only refs explicitly approved for publication.

## Pre-release validation

From the expected clean Git state:

```sh
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
git diff --check
tests/portable/run-tests.sh          # deterministic distribution contract
tests/portable/real-binary-proof.sh  # real-artifact structure/chain proof
```

The portable tests use disposable roots and ephemeral signing identities
only; the persistent release key is never touched by tooling. Complete
release-aware documentation and privacy checks (no private paths, hosts, or
credentials) before promoting.

## Promotion and signed tags

Official release tags are ANNOTATED and SIGNED with the dedicated wumbOS
release-signing key (Ed25519, principal `wumbos-release`, DEP-004 — the same
organizational trust root the Shell uses):

```sh
deploy/sign-release-tag.sh --key <release-key-path> --message "<notes>" <tag>
```

The helper signs, immediately verifies with `deploy/verify-release-tag.sh`,
and fails the release on any verification failure. It never pushes.
Unsigned, lightweight, foreign-signed, and malformed tags all fail
verification; never relax the anchor to make verification pass.

## Release artifact and manifest

```sh
deploy/build-release.sh                 # BUILD: artifact + UNSIGNED manifest
deploy/sign-manifest.sh --key <release-key-path> --manifest dist/release-manifest.json
```

`build-release.sh` refuses a dirty tree, so the artifact always traces to the
HEAD commit recorded in the manifest. `sign-manifest.sh` attaches a detached
SSHSIG (namespace `wumbosd-release`) to the manifest and then verifies the
full chain; a verification failure stops the release. The release set is
exactly:

```
dist/wumbosd-<version>-linux-x86_64.tar.gz
dist/release-manifest.json
dist/release-manifest.json.sig
```

The signed tag authenticates source history; the signed manifest
authenticates the binary artifact via its SHA-256. Both are required; GitHub
transport alone authenticates nothing.

## Keys and trust anchors

- The release-signing private key is human-controlled, provisioned under
  `~/.local/share/wumbos/release-signing/`, never stored in the repository,
  `tmp/`, logs, or reports, and never invoked by automation. Helpers take it
  only from `--key` (or `WUMBOS_RELEASE_KEY`) and refuse weak file modes.
- Public trust material only: `deploy/trusted-signers` (single
  `wumbos-release` Ed25519 entry, fingerprint
  `SHA256:1DLVAw/cLCy1zl5gXIbMbynNNU/hI5ALzF5a+LyvueQ`) and, on installed
  hosts, the pinned copy under the component trust directory.
- Every release announcement must publish the anchor fingerprint so the
  bootstrap human gate on a second host has an out-of-band comparison.

## First public release

The first public tag is recommended to be `v0.1.0-alpha.1`: the component
carries `version = "0.1.0"` and no prior public releases, the shell's alpha
tag convention is `v0.1.0-alpha.N`, and wumbOS is explicitly alpha. The
approved release commit sets `version = "0.1.0-alpha.1"` in `Cargo.toml`
(the version is observable through the D-Bus and socket hello metadata), and
the tag is `v` + that version, exactly as the manifest binding requires.

## Publication sequence (human gates)

1. Final local Git gate: review the full diff on the milestone branch, then
   promote to `main`.
2. Create the GitHub repository `WumboLabs/wumbosd`:
   - visibility PUBLIC (the publication audit in the milestone report must
     be clean first);
   - description: `wumbOS user-session service daemon (notifications,
     attention events, socket transport)`;
   - default branch `main`; license MPL-2.0;
   - topics: `wumbos`, `rust`, `dbus`, `systemd`, `notifications`, `wayland`.
3. Add the approved `origin` remote locally.
4. Push only approved `main`; never `--all`/`--mirror`/wildcards. Feature
   branches, `tmp/`, and reports stay local.
5. Outsider check: clean public clone; verify the intended branch inventory,
   documentation, and license.
6. Create the first signed component release per the sections above (release
   commit, signed tag, build, sign manifest).
7. Push only the exact approved release tag.
8. Create a GitHub prerelease for the tag; upload exactly the artifact,
   manifest, and signature — nothing else.
9. Outsider verification: fetch the three release files, run
   `deploy/wumbosdctl verify-manifest --manifest release-manifest.json
   --signature release-manifest.json.sig --artifact <artifact>.tar.gz
   --trust-anchor deploy/trusted-signers` from the public clone, and compare
   the anchor fingerprint with the announcement.
10. Record the release evidence report.

## Second-host deployment gate

The distribution contract is qualified on disposable environments; a real
deployment still owes the live checks `wumbosdctl preflight`, install per
[PORTABLE_DISTRIBUTION.md](PORTABLE_DISTRIBUTION.md), `systemctl --user`
socket activation observed on the real user manager, one update/rollback
cycle, and a reboot persistence observation.
