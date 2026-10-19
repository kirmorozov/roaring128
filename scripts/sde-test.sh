#!/usr/bin/env bash
# Runs the test suite under the Intel Software Development Emulator (SDE) with AVX-512 CPUs
# emulated, so that the AVX-512 backend is exercised on machines without AVX-512.
#
# Usage:
#   scripts/sde-test.sh [cargo test arguments...]     (default: --lib)
#
# Examples:
#   scripts/sde-test.sh                               # library tests on Ice Lake and Sapphire Rapids
#   scripts/sde-test.sh --lib simd                    # only the SIMD kernel tests
#   SDE_CPUS=icx scripts/sde-test.sh --release --lib  # one CPU, optimized build
#
# Environment:
#   SDE_PATH               an existing `sde64` binary (skips the download)
#   ROARING128_SDE_CACHE   download and extraction directory
#                          (default: ${XDG_CACHE_HOME:-$HOME/.cache}/roaring128/sde)
#   SDE_CPUS               emulated CPUs, `sde64 -help` lists them (default: "icx spr")
#   SDE_URL, SDE_SHA256    override the pinned SDE release (URL of a *-lin.tar.xz kit and the
#                          SHA-256 Intel publishes for it on the download page)
#
# Every run sets ROARING128_EXPECT_BACKEND=avx512, so the tests fail if the emulated CPU does
# not select the AVX-512 backend. By downloading SDE you accept Intel's SDE license.

set -euo pipefail

SDE_VERSION="10.13.1-2026-07-28"
SDE_URL="${SDE_URL:-https://downloadmirror.intel.com/924984/sde-external-${SDE_VERSION}-lin.tar.xz}"
# Published on https://www.intel.com/content/www/us/en/download/684897/ for the kit above.
SDE_SHA256="${SDE_SHA256:-94e97d623fec54385686e1e7ba65ebc9941748c05ee451423948334892bf2b50}"
SDE_CPUS="${SDE_CPUS:-icx spr}"
CACHE="${ROARING128_SDE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/roaring128/sde}"

log() { printf 'sde-test: %s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *) die "Intel SDE for Linux needs an x86_64 Linux host" ;;
esac

find_sde() {
    if [[ -n "${SDE_PATH:-}" ]]; then
        [[ -x "$SDE_PATH" ]] || die "SDE_PATH=$SDE_PATH is not an executable"
        printf '%s\n' "$SDE_PATH"
        return
    fi
    local kit
    kit="$CACHE/$(basename "$SDE_URL" .tar.xz)"
    if [[ ! -x "$kit/sde64" ]]; then
        command -v curl >/dev/null || die "curl is required to download SDE"
        command -v sha256sum >/dev/null || die "sha256sum is required to verify SDE"
        mkdir -p "$CACHE"
        local tarball="$CACHE/$(basename "$SDE_URL")"
        if [[ ! -f "$tarball" ]] || ! echo "$SDE_SHA256  $tarball" | sha256sum -c --status; then
            log "downloading $SDE_URL"
            curl -fL --retry 3 --proto '=https' -o "$tarball.part" "$SDE_URL"
            mv "$tarball.part" "$tarball"
        fi
        echo "$SDE_SHA256  $tarball" | sha256sum -c --status \
            || die "checksum mismatch for $tarball (expected $SDE_SHA256); delete it to retry"
        log "extracting into $CACHE"
        local tmp
        tmp="$(mktemp -d "$CACHE/extract.XXXXXX")"
        tar -xJf "$tarball" -C "$tmp"
        local extracted
        extracted="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
        [[ -x "$extracted/sde64" ]] || die "no sde64 in $tarball"
        rm -rf "$kit"
        mv "$extracted" "$kit"
        rmdir "$tmp"
    fi
    printf '%s\n' "$kit/sde64"
}

SDE="$(find_sde)"
log "using $SDE"

args=("$@")
if [[ ${#args[@]} -eq 0 ]]; then
    args=(--lib)
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."
for cpu in $SDE_CPUS; do
    log "cargo test ${args[*]} on emulated '$cpu'"
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="$SDE -$cpu --" \
        ROARING128_EXPECT_BACKEND=avx512 \
        cargo test "${args[@]}"
done
log "all emulated runs passed ($SDE_CPUS)"
