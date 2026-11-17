#!/usr/bin/env bash
# WASM readiness gate (Phase 0).
#
# Checks that the wasm foundation crates compile for `wasm32-unknown-unknown`
# and fails when reality diverges from `EXPECTED_RED` below.
#
# Why an expected-failure list instead of a plain check: the fork is migrating
# crate-by-crate (docs/docs/WASM/implementation-plan.md, chunk 0). While Phase 0
# is in progress some crates are known-red; the gate keeps the suite green but
# still catches (a) a green crate regressing, and (b) a red crate turning green
# without its entry being removed here.
#
# Usage: scripts/check-wasm.bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
RESET='\033[0m'

pass() { echo -e "${GREEN}✓${RESET} $1"; }
fail() { echo -e "${RED}✗${RESET} $1"; }
note() { echo -e "  ${BOLD}·${RESET} $1"; }
step() { echo -e "\n${BOLD}▸ $1${RESET}"; }

TARGET='wasm32-unknown-unknown'

# Crates that must compile for wasm32-unknown-unknown.
CRATES=(
    infinity-protocol
    rap-protocol
    infinity-provider-protocol
    rap-client
    infinity-agent-core
    sandbox-core
    infinity-mcp-bridge
)

# Crates still expected to FAIL. Remove entries as Phase 0 chunks land; the gate
# fails when a crate here starts passing, so the list cannot silently rot.
# Empty since chunk 0.7: the whole foundation set compiles for wasm.
EXPECTED_RED=()

is_expected_red() {
    local crate=$1
    local entry
    for entry in ${EXPECTED_RED[@]+"${EXPECTED_RED[@]}"}; do
        [ "$entry" = "$crate" ] && return 0
    done
    return 1
}

step "WASM readiness ($TARGET)"

if ! rustup target list --installed | grep -qx "$TARGET"; then
    fail "target $TARGET is not installed (run: rustup target add $TARGET)"
    exit 1
fi

status=0
unexpected_green=()
unexpected_red=()

for crate in "${CRATES[@]}"; do
    # Crates with a default-on `native` feature (HTTP server, git/jj plumbing,
    # process handles, stdio MCP transport): a browser build is their
    # no-default-features configuration. See docs/docs/WASM/README.md.
    case "$crate" in
        sandbox-core | infinity-mcp-bridge) extra=(--no-default-features) ;;
        *) extra=() ;;
    esac

    if cargo check --quiet --target "$TARGET" -p "$crate" "${extra[@]}" > /dev/null 2>&1; then
        if is_expected_red "$crate"; then
            unexpected_green+=("$crate")
        else
            pass "$crate"
        fi
    else
        if is_expected_red "$crate"; then
            note "$crate (expected red — see implementation-plan.md chunk 0)"
        else
            unexpected_red+=("$crate")
        fi
    fi
done

if [ ${#unexpected_green[@]} -gt 0 ]; then
    echo
    fail "unexpectedly GREEN: ${unexpected_green[*]}"
    note "Remove them from EXPECTED_RED in scripts/check-wasm.bash."
    status=1
fi

if [ ${#unexpected_red[@]} -gt 0 ]; then
    echo
    fail "unexpectedly RED: ${unexpected_red[*]}"
    note "These crates must compile for $TARGET; run 'cargo check --target $TARGET -p <crate>'." 
    status=1
fi

if [ "$status" -eq 0 ]; then
    echo
    pass "WASM readiness gate matches the expected Phase 0 state"
fi

exit "$status"
