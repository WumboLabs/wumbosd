#!/bin/sh
# wumbosd release-tag verifier (DEP-004): fail-closed authenticity check for
# official release tags. An official release tag is an ANNOTATED tag signed
# with the dedicated wumbOS release-signing SSH key; trust is anchored in the
# single-entry allowed-signers file next to this script (deploy/trusted-
# signers, principal "wumbos-release", public key only). Unsigned, wrongly
# signed, malformed, and lightweight tags all fail; see
# docs/RELEASE_PROCESS.md. Global git configuration is never read or written:
# all settings are passed per command.
#
# WUMBOS_TRUST_ANCHOR (portable installs): when set, verification uses that
# allowed-signers file INSTEAD of this script's own deploy/trusted-signers,
# with exactly the same fail-closed validation. Deployed hosts pin their
# trust anchor outside any candidate release and verify updates against the
# pinned copy (docs/PORTABLE_INSTALL.md); a candidate release can never
# weaken or rotate the anchor it is verified with.
#
# Usage: deploy/verify-release-tag.sh <tag>
# Exit codes:
#   0  verified
#   2  usage error
#   3  trust anchor missing or malformed
#   4  unknown or unreadable ref
#   5  not an annotated tag
#   6  signature verification failed
set -u

fail() {
    echo "verify-release-tag: $1" >&2
    exit "$2"
}

[ "$#" -eq 1 ] || fail "usage: verify-release-tag.sh <tag>" 2
TAG=$1
case "$TAG" in
    '' | -* | *[!A-Za-z0-9._-]*) fail "invalid tag name: $TAG" 2 ;;
esac

git rev-parse --git-dir >/dev/null 2>&1 || fail "not inside a wumbosd git checkout" 4

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Pinned-anchor override (see header): same validation either way; the
# default remains this script's own single-entry anchor.
TRUST="${WUMBOS_TRUST_ANCHOR:-$SCRIPT_DIR/trusted-signers}"
[ -f "$TRUST" ] || fail "no trust anchor: $TRUST is missing (release signing not provisioned)" 3
[ "$(awk 'NF && !/^#/' "$TRUST" | wc -l)" -eq 1 ] \
    || fail "trust anchor must contain exactly one key entry" 3
awk 'NF && !/^#/ {print $1}' "$TRUST" | grep -qx 'wumbos-release' \
    || fail 'trust anchor principal must be wumbos-release' 3

git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null || fail "tag not found: $TAG" 4
[ "$(git cat-file -t "refs/tags/$TAG")" = "tag" ] \
    || fail "$TAG is not an annotated tag; lightweight tags are not verifiable releases" 5
COMMIT=$(git rev-parse "refs/tags/$TAG^{commit}") \
    || fail "tag does not resolve to a commit: $TAG" 4

OUTPUT=$(git -c gpg.format=ssh -c gpg.ssh.allowedSignersFile="$TRUST" verify-tag "$TAG" 2>&1)
[ "$?" -eq 0 ] || {
    echo "$OUTPUT" >&2
    fail "signature verification failed for $TAG" 6
}

echo "VERIFIED $TAG -> $COMMIT"
echo "$OUTPUT" | grep -m1 '^Good ' || true
exit 0
