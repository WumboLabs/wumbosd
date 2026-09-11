#!/bin/sh
# wumbosd release-manifest signing helper. SIGN ONLY: attaches a detached
# SSHSIG signature (namespace wumbosd-release) to a release manifest using
# the dedicated wumbOS release-signing key, then IMMEDIATELY verifies the
# full manifest/artifact/signature chain. This helper never pushes or
# publishes, and it never discovers the private key on its own: the key is
# taken only from --key (or WUMBOS_RELEASE_KEY) and its contents are never
# read, printed, or logged. See docs/RELEASE_PROCESS.md.
#
# Usage: deploy/sign-manifest.sh --key <private-key-path> --manifest <release-manifest.json>
#        [--artifact <override>] [--trust-anchor <allowed-signers>]
# Exit codes: 0 signed and verified; nonzero otherwise.
set -u

die() {
    echo "sign-manifest: $1" >&2
    exit 1
}

KEY=${WUMBOS_RELEASE_KEY:-}
MANIFEST=
ARTIFACT=
ANCHOR=

while [ "$#" -gt 0 ]; do
    case "$1" in
        --key)
            [ "$#" -ge 2 ] || die "--key needs a path"
            KEY=$2
            shift 2
            ;;
        --key=*)
            KEY=${1#--key=}
            shift
            ;;
        --manifest)
            [ "$#" -ge 2 ] || die "--manifest needs a path"
            MANIFEST=$2
            shift 2
            ;;
        --manifest=*)
            MANIFEST=${1#--manifest=}
            shift
            ;;
        --artifact)
            [ "$#" -ge 2 ] || die "--artifact needs a path"
            ARTIFACT=$2
            shift 2
            ;;
        --artifact=*)
            ARTIFACT=${1#--artifact=}
            shift
            ;;
        --trust-anchor)
            [ "$#" -ge 2 ] || die "--trust-anchor needs a path"
            ANCHOR=$2
            shift 2
            ;;
        --trust-anchor=*)
            ANCHOR=${1#--trust-anchor=}
            shift
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

[ -n "$KEY" ] || die "no signing key: pass --key <path> (the release key is never auto-discovered)"
[ -n "$MANIFEST" ] || die "usage: sign-manifest.sh --key <path> --manifest <release-manifest.json>"
[ -f "$MANIFEST" ] || die "manifest not found: $MANIFEST"
[ -f "$KEY" ] || die "signing key not found: $KEY"

PERMS=$(stat -c %a "$KEY" 2>/dev/null || echo '?')
case "$PERMS" in
    600 | 400) : ;;
    *) die "refusing weak key permissions ($PERMS): chmod 600 the key file first" ;;
esac

case "$(basename "$MANIFEST")" in
    release-manifest.json) : ;;
    *) die "manifest must be named release-manifest.json" ;;
esac

REPO=$(git rev-parse --show-toplevel 2>/dev/null) || die "not inside a git checkout"
[ -n "$ANCHOR" ] || ANCHOR="$REPO/deploy/trusted-signers"
[ -f "$ANCHOR" ] || die "trust anchor not found: $ANCHOR"

if [ -z "$ARTIFACT" ]; then
    NAME=$(sed -n 's/.*"artifact": "\([^"]*\)".*/\1/p' "$MANIFEST" | head -n 1)
    [ -n "$NAME" ] || die "cannot read the artifact name from the manifest"
    ARTIFACT="$(dirname "$MANIFEST")/$NAME"
fi
[ -f "$ARTIFACT" ] || die "artifact not found: $ARTIFACT"

SIG="$MANIFEST.sig"
if [ -e "$SIG" ]; then
    die "signature already exists: $SIG (remove it deliberately before re-signing)"
fi

ssh-keygen -Y sign -f "$KEY" -n wumbosd-release "$MANIFEST" || die "signing failed"
[ -f "$SIG" ] || die "signing produced no signature file"

echo "sign-manifest: verifying the full chain with deploy/wumbosdctl"
"$REPO/deploy/wumbosdctl" verify-manifest \
    --manifest "$MANIFEST" --signature "$SIG" --artifact "$ARTIFACT" \
    --trust-anchor "$ANCHOR" \
    || {
        echo "sign-manifest: VERIFICATION FAILED - do not publish this manifest" >&2
        exit 1
    }

echo "SIGN OK (signed and verified; nothing was pushed or published)"
echo "  manifest:  $MANIFEST"
echo "  signature: $SIG"
echo "  artifact:  $ARTIFACT"
