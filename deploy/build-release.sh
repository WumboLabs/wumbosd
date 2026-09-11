#!/bin/sh
# wumbosd release builder: builds the release candidate artifact and emits
# the unsigned release manifest. BUILD ONLY: this helper never signs, never
# pushes, and never publishes. Signing is a separate explicit human step
# (deploy/sign-manifest.sh); publishing is a separate human gate
# (docs/RELEASE_PROCESS.md).
#
# The source state must be clean (no tracked modifications, no untracked
# non-ignored files) so the artifact is reproducibly traceable to the HEAD
# commit recorded in the manifest.
#
# Usage: deploy/build-release.sh [--release-tag <tag>] [--out <dir>]
# Output: <out>/wumbosd-<version>-linux-x86_64.tar.gz
#         <out>/release-manifest.json          (UNSIGNED)
# Exit codes: 0 built; nonzero otherwise.
set -u

die() {
    echo "build-release: $1" >&2
    exit 1
}

REPO=$(git rev-parse --show-toplevel 2>/dev/null) || die "not inside a git checkout"
TAG=
OUT="$REPO/dist"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --release-tag)
            [ "$#" -ge 2 ] || die "--release-tag needs a value"
            TAG=$2
            shift 2
            ;;
        --release-tag=*)
            TAG=${1#--release-tag=}
            shift
            ;;
        --out)
            [ "$#" -ge 2 ] || die "--out needs a value"
            OUT=$2
            shift 2
            ;;
        --out=*)
            OUT=${1#--out=}
            shift
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

case "$(uname -s):$(uname -m)" in
    Linux:x86_64) : ;;
    *) die "release targets Linux x86_64; this host is $(uname -s):$(uname -m)" ;;
esac

[ -n "$(git status --porcelain)" ] && die "source tree is not clean; commit or clean first (HEAD is what the manifest will attest)"
COMMIT=$(git rev-parse HEAD) || die "cannot resolve HEAD"
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$REPO/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || die "cannot read the package version from Cargo.toml"
case "$VERSION" in
    *[!0-9A-Za-z.-]*) die "package version has unexpected characters: $VERSION" ;;
esac
[ -n "$TAG" ] || TAG="v$VERSION"
case "$TAG" in
    "v$VERSION") : ;;
    *) die "release tag must be 'v' + the Cargo version for v1 manifests ($TAG != v$VERSION)" ;;
esac

command -v cargo >/dev/null 2>&1 || die "cargo not found"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum not found"

echo "build-release: cargo build --release (wumbosd $VERSION, $TAG, $COMMIT)"
(cd "$REPO" && cargo build --release) || die "cargo build failed"
BINARY="$REPO/target/release/wumbosd"
[ -f "$BINARY" ] || die "release binary missing: $BINARY"

ARTIFACT="wumbosd-$VERSION-linux-x86_64"
PAYLOAD="$OUT/$ARTIFACT"
rm -rf "$PAYLOAD"
mkdir -p "$PAYLOAD/systemd/wumbosd.service.d" "$PAYLOAD/dbus-1/services" || die "cannot stage payload"

cp "$BINARY" "$PAYLOAD/wumbosd" || die "cannot copy daemon binary"
cp "$REPO/deploy/wumbosdctl" "$PAYLOAD/wumbosdctl" || die "cannot copy wumbosdctl"
chmod 755 "$PAYLOAD/wumbosd" "$PAYLOAD/wumbosdctl"

cat > "$PAYLOAD/RELEASE" <<EOF
component=wumbosd
version=$VERSION
release-tag=$TAG
source-commit=$COMMIT
platform=linux
arch=x86_64
dbus-api-version=1
socket-protocol-version=1
attention-api-version=1
notification-api=freedesktop-notifications
EOF

cp "$REPO/systemd/wumbosd.service" "$PAYLOAD/systemd/wumbosd.service" || die "cannot copy service unit"
cp "$REPO/systemd/wumbosd.socket" "$PAYLOAD/systemd/wumbosd.socket" || die "cannot copy socket unit"
cp "$REPO/systemd/wumbosd.service.d/notifications.conf" \
    "$PAYLOAD/systemd/wumbosd.service.d/notifications.conf" || die "cannot copy drop-in"
cp "$REPO/dbus-1/services/fr.emersion.mako.service" \
    "$PAYLOAD/dbus-1/services/fr.emersion.mako.service" || die "cannot copy D-Bus activation file"

find "$PAYLOAD" -type d -exec chmod 755 {} +
find "$PAYLOAD" -type f ! -name wumbosd ! -name wumbosdctl -exec chmod 644 {} +

mkdir -p "$OUT" || die "cannot create output directory $OUT"
TARBALL="$OUT/$ARTIFACT.tar.gz"
# Controlled, reproducible archive: fixed member order/metadata, no gzip name
# or timestamp. The hash of these exact bytes goes into the manifest.
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
    -C "$OUT" -cf - "$ARTIFACT" | gzip -n -9 > "$TARBALL" || die "tar/gzip failed"

SHA=$(sha256sum "$TARBALL" | awk '{print $1}')
case "$SHA" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*) : ;;
    *) die "unexpected sha256sum output" ;;
esac

MANIFEST="$OUT/release-manifest.json"
cat > "$MANIFEST" <<EOF
{
  "manifest-version": 1,
  "component": "wumbosd",
  "version": "$VERSION",
  "release-tag": "$TAG",
  "source-commit": "$COMMIT",
  "platform": "linux",
  "arch": "x86_64",
  "artifact": "$ARTIFACT.tar.gz",
  "artifact-sha256": "$SHA",
  "dbus-api-version": 1,
  "socket-protocol-version": 1,
  "attention-api-version": 1,
  "notification-api": "freedesktop-notifications"
}
EOF

echo "build-release: self-check (fields + hash; signature comes later)"
"$REPO/deploy/wumbosdctl" verify-manifest \
    --manifest "$MANIFEST" --artifact "$TARBALL" --allow-unsigned \
    || die "self-check failed"

echo "BUILD OK (unsigned)"
echo "  artifact:  $TARBALL"
echo "  manifest:  $MANIFEST (UNSIGNED)"
echo "next: deploy/sign-manifest.sh --key <release-key-path> --manifest $MANIFEST"
