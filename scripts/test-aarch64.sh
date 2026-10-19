#!/usr/bin/env bash
# Runs `cargo test` for aarch64 (NEON backend) on an x86_64 Linux host, under user-mode QEMU.
#
# Usage: scripts/test-aarch64.sh [cargo test arguments...]
#   e.g. scripts/test-aarch64.sh --lib simd
#        scripts/test-aarch64.sh --release --lib simd -- --ignored neon_timing --nocapture
#
# Nothing is installed system wide and no binfmt handler is registered. The tools are downloaded
# once (pinned versions, SHA-256 verified) into the cache directory:
#   * qemu-aarch64 (static) from Debian's qemu-user package, run as the cargo target runner;
#   * Zig, used as the aarch64 C compiler (`zig cc`) and archiver for build scripts of
#     dev-dependencies that compile C code (criterion -> alloca).
# Linking uses rust-lld (shipped with rustup), the target is `aarch64-unknown-linux-musl`
# (static binaries, no aarch64 sysroot needed).
#
# Environment:
#   ROARING128_QEMU_CACHE    cache directory (default: $HOME/.cache/roaring128/qemu)
#   ROARING128_QEMU          use this qemu-aarch64 binary instead of downloading one
#                            (default: `qemu-aarch64-static` / `qemu-aarch64` from PATH if present)
#   ROARING128_QEMU_CPU      emulated CPU model passed as QEMU_CPU (default: max). Use for example
#                            `cortex-a53` to check that only ARMv8.0 baseline instructions are used.
#   CC_aarch64_unknown_linux_musl / AR_aarch64_unknown_linux_musl
#                            use an existing aarch64 C toolchain instead of downloading Zig
#   ROARING128_EXPECT_BACKEND
#                            backend the tests assert to be selected (default: neon)
#
# On an aarch64 host (Linux or macOS) this simply runs `cargo test` natively for the host target.

set -euo pipefail

TARGET=aarch64-unknown-linux-musl
TARGET_ENV=AARCH64_UNKNOWN_LINUX_MUSL
CACHE_DIR="${ROARING128_QEMU_CACHE:-$HOME/.cache/roaring128/qemu}"

QEMU_VERSION=10.0.13+ds-0+deb13u1
QEMU_DEB="qemu-user_${QEMU_VERSION}_amd64.deb"
QEMU_SHA256=ca6ede739327a20ae5a3498230c8a3a9a072c586e131bc6bcc1201eb1725e336
QEMU_SHA1=dec1373851d29f91642c197f4a339d0d2ad25ed0
QEMU_URLS=(
    "https://deb.debian.org/debian/pool/main/q/qemu/${QEMU_DEB}"
    # Permanent copy, in case the package was superseded in the Debian pool.
    "https://snapshot.debian.org/file/${QEMU_SHA1}"
)

ZIG_VERSION=0.16.0
ZIG_DIR="zig-x86_64-linux-${ZIG_VERSION}"
ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ZIG_URLS=("https://ziglang.org/download/${ZIG_VERSION}/${ZIG_DIR}.tar.xz")

log() { printf 'test-aarch64: %s\n' "$*" >&2; }
die() {
    log "error: $*"
    exit 1
}

# download <dest> <sha256> <url>...: fetches the first URL that works, verifies the checksum.
download() {
    local dest=$1 sha=$2
    shift 2
    local tmp="${dest}.part.$$"
    local url
    for url in "$@"; do
        log "downloading ${url}"
        if curl -fL --retry 3 --connect-timeout 20 -o "$tmp" "$url"; then
            if echo "${sha}  ${tmp}" | sha256sum -c --status; then
                mv -f "$tmp" "$dest"
                return 0
            fi
            log "checksum mismatch for ${url}"
        fi
        rm -f "$tmp"
    done
    die "could not download $(basename "$dest")"
}

ensure_qemu() {
    if [[ -n "${ROARING128_QEMU:-}" ]]; then
        [[ -x "$ROARING128_QEMU" ]] || die "ROARING128_QEMU=${ROARING128_QEMU} is not executable"
        QEMU=$ROARING128_QEMU
        return
    fi
    local candidate
    for candidate in qemu-aarch64-static qemu-aarch64; do
        if QEMU=$(command -v "$candidate"); then
            return
        fi
    done
    local dir="${CACHE_DIR}/qemu-${QEMU_VERSION}"
    QEMU="${dir}/qemu-aarch64"
    [[ -x "$QEMU" ]] && return
    mkdir -p "$dir"
    local deb="${dir}/${QEMU_DEB}"
    [[ -f "$deb" ]] || download "$deb" "$QEMU_SHA256" "${QEMU_URLS[@]}"
    local tmp
    tmp=$(mktemp -d "${dir}/extract.XXXXXX")
    if command -v dpkg-deb >/dev/null; then
        dpkg-deb --fsys-tarfile "$deb" | tar -x -C "$tmp" ./usr/bin/qemu-aarch64
    else
        command -v ar >/dev/null || die "need dpkg-deb or ar (binutils) to unpack ${QEMU_DEB}"
        (cd "$tmp" && ar x "$deb" data.tar.xz && tar -xJf data.tar.xz ./usr/bin/qemu-aarch64)
    fi
    mv -f "${tmp}/usr/bin/qemu-aarch64" "$QEMU"
    rm -rf "$tmp" "$deb"
    "$QEMU" --version | head -n 1 >&2
}

ensure_c_toolchain() {
    local cc_var=CC_aarch64_unknown_linux_musl ar_var=AR_aarch64_unknown_linux_musl
    if [[ -n "${!cc_var:-}" ]]; then
        return
    fi
    if command -v aarch64-linux-musl-gcc >/dev/null; then
        export "${cc_var}=aarch64-linux-musl-gcc"
        command -v aarch64-linux-musl-ar >/dev/null && export "${ar_var}=aarch64-linux-musl-ar"
        return
    fi
    local dir="${CACHE_DIR}/${ZIG_DIR}"
    local zig="${dir}/zig"
    if [[ ! -x "$zig" ]]; then
        mkdir -p "$CACHE_DIR"
        local tarball="${CACHE_DIR}/${ZIG_DIR}.tar.xz"
        [[ -f "$tarball" ]] || download "$tarball" "$ZIG_SHA256" "${ZIG_URLS[@]}"
        local tmp
        tmp=$(mktemp -d "${CACHE_DIR}/extract.XXXXXX")
        tar -xJf "$tarball" -C "$tmp"
        rm -rf "$dir"
        mv "${tmp}/${ZIG_DIR}" "$dir"
        rm -rf "$tmp" "$tarball"
    fi
    # cc-rs expects plain executables; the wrappers pin zig's target triple (and drop the
    # `--target=` flag cc-rs adds for clang-like compilers, whose spelling zig rejects).
    local bin="${CACHE_DIR}/bin-${ZIG_VERSION}"
    mkdir -p "$bin"
    cat >"${bin}/aarch64-cc.tmp" <<EOF
#!/bin/sh
for arg do
    shift
    case "\$arg" in --target=*) ;; *) set -- "\$@" "\$arg" ;; esac
done
exec "${zig}" cc -target aarch64-linux-musl -fno-sanitize=all "\$@"
EOF
    cat >"${bin}/aarch64-ar.tmp" <<EOF
#!/bin/sh
exec "${zig}" ar "\$@"
EOF
    chmod +x "${bin}/aarch64-cc.tmp" "${bin}/aarch64-ar.tmp"
    mv -f "${bin}/aarch64-cc.tmp" "${bin}/aarch64-cc"
    mv -f "${bin}/aarch64-ar.tmp" "${bin}/aarch64-ar"
    export "${cc_var}=${bin}/aarch64-cc" "${ar_var}=${bin}/aarch64-ar"
    export ZIG_GLOBAL_CACHE_DIR="${ZIG_GLOBAL_CACHE_DIR:-${CACHE_DIR}/zig-cache}"
    export ZIG_LOCAL_CACHE_DIR="${ZIG_LOCAL_CACHE_DIR:-${CACHE_DIR}/zig-cache}"
}

command -v cargo >/dev/null || die "cargo not found"
export ROARING128_EXPECT_BACKEND="${ROARING128_EXPECT_BACKEND-neon}"

case "$(uname -m)" in
    aarch64 | arm64)
        log "native aarch64 host, running the tests without emulation"
        exec cargo test "$@"
        ;;
    x86_64 | amd64) ;;
    *) die "unsupported host architecture $(uname -m) (need x86_64 or aarch64)" ;;
esac
[[ "$(uname -s)" == Linux ]] || die "emulated runs need a Linux host (user-mode QEMU)"

if command -v rustup >/dev/null && ! rustup target list --installed | grep -qx "$TARGET"; then
    log "installing the ${TARGET} Rust target (user local)"
    rustup target add "$TARGET"
fi

ensure_qemu
ensure_c_toolchain
linker_var="CARGO_TARGET_${TARGET_ENV}_LINKER"
runner_var="CARGO_TARGET_${TARGET_ENV}_RUNNER"
export "${linker_var}=${!linker_var:-rust-lld}"
export "${runner_var}=${!runner_var:-$QEMU}"
export QEMU_CPU="${ROARING128_QEMU_CPU:-${QEMU_CPU:-max}}"
log "runner: ${!runner_var} (QEMU_CPU=${QEMU_CPU})"

exec cargo test --target "$TARGET" "$@"
