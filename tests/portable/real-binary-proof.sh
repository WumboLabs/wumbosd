#!/bin/sh
# Real-binary artifact proof for the wumbosd portable distribution contract.
# Builds a release-shaped artifact from the REAL cargo release binary and the
# REAL systemd/D-Bus payload files, signs it with an EPHEMERAL Ed25519
# identity, and verifies the full trust chain as an outsider would.
#
# This proves artifact structure, executable modes, manifest hash binding,
# signature verification, extraction, and portable-layout validity. It never
# uses the persistent human release key and never installs anything: the
# live daemon on this host is not touched.
#
# Unlike deploy/build-release.sh (which refuses dirty trees because release
# artifacts must trace to a clean commit), this proof works on the current
# tree on purpose: it qualifies the artifact format, not a release candidate.
#
# Usage: tests/portable/real-binary-proof.sh [output-dir]
# Exit codes: 0 all proofs passed; nonzero otherwise.
set -u

REPO=$(git rev-parse --show-toplevel 2>/dev/null) || {
    echo "real-binary-proof: not inside a git checkout" >&2
    exit 2
}
OUT=${1:-"$REPO/tmp/real-artifact-proof"}
rm -rf "$OUT"
mkdir -p "$OUT/keys"

fail() {
    echo "FAIL $1" >&2
    exit 1
}
ok() { echo "ok $1"; }

# 1. Real release binary.
if [ ! -f "$REPO/target/release/wumbosd" ]; then
    (cd "$REPO" && cargo build --release) || fail "cargo build --release"
fi
[ -s "$REPO/target/release/wumbosd" ] || fail "release binary missing"
ok "release binary present: $(du -h "$REPO/target/release/wumbosd" | cut -f1)"

# 2. Ephemeral signing identity and matching anchor.
ssh-keygen -t ed25519 -N '' -C 'wumbosd real-artifact proof (ephemeral)' \
    -f "$OUT/keys/release" >/dev/null || fail "ephemeral keygen"
printf 'wumbos-release %s\n' "$(cat "$OUT/keys/release.pub")" > "$OUT/keys/anchor"

# 3. Assemble the real payload.
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$REPO/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || fail "cannot read package version"
NAME="wumbosd-$VERSION-linux-x86_64"
PAYLOAD="$OUT/$NAME"
mkdir -p "$PAYLOAD/systemd/wumbosd.service.d" "$PAYLOAD/dbus-1/services"
cp "$REPO/target/release/wumbosd" "$PAYLOAD/wumbosd"
cp "$REPO/deploy/wumbosdctl" "$PAYLOAD/wumbosdctl"
COMMIT=$(git rev-parse HEAD)
cat > "$PAYLOAD/RELEASE" <<EOF
component=wumbosd
version=$VERSION
release-tag=v$VERSION
source-commit=$COMMIT
platform=linux
arch=x86_64
dbus-api-version=1
socket-protocol-version=1
attention-api-version=1
notification-api=freedesktop-notifications
EOF
cp "$REPO/systemd/wumbosd.service" "$PAYLOAD/systemd/wumbosd.service"
cp "$REPO/systemd/wumbosd.socket" "$PAYLOAD/systemd/wumbosd.socket"
cp "$REPO/systemd/wumbosd.service.d/notifications.conf" \
    "$PAYLOAD/systemd/wumbosd.service.d/notifications.conf"
cp "$REPO/dbus-1/services/fr.emersion.mako.service" \
    "$PAYLOAD/dbus-1/services/fr.emersion.mako.service"
find "$PAYLOAD" -type d -exec chmod 755 {} +
find "$PAYLOAD" -type f ! -name wumbosd ! -name wumbosdctl -exec chmod 644 {} +
ok "payload assembled ($NAME)"

# 4. Deterministic tarball + manifest.
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
    -C "$OUT" -cf - "$NAME" | gzip -n -9 > "$OUT/$NAME.tar.gz" || fail "tar/gzip"
SHA=$(sha256sum "$OUT/$NAME.tar.gz" | awk '{print $1}')
cat > "$OUT/release-manifest.json" <<EOF
{
  "manifest-version": 1,
  "component": "wumbosd",
  "version": "$VERSION",
  "release-tag": "v$VERSION",
  "source-commit": "$COMMIT",
  "platform": "linux",
  "arch": "x86_64",
  "artifact": "$NAME.tar.gz",
  "artifact-sha256": "$SHA",
  "dbus-api-version": 1,
  "socket-protocol-version": 1,
  "attention-api-version": 1,
  "notification-api": "freedesktop-notifications"
}
EOF

# 5. Sign with the ephemeral identity through the real signing helper.
"$REPO/deploy/sign-manifest.sh" --key "$OUT/keys/release" \
    --manifest "$OUT/release-manifest.json" \
    --trust-anchor "$OUT/keys/anchor" > "$OUT/sign.log" 2>&1 \
    || { cat "$OUT/sign.log" >&2; fail "sign-manifest"; }
ok "manifest signed (ephemeral key) and helper self-verification passed"

# 6. Outsider verification of the full chain.
"$REPO/deploy/wumbosdctl" verify-manifest \
    --manifest "$OUT/release-manifest.json" \
    --signature "$OUT/release-manifest.json.sig" \
    --artifact "$OUT/$NAME.tar.gz" \
    --trust-anchor "$OUT/keys/anchor" > "$OUT/verify.log" 2>&1 \
    || { cat "$OUT/verify.log" >&2; fail "outsider verify-manifest"; }
ok "outsider verification passed (signature -> manifest -> artifact SHA-256)"

# 7. Structure and mode proof of the artifact itself.
# GNU tar -tvzf lines: <mode> <owner/group> <size> <date> <time> <path>
tar -tvzf "$OUT/$NAME.tar.gz" | awk '{print $1, $NF}' > "$OUT/tar-listing.txt"
PROBLEMS=$(python3 - "$OUT/tar-listing.txt" "$NAME" <<'PY'
import sys

listing, root = sys.argv[1], sys.argv[2]
seen = {}
for line in open(listing, encoding="utf-8"):
    parts = line.split(None, 1)
    if len(parts) != 2:
        continue
    mode, path = parts[0], parts[1].strip()
    if path.endswith("/"):
        seen[path.rstrip("/")] = "d" + mode[1:]
        continue
    seen[path] = mode
required = {
    root: "drwxr-xr-x",
    root + "/wumbosd": "-rwxr-xr-x",
    root + "/wumbosdctl": "-rwxr-xr-x",
    root + "/RELEASE": "-rw-r--r--",
    root + "/systemd": "drwxr-xr-x",
    root + "/systemd/wumbosd.service.d": "drwxr-xr-x",
    root + "/systemd/wumbosd.service.d/notifications.conf": "-rw-r--r--",
    root + "/systemd/wumbosd.service": "-rw-r--r--",
    root + "/systemd/wumbosd.socket": "-rw-r--r--",
    root + "/dbus-1": "drwxr-xr-x",
    root + "/dbus-1/services": "drwxr-xr-x",
    root + "/dbus-1/services/fr.emersion.mako.service": "-rw-r--r--",
}
problems = []
for path, mode in required.items():
    actual = seen.get(path)
    if actual is None:
        problems.append("missing: %s" % path)
    elif actual != mode:
        problems.append("mode %s on %s (expected %s)" % (actual, path, mode))
for path in sorted(set(seen) - set(required)):
    problems.append("unexpected: %s" % path)
for problem in problems:
    print(problem)
sys.exit(1 if problems else 0)
PY
) || fail "artifact structure/modes:
$PROBLEMS"
ok "artifact structure and modes exact (12 entries, 755 binaries, 644 data)"

# 8. Extraction through the client's safe path.
python3 - "$OUT/$NAME.tar.gz" "$OUT/extracted" "$VERSION" <<'PY'
import os
import sys
import tarfile

artifact, destination, version = sys.argv[1], sys.argv[2], sys.argv[3]
with tarfile.open(artifact, "r:gz") as tar:
    tar.extractall(destination, filter="data")
root = os.path.join(destination, "wumbosd-%s-linux-x86_64" % version)
assert os.access(os.path.join(root, "wumbosd"), os.X_OK), "daemon not executable"
assert os.access(os.path.join(root, "wumbosdctl"), os.X_OK), "wumbosdctl not executable"
release = dict(
    line.strip().split("=", 1)
    for line in open(os.path.join(root, "RELEASE"), encoding="utf-8")
    if line.strip()
)
assert release["component"] == "wumbosd" and release["version"] == version, release
print("extracted payload valid (executables + RELEASE metadata)")
PY
ok "extraction through client path valid; portable layout usable"

# 9. Tamper spot-check: one altered artifact byte must fail verification.
# Flip the byte (XOR), never overwrite with a fixed value: writing 'X' onto
# a byte that is already 'X' would silently leave the artifact intact.
python3 - "$OUT/$NAME.tar.gz" "$OUT/tampered.tar.gz" <<'PY'
import sys

data = bytearray(open(sys.argv[1], "rb").read())
data[300] ^= 0xFF
open(sys.argv[2], "wb").write(bytes(data))
PY
if "$REPO/deploy/wumbosdctl" verify-manifest \
    --manifest "$OUT/release-manifest.json" \
    --signature "$OUT/release-manifest.json.sig" \
    --artifact "$OUT/tampered.tar.gz" \
    --trust-anchor "$OUT/keys/anchor" >/dev/null 2>&1; then
    fail "tampered artifact unexpectedly verified"
fi
ok "tampered artifact fails verification (fail-closed)"

echo "REAL-BINARY PROOF PASS (artifact: $OUT/$NAME.tar.gz; never installed)"
