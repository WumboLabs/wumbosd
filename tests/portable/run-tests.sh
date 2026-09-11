#!/bin/sh
# Disposable end-to-end qualification for the wumbosd portable distribution
# contract. No production host, unit, daemon, or key is touched:
#
#   - temporary HOME/XDG roots under a mktemp directory;
#   - fixture component releases (A, B, C, ...) with harmless shell-script
#     daemon markers instead of real binaries;
#   - ephemeral Ed25519 signing identities generated under the temp root;
#   - faithful stubs for systemctl --user and busctl --user that implement
#     exactly the calls wumbosdctl makes and fail on anything else.
#
# The suite performs no network I/O. Real-binary release proof is a separate
# gate (docs/RELEASE_PROCESS.md).
#
# Usage: tests/portable/run-tests.sh [name-filter]
# Environment: KEEP=1 keeps the temporary roots for inspection.
set -u

REPO=$(git rev-parse --show-toplevel 2>/dev/null) || {
    echo "run-tests: not inside a git checkout" >&2
    exit 2
}
WUMBOSDCTL="$REPO/deploy/wumbosdctl"
FILTER=${1:-}

PASS=0
FAIL=0
CURRENT_TEST=

ROOT=$(mktemp -d "${TMPDIR:-/tmp}/wumbosd-portable.XXXXXX")
mkdir -p "$ROOT/logs" "$ROOT/bin" "$ROOT/keys" "$ROOT/artifacts"

cleanup() {
    if [ "${KEEP:-0}" = 1 ]; then
        echo "run-tests: temporary roots kept under $ROOT"
    else
        rm -rf "$ROOT"
    fi
}
trap cleanup EXIT

note() { printf '%s\n' "$*"; }
ok() { PASS=$((PASS + 1)); printf 'ok %s\n' "$*"; }
bad() {
    FAIL=$((FAIL + 1))
    printf 'FAIL %s\n' "$*"
}
begin() {
    CURRENT_TEST=$1
    if [ -n "$FILTER" ] && ! case "$CURRENT_TEST" in *"$FILTER"*) true ;; *) false ;; esac; then
        CURRENT_TEST=skipped
        return 0
    fi
    printf '\n== %s ==\n' "$CURRENT_TEST"
}
running() { [ "$CURRENT_TEST" != "skipped" ]; }

check() {
    # check <description> <simple-command args...>  (no shell operators)
    description=$1
    shift
    if "$@"; then
        ok "$description"
    else
        bad "$description (test: ${CURRENT_TEST:-?})"
    fi
}

check_not() {
    description=$1
    shift
    if "$@"; then
        bad "NOT: $description (test: ${CURRENT_TEST:-?})"
    else
        ok "$description"
    fi
}

log_has() { grep -q "$1" "$ROOT/logs/$2.log"; }
log_has_e() { grep -Eq "$1" "$ROOT/logs/$2.log"; }

# ---------------------------------------------------------------- fixtures

make_stub_systemctl() {
    STUB_STATE_DIR=$1
    cat > "$ROOT/bin/systemctl" <<STUB
#!/bin/sh
# Faithful stub for the systemctl --user calls wumbosdctl makes.
STATE_DIR="$STUB_STATE_DIR"
mkdir -p "\$STATE_DIR"
log() { echo "systemctl \$*" >> "\$STATE_DIR/log"; }
get() { sed -n "s/^\$1=//p" "\$STATE_DIR/state" 2>/dev/null | tail -n 1; }
setvar() {
    touch "\$STATE_DIR/state"
    grep -v "^\$1=" "\$STATE_DIR/state" > "\$STATE_DIR/state.new" 2>/dev/null
    mv "\$STATE_DIR/state.new" "\$STATE_DIR/state"
    echo "\$1=\$2" >> "\$STATE_DIR/state"
}
[ "\$1" = "--user" ] || { echo "stub: expected --user, got: \$*" >&2; exit 99; }
shift
case "\$1" in
    daemon-reload) log "\$*" ;;
    enable)
        [ "\$2" = "--now" ] || { echo "stub: unexpected enable form: \$*" >&2; exit 99; }
        [ "\$3" = "wumbosd.socket" ] || { echo "stub: unexpected unit: \$3" >&2; exit 99; }
        setvar socket-enabled 1
        setvar socket-active 1
        log "\$*"
        ;;
    is-enabled)
        case "\$2:\$(get socket-enabled)" in
            wumbosd.socket:1) echo enabled; exit 0 ;;
            wumbosd.socket:) echo not-found; exit 1 ;;
            *) echo disabled; exit 1 ;;
        esac
        ;;
    is-active)
        case "\$2" in
            wumbosd.socket)
                [ "\$(get socket-active)" = "1" ] && { echo active; exit 0; }
                echo inactive
                exit 3
                ;;
            wumbosd.service)
                [ "\$(get service-active)" = "1" ] && { echo active; exit 0; }
                echo inactive
                exit 3
                ;;
            *) echo "stub: unexpected unit: \$2" >&2; exit 99 ;;
        esac
        ;;
    try-restart)
        [ "\$2" = "wumbosd.service" ] || { echo "stub: unexpected unit: \$2" >&2; exit 99; }
        log "\$*"
        ;;
    is-system-running) echo running; exit 0 ;;
    *) echo "stub: unexpected call: \$*" >&2; exit 99 ;;
esac
exit 0
STUB
    chmod +x "$ROOT/bin/systemctl"
}

make_stub_busctl() {
    STUB_STATE_DIR=$1
    cat > "$ROOT/bin/busctl" <<STUB
#!/bin/sh
STATE_DIR="$STUB_STATE_DIR"
mkdir -p "\$STATE_DIR"
[ "\$1" = "--user" ] || { echo "stub: expected --user, got: \$*" >&2; exit 99; }
shift
echo "busctl \$*" >> "\$STATE_DIR/log"
case "\$1:\$5" in
    call:ReloadConfig) exit 0 ;;
    *) echo "stub: unexpected call: \$*" >&2; exit 99 ;;
esac
STUB
    chmod +x "$ROOT/bin/busctl"
}

make_release() {
    # make_release <version> <marker> [signing-key [extra=src ...]]
    # extra is <path-inside-payload>=<source-file>
    version=$1
    marker=$2
    if [ "$#" -ge 3 ]; then
        key=$3
        shift 3
    else
        key=$ROOT/keys/release
        shift 2
    fi
    name="wumbosd-$version-linux-x86_64"
    payload="$ROOT/artifacts/$name"
    dir="$ROOT/artifacts"
    rm -rf "$payload"
    mkdir -p "$payload/systemd/wumbosd.service.d" "$payload/dbus-1/services"
    cat > "$payload/wumbosd" <<EOF
#!/bin/sh
printf 'wumbosd fixture $version marker=$marker\n'
EOF
    cp "$WUMBOSDCTL" "$payload/wumbosdctl"
    chmod 755 "$payload/wumbosd" "$payload/wumbosdctl"
    commit=$(printf '%s' "$marker" | sha256sum | cut -c1-40)
    cat > "$payload/RELEASE" <<EOF
component=wumbosd
version=$version
release-tag=v$version
source-commit=$commit
platform=linux
arch=x86_64
dbus-api-version=1
socket-protocol-version=1
attention-api-version=1
notification-api=freedesktop-notifications
EOF
    cp "$REPO/systemd/wumbosd.service" "$payload/systemd/wumbosd.service"
    cp "$REPO/systemd/wumbosd.socket" "$payload/systemd/wumbosd.socket"
    cp "$REPO/systemd/wumbosd.service.d/notifications.conf" \
        "$payload/systemd/wumbosd.service.d/notifications.conf"
    cp "$REPO/dbus-1/services/fr.emersion.mako.service" \
        "$payload/dbus-1/services/fr.emersion.mako.service"
    for extra in "$@"; do
        target=${extra%%=*}
        source=${extra#*=}
        mkdir -p "$payload/$(dirname "$target")"
        cp "$source" "$payload/$target"
    done
    find "$payload" -type d -exec chmod 755 {} +
    find "$payload" -type f ! -name wumbosd ! -name wumbosdctl -exec chmod 644 {} +
    tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
        -C "$dir" -cf - "$name" | gzip -n -9 > "$dir/$name.tar.gz"
    sha=$(sha256sum "$dir/$name.tar.gz" | awk '{print $1}')
    cat > "$dir/release-manifest-$version.json" <<EOF
{
  "manifest-version": 1,
  "component": "wumbosd",
  "version": "$version",
  "release-tag": "v$version",
  "source-commit": "$commit",
  "platform": "linux",
  "arch": "x86_64",
  "artifact": "$name.tar.gz",
  "artifact-sha256": "$sha",
  "dbus-api-version": 1,
  "socket-protocol-version": 1,
  "attention-api-version": 1,
  "notification-api": "freedesktop-notifications"
}
EOF
    ssh-keygen -Y sign -f "$key" -n wumbosd-release \
        "$dir/release-manifest-$version.json" >/dev/null 2>&1
    [ -f "$dir/release-manifest-$version.json.sig" ] || {
        echo "run-tests: signing fixture release $version failed" >&2
        exit 2
    }
}

# ------------------------------------------------------------- environments

new_env() {
    envroot="$ROOT/env-$1"
    mkdir -p "$envroot/home/.local/share" "$envroot/home/.local/bin" "$envroot/home/.config"
    make_stub_systemctl "$envroot/stub"
    make_stub_busctl "$envroot/stub"
    printf '%s\n' "$envroot"
}

env_run() {
    # env_run <envroot> <logname> <wumbosdctl args...> -> rc
    env=$1
    logname=$2
    shift 2
    HOME="$env/home" XDG_DATA_HOME="$env/home/.local/share" \
        XDG_CONFIG_HOME="$env/home/.config" \
        WUMBOSD_SYSTEMCTL="$ROOT/bin/systemctl" WUMBOSD_BUSCTL="$ROOT/bin/busctl" \
        "$WUMBOSDCTL" "$@" > "$ROOT/logs/$logname.log" 2>&1
}

env_run_expect_ok() {
    env_run_result=0
    env_run "$@" || env_run_result=$?
    if [ "$env_run_result" -eq 0 ]; then
        ok "$2: exit 0"
        return 0
    fi
    bad "$2: expected success, got rc=$env_run_result (see $ROOT/logs/$2.log)"
    sed -n '1,8p' "$ROOT/logs/$2.log"
    return 1
}

env_run_expect_fail() {
    env_run "$@" && {
        bad "$2: expected failure but exit 0 (see $ROOT/logs/$2.log)"
        return 1
    }
    ok "$2: refused (exit nonzero)"
    return 0
}

env_run_expect_rc() {
    expected=$1
    shift
    env_run_result=0
    env_run "$@" || env_run_result=$?
    if [ "$env_run_result" -eq "$expected" ]; then
        ok "$2: exit $expected as required"
        return 0
    fi
    bad "$2: expected rc=$expected, got rc=$env_run_result (see $ROOT/logs/$2.log)"
    return 1
}

link_target() { basename "$(readlink "$1" 2>/dev/null)" 2>/dev/null; }
state_value() {
    sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$2" | head -n 1
}

# ------------------------------------------------------------------- setup

command -v ssh-keygen >/dev/null 2>&1 || {
    echo "run-tests: ssh-keygen is required" >&2
    exit 2
}
ssh-keygen -t ed25519 -N '' -C 'wumbosd portable qualification (ephemeral)' \
    -f "$ROOT/keys/release" >/dev/null
chmod 600 "$ROOT/keys/release"
ANCHOR="$ROOT/keys/anchor"
printf 'wumbos-release %s\n' "$(cat "$ROOT/keys/release.pub")" > "$ANCHOR"
ssh-keygen -t ed25519 -N '' -C 'untrusted qualification key (ephemeral)' \
    -f "$ROOT/keys/other" >/dev/null

make_release 0.9.0 alpha
make_release 0.9.1 bravo
make_release 0.9.2 charlie

note "wumbosd portable qualification"
note "root: $ROOT"

# ------------------------------------------------------------------- TEST 1

begin "TEST 1 fresh install A"
if running; then
    ENV1=$(new_env first)
    R1="$ENV1/home/.local/share/wumbos/components/wumbosd"
    ART_A="$ROOT/artifacts/wumbosd-0.9.0-linux-x86_64.tar.gz"
    MAN_A="$ROOT/artifacts/release-manifest-0.9.0.json"
    SIG_A="$MAN_A.sig"
    ART_B="$ROOT/artifacts/wumbosd-0.9.1-linux-x86_64.tar.gz"
    MAN_B="$ROOT/artifacts/release-manifest-0.9.1.json"
    SIG_B="$MAN_B.sig"

    env_run_expect_rc 3 "$ENV1" t1-gate install \
        "$ART_A" --manifest "$MAN_A" --signature "$SIG_A" --trust-anchor "$ANCHOR"
    check "t1 gate: no activation before seed-trust" test ! -L "$R1/current"
    check_not "t1 gate: no trust pinned before seed-trust" test -e "$R1/trust/trusted-signers"
    check_not "t1 gate: no units deployed before seed-trust" \
        test -e "$ENV1/home/.config/systemd/user/wumbosd.service"
    check "t1 gate: gate message shown" log_has "human gate" t1-gate
    check "t1 gate: verification reported for A" log_has "VERIFICATION OK for 0.9.0" t1-gate

    env_run_expect_ok "$ENV1" t1-install install \
        "$ART_A" --manifest "$MAN_A" --signature "$SIG_A" \
        --trust-anchor "$ANCHOR" --seed-trust
    check "t1 current -> A" test "$(link_target "$R1/current")" = 0.9.0
    check_not "t1 previous absent" test -L "$R1/previous"
    check "t1 activation symlink in place" \
        test "$(link_target "$ENV1/home/.local/bin/wumbosd")" = wumbosd
    check "t1 activation resolves into A" \
        test "$(basename "$(readlink -f "$ENV1/home/.local/bin/wumbosd")")" = wumbosd
    check "t1 service unit installed" \
        cmp -s "$ENV1/home/.config/systemd/user/wumbosd.service" \
            "$ROOT/artifacts/wumbosd-0.9.0-linux-x86_64/systemd/wumbosd.service"
    check "t1 socket unit installed" test -f "$ENV1/home/.config/systemd/user/wumbosd.socket"
    check "t1 drop-in installed" \
        test -f "$ENV1/home/.config/systemd/user/wumbosd.service.d/notifications.conf"
    check "t1 dbus activation file installed" \
        test -f "$ENV1/home/.local/share/dbus-1/services/fr.emersion.mako.service"
    check "t1 trust pinned" test -f "$R1/trust/trusted-signers"
    check "t1 trust identical to bootstrap anchor" cmp -s "$R1/trust/trusted-signers" "$ANCHOR"
    check "t1 state current=A" test "$(state_value current "$R1/state.json")" = 0.9.0
    env_run "$ENV1" t1-status status
    check "t1 status reports A" sh -c "grep -q '^current: 0.9.0$' '$ROOT/logs/t1-status.log'"
    check "t1 status reports pinned fingerprint" \
        sh -c "grep -q '^pinned trust: SHA256:' '$ROOT/logs/t1-status.log'"
    check "t1 fixture marker A" \
        sh -c "grep -q 'marker=alpha' '$ROOT/artifacts/wumbosd-0.9.0-linux-x86_64/wumbosd'"
    check "t1 no source checkout referenced" \
        sh -c "! grep -rq '$REPO' '$R1'"
fi

# ------------------------------------------------------------------- TEST 2

begin "TEST 2 update A -> B"
if running; then
    env_run_expect_ok "$ENV1" t2-update update "$ART_B" --manifest "$MAN_B" --signature "$SIG_B"
    check "t2 current -> B" test "$(link_target "$R1/current")" = 0.9.1
    check "t2 previous -> A" test "$(link_target "$R1/previous")" = 0.9.0
    check "t2 A retained" test -d "$R1/releases/0.9.0"
    check "t2 state previous=A" test "$(state_value previous "$R1/state.json")" = 0.9.0
    check "t2 daemon-reload issued" sh -c "grep -q 'daemon-reload' '$ENV1/stub/log'"
    check "t2 units updated from B payload" \
        cmp -s "$ENV1/home/.config/systemd/user/wumbosd.service" \
            "$ROOT/artifacts/wumbosd-0.9.1-linux-x86_64/systemd/wumbosd.service"
fi

# ------------------------------------------------------------------- TEST 3

begin "TEST 3 rollback B -> A"
if running; then
    env_run_expect_ok "$ENV1" t3-rollback rollback
    check "t3 current -> A" test "$(link_target "$R1/current")" = 0.9.0
    check "t3 B retained" test -d "$R1/releases/0.9.1"
    check "t3 state previous=B" test "$(state_value previous "$R1/state.json")" = 0.9.1
    check "t3 units restored from A payload" \
        cmp -s "$ENV1/home/.config/systemd/user/wumbosd.service" \
            "$ROOT/artifacts/wumbosd-0.9.0-linux-x86_64/systemd/wumbosd.service"
fi

# ------------------------------------------------------------------- TEST 4

begin "TEST 4 re-update A -> B"
if running; then
    env_run_expect_ok "$ENV1" t4-update update "$ART_B" --manifest "$MAN_B" --signature "$SIG_B"
    check "t4 current -> B without repair" test "$(link_target "$R1/current")" = 0.9.1
    env_run_expect_ok "$ENV1" t4-idempotent update "$ART_B" --manifest "$MAN_B" --signature "$SIG_B"
    check "t4 repeat update is a no-op" log_has "already active" t4-idempotent
fi

# ------------------------------------------------------------------- TEST 5

begin "TEST 5 tampered artifact"
if running; then
    cp "$ROOT/artifacts/wumbosd-0.9.2-linux-x86_64.tar.gz" \
        "$ROOT/artifacts/tampered.tar.gz"
    printf 'X' | dd of="$ROOT/artifacts/tampered.tar.gz" bs=1 seek=200 conv=notrunc 2>/dev/null
    env_run_expect_fail "$ENV1" t5-tampered update \
        "$ROOT/artifacts/tampered.tar.gz" \
        --manifest "$ROOT/artifacts/release-manifest-0.9.2.json" \
        --signature "$ROOT/artifacts/release-manifest-0.9.2.json.sig"
    check "t5 refusal names hash mismatch" log_has "SHA-256 mismatch" t5-tampered
    check "t5 current unchanged" test "$(link_target "$R1/current")" = 0.9.1
    check_not "t5 no partial materialization" test -e "$R1/releases/0.9.2"
fi

# ------------------------------------------------------------------- TEST 6

begin "TEST 6 tampered manifest"
if running; then
    # Tamper while keeping the manifest structurally valid, so the failure
    # is specifically signature verification, not JSON/format parsing.
    python3 - "$ROOT/artifacts/release-manifest-0.9.2.json" \
        "$ROOT/artifacts/release-manifest-0.9.2-tampered.json" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    manifest = json.load(handle)
commit = manifest["source-commit"]
manifest["source-commit"] = ("1" if commit[0] != "1" else "2") + commit[1:]
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, sort_keys=True)
    handle.write("\n")
PY
    env_run_expect_fail "$ENV1" t6-tampered update \
        "$ROOT/artifacts/wumbosd-0.9.2-linux-x86_64.tar.gz" \
        --manifest "$ROOT/artifacts/release-manifest-0.9.2-tampered.json" \
        --signature "$ROOT/artifacts/release-manifest-0.9.2.json.sig"
    check "t6 refusal is signature failure" log_has "signature verification FAILED" t6-tampered
    check "t6 current unchanged" test "$(link_target "$R1/current")" = 0.9.1
fi

# ------------------------------------------------------------------- TEST 7

begin "TEST 7 unsigned manifest"
if running; then
    env_run_expect_fail "$ENV1" t7-unsigned update \
        "$ROOT/artifacts/wumbosd-0.9.2-linux-x86_64.tar.gz" \
        --manifest "$ROOT/artifacts/release-manifest-0.9.2.json" \
        --signature "$ROOT/artifacts/does-not-exist.sig"
    check "t7 current unchanged" test "$(link_target "$R1/current")" = 0.9.1
    HOME="$ENV1/home" XDG_DATA_HOME="$ENV1/home/.local/share" \
        XDG_CONFIG_HOME="$ENV1/home/.config" \
        "$WUMBOSDCTL" verify-manifest \
        --manifest "$ROOT/artifacts/release-manifest-0.9.2.json" \
        --artifact "$ROOT/artifacts/wumbosd-0.9.2-linux-x86_64.tar.gz" \
        > "$ROOT/logs/t7-nosig.log" 2>&1
    rc=$?
    check "t7 verify-manifest requires signature" test "$rc" -ne 0
fi

# ------------------------------------------------------------------- TEST 8

begin "TEST 8 untrusted signer"
if running; then
    cp "$ROOT/artifacts/release-manifest-0.9.2.json" \
        "$ROOT/artifacts/release-manifest-0.9.2-untrusted.json"
    ssh-keygen -Y sign -f "$ROOT/keys/other" -n wumbosd-release \
        "$ROOT/artifacts/release-manifest-0.9.2-untrusted.json" >/dev/null 2>&1
    env_run_expect_fail "$ENV1" t8-untrusted update \
        "$ROOT/artifacts/wumbosd-0.9.2-linux-x86_64.tar.gz" \
        --manifest "$ROOT/artifacts/release-manifest-0.9.2-untrusted.json" \
        --signature "$ROOT/artifacts/release-manifest-0.9.2-untrusted.json.sig"
    check "t8 signature rejected against pinned anchor" \
        log_has "signature verification FAILED" t8-untrusted
    check "t8 current unchanged" test "$(link_target "$R1/current")" = 0.9.1
fi

# ------------------------------------------------------------------- TEST 9

begin "TEST 9 candidate trust-root substitution"
if running; then
    printf 'wumbos-release %s\n' "$(cat "$ROOT/keys/other.pub")" \
        > "$ROOT/artifacts/rogue-anchor"
    make_release 0.9.4 delta "$ROOT/keys/release" \
        "trust/trusted-signers=$ROOT/artifacts/rogue-anchor"
    ANCHOR_BEFORE=$(sha256sum "$R1/trust/trusted-signers" | awk '{print $1}')
    env_run_expect_fail "$ENV1" t9-substitute update \
        "$ROOT/artifacts/wumbosd-0.9.4-linux-x86_64.tar.gz" \
        --manifest "$ROOT/artifacts/release-manifest-0.9.4.json" \
        --signature "$ROOT/artifacts/release-manifest-0.9.4.json.sig"
    check "t9 smuggled anchor rejected despite valid signature" \
        log_has_e 'unexpected (directory|file) in artifact' t9-substitute
    check "t9 current unchanged" test "$(link_target "$R1/current")" = 0.9.1
    ANCHOR_AFTER=$(sha256sum "$R1/trust/trusted-signers" | awk '{print $1}')
    check "t9 pinned trust byte-identical" test "$ANCHOR_BEFORE" = "$ANCHOR_AFTER"
fi

# ------------------------------------------------------------------ TEST 10

begin "TEST 10 failed extraction / invalid artifact"
if running; then
    printf '#!/bin/sh\nevil\n' > "$ROOT/artifacts/evil.sh"
    make_release 0.9.5 echo "$ROOT/keys/release" \
        "evil.sh=$ROOT/artifacts/evil.sh"
    env_run_expect_fail "$ENV1" t10-invalid update \
        "$ROOT/artifacts/wumbosd-0.9.5-linux-x86_64.tar.gz" \
        --manifest "$ROOT/artifacts/release-manifest-0.9.5.json" \
        --signature "$ROOT/artifacts/release-manifest-0.9.5.json.sig"
    check "t10 unexpected payload entry rejected" \
        log_has_e 'unexpected (directory|file) in artifact' t10-invalid
    check "t10 current unchanged" test "$(link_target "$R1/current")" = 0.9.1
    check_not "t10 no partial activation" test -e "$R1/releases/0.9.5"
fi

# ------------------------------------------------------------------ TEST 11

begin "TEST 11 foreign existing install"
if running; then
    ENV2=$(new_env second)
    R2="$ENV2/home/.local/share/wumbos/components/wumbosd"
    printf 'foreign daemon\n' > "$ENV2/home/.local/bin/wumbosd"
    mkdir -p "$ENV2/home/.config/systemd/user/wumbosd.service.d"
    printf 'foreign drop-in\n' \
        > "$ENV2/home/.config/systemd/user/wumbosd.service.d/notifications.conf"
    FOREIGN_BIN_BEFORE=$(cat "$ENV2/home/.local/bin/wumbosd")
    FOREIGN_UNIT_BEFORE=$(cat "$ENV2/home/.config/systemd/user/wumbosd.service.d/notifications.conf")
    env_run_expect_fail "$ENV2" t11-foreign install \
        "$ART_A" --manifest "$MAN_A" --signature "$SIG_A" \
        --trust-anchor "$ANCHOR" --seed-trust
    check "t11 refusal reports foreign binary" log_has "refusing to seize" t11-foreign
    check "t11 foreign binary untouched" \
        test "$(cat "$ENV2/home/.local/bin/wumbosd")" = "$FOREIGN_BIN_BEFORE"
    check "t11 foreign unit untouched" \
        test "$(cat "$ENV2/home/.config/systemd/user/wumbosd.service.d/notifications.conf")" = "$FOREIGN_UNIT_BEFORE"
    check_not "t11 nothing activated" test -L "$R2/current"

    rm -f "$ENV2/home/.local/bin/wumbosd"
    cp "$ROOT/artifacts/wumbosd-0.9.0-linux-x86_64/systemd/wumbosd.service.d/notifications.conf" \
        "$ENV2/home/.config/systemd/user/wumbosd.service.d/notifications.conf"
    env_run_expect_fail "$ENV2" t11-adopt-required install \
        "$ART_A" --manifest "$MAN_A" --signature "$SIG_A" \
        --trust-anchor "$ANCHOR" --seed-trust
    check "t11 identical content still refused without --adopt" \
        log_has "adopt" t11-adopt-required
    env_run_expect_ok "$ENV2" t11-adopt install \
        "$ART_A" --manifest "$MAN_A" --signature "$SIG_A" \
        --trust-anchor "$ANCHOR" --seed-trust --adopt
    check "t11 --adopt completes install" test "$(link_target "$R2/current")" = 0.9.0
fi

# ------------------------------------------------------------------ TEST 12

begin "TEST 12 state preservation through A -> B -> A -> B"
if running; then
    ENV3=$(new_env third)
    R3="$ENV3/home/.local/share/wumbos/components/wumbosd"
    printf 'runtime state marker v1\n' > "$ENV3/home/.local/share/wumbos-runtime-marker"
    mkdir -p "$ENV3/home/.local/share/wumbos/other-component"
    printf 'sibling component state\n' \
        > "$ENV3/home/.local/share/wumbos/other-component/state"
    MARKER_BEFORE=$(sha256sum "$ENV3/home/.local/share/wumbos-runtime-marker" | awk '{print $1}')
    SIBLING_BEFORE=$(sha256sum "$ENV3/home/.local/share/wumbos/other-component/state" | awk '{print $1}')
    env_run_expect_ok "$ENV3" t12-install install "$ART_A" --manifest "$MAN_A" --signature "$SIG_A" --trust-anchor "$ANCHOR" --seed-trust
    env_run_expect_ok "$ENV3" t12-update-b update "$ART_B" --manifest "$MAN_B" --signature "$SIG_B"
    env_run_expect_ok "$ENV3" t12-rollback rollback
    env_run_expect_ok "$ENV3" t12-reupdate-b update "$ART_B" --manifest "$MAN_B" --signature "$SIG_B"
    MARKER_AFTER=$(sha256sum "$ENV3/home/.local/share/wumbos-runtime-marker" | awk '{print $1}')
    SIBLING_AFTER=$(sha256sum "$ENV3/home/.local/share/wumbos/other-component/state" | awk '{print $1}')
    check "t12 runtime marker byte-identical" test "$MARKER_BEFORE" = "$MARKER_AFTER"
    check "t12 sibling component state byte-identical" test "$SIBLING_BEFORE" = "$SIBLING_AFTER"
    check "t12 final current is B" test "$(link_target "$R3/current")" = 0.9.1
    check "t12 previous is A" test "$(link_target "$R3/previous")" = 0.9.0
    check "t12 A still materialized" test -d "$R3/releases/0.9.0"
    check "t12 owned unit hashes recorded" sh -c "grep -q 'owned-files' '$R3/state.json'"
fi

# ------------------------------------------------------------------ TEST 13

begin "TEST 13 offline rollback"
if running; then
    rm -rf "$ROOT/artifacts"
    check_not "t13 artifact cache deleted" test -e "$ROOT/artifacts/wumbosd-0.9.0-linux-x86_64.tar.gz"
    env_run_expect_ok "$ENV1" t13-rollback rollback
    check "t13 current -> A with no artifacts present" \
        test "$(link_target "$R1/current")" = 0.9.0
    check "t13 B retained" test -d "$R1/releases/0.9.1"
    check_not "t13 no artifact tree needed for rollback" test -e "$ROOT/artifacts"
fi

# ------------------------------------------------------------------ TEST 14

begin "TEST 14 status"
if running; then
    env_run "$ENV1" t14-status status
    check "t14 current version" sh -c "grep -q '^current: 0.9.0$' '$ROOT/logs/t14-status.log'"
    check "t14 previous version" sh -c "grep -q '^previous: 0.9.1$' '$ROOT/logs/t14-status.log'"
    check "t14 release identity" sh -c "grep -q '^release: 0.9.0 (v0.9.0' '$ROOT/logs/t14-status.log'"
    check "t14 signer fingerprint" sh -c "grep -q '^pinned trust: SHA256:' '$ROOT/logs/t14-status.log'"
    check "t14 socket unit path and state" \
        sh -c "grep -Eq '^wumbosd\.socket: .+/wumbosd\.socket\)$' '$ROOT/logs/t14-status.log'"
    check "t14 service unit path and state" \
        sh -c "grep -q '^wumbosd.service: ' '$ROOT/logs/t14-status.log'"
    check "t14 expected listener" sh -c "grep -q 'expected listener' '$ROOT/logs/t14-status.log'"
    check "t14 materialized releases" \
        sh -c "grep -q '^releases materialized: 0.9.0, 0.9.1$' '$ROOT/logs/t14-status.log'"
    env_run "$ENV1" t14-preflight preflight
    check "t14 preflight produces report" test -s "$ROOT/logs/t14-preflight.log"
    check "t14 preflight platform line" \
        sh -c "grep -q '^platform: Linux/x86_64$' '$ROOT/logs/t14-preflight.log'"
    check "t14 preflight has no failures" \
        sh -c "! grep -q '^FAIL:' '$ROOT/logs/t14-preflight.log'"
fi

# ------------------------------------------------------------------ summary

printf '\n== summary ==\n'
printf 'passed: %d\nfailed: %d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
