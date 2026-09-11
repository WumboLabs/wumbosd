#!/bin/sh
# wumbosd release-tag signing helper (DEP-004). Creates an ANNOTATED, SSH-
# signed release tag with the dedicated wumbOS release-signing key and then
# IMMEDIATELY verifies it with deploy/verify-release-tag.sh. The private key
# is taken only from --key (or WUMBOS_RELEASE_KEY); it is never discovered,
# read, or printed, and its contents never enter logs. This helper never
# pushes or publishes; a failed verification leaves the local tag in place
# for inspection and must stop the release. See docs/RELEASE_PROCESS.md.
#
# Usage: deploy/sign-release-tag.sh --key <private-key-path> [--message <text>] <tag>
# Exit codes: 0 signed and verified; nonzero otherwise.
set -u

die() {
    echo "sign-release-tag: $1" >&2
    exit 1
}

KEY=${WUMBOS_RELEASE_KEY:-}
MESSAGE=
TAG=
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
        --message)
            [ "$#" -ge 2 ] || die "--message needs text"
            MESSAGE=$2
            shift 2
            ;;
        --message=*)
            MESSAGE=${1#--message=}
            shift
            ;;
        -*)
            die "unknown option: $1"
            ;;
        *)
            [ -z "$TAG" ] || die "exactly one tag expected"
            TAG=$1
            shift
            ;;
    esac
done

[ -n "$KEY" ] || die "no signing key: pass --key <path> (the release key is never auto-discovered)"
[ -n "$TAG" ] || die "usage: sign-release-tag.sh --key <private-key-path> [--message <text>] <tag>"
case "$TAG" in
    '' | -* | *[!A-Za-z0-9._-]*) die "invalid tag name: $TAG" ;;
esac
[ -f "$KEY" ] || die "signing key not found: $KEY"

PERMS=$(stat -c %a "$KEY" 2>/dev/null || echo '?')
case "$PERMS" in
    600 | 400) : ;;
    *) die "refusing weak key permissions ($PERMS): chmod 600 the key file first" ;;
esac

git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null && die "tag already exists: $TAG"

MESSAGE_ARGS_SET=
[ -n "$MESSAGE" ] && MESSAGE_ARGS_SET=1
git -c gpg.format=ssh -c user.signingkey="$KEY" tag -a -s ${MESSAGE_ARGS_SET:+"-m"} ${MESSAGE_ARGS_SET:+"$MESSAGE"} "$TAG" \
    || die "tag creation failed"

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
"$SCRIPT_DIR/verify-release-tag.sh" "$TAG" || die "VERIFICATION FAILED for $TAG - do not push this tag"
