#!/usr/bin/env bash
# Build portable akron release binaries into dist/.
#
# Produces, for each supported target:
#   dist/akron-<version>-<target>          the binary
#   dist/akron-<version>-<target>.sha256   its checksum
#
# One-time setup on an arm64 macOS host:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-musl
#   brew install FiloSottile/musl-cross/musl-cross   # provides x86_64-linux-musl-gcc,
#                                                     # needed because tree-sitter/gix
#                                                     # pull in C deps that must be
#                                                     # cross-compiled for the target too.
#
# Usage:
#   ./scripts/release.sh
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(grep -m1 '^version' Cargo.toml | sed -E 's/version = "(.*)"/\1/')
mkdir -p dist

build_target() {
  local target="$1"
  shift
  echo "==> building $target"
  cargo build --release --target "$target" "$@"
  local out="dist/akron-${version}-${target}"
  cp "target/${target}/release/akron" "$out"
  chmod +x "$out"
  (cd dist && shasum -a 256 "$(basename "$out")" > "$(basename "$out").sha256")
  echo "==> wrote $out"
}

# Native target: full build, `semantic` feature (`akron find`) included.
build_target "aarch64-apple-darwin"

# Same-OS cross target: no extra toolchain needed beyond rustup, but TKI-41's
# `semantic` feature cross-compiles the ONNX Runtime linkage via `ort`, which
# only ships prebuilt binaries for a target matching the *build host* — from
# this arm64 host, `cargo build --target x86_64-apple-darwin` fails with
# "ort does not provide prebuilt binaries for the target x86_64-apple-darwin"
# even though the native aarch64 build above succeeds. Same shape as the musl
# problem below: ship this target with `find` built out rather than block
# the release (re-enable once building natively on x86_64, or once `ort`
# ships that slice).
build_target "x86_64-apple-darwin" --no-default-features

# musl target for portable Linux binaries. Requires the musl-cross C toolchain
# above (tree-sitter/gix have C build-script deps) — skip with a clear message
# if it isn't installed rather than failing the whole release.
if command -v x86_64-linux-musl-gcc >/dev/null 2>&1; then
  export CC_x86_64_unknown_linux_musl=x86_64-linux-musl-gcc
  export CXX_x86_64_unknown_linux_musl=x86_64-linux-musl-g++
  export AR_x86_64_unknown_linux_musl=x86_64-linux-musl-ar
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc
  # TKI-41: the `semantic` feature (`akron find`) pulls fastembed -> hf-hub ->
  # reqwest -> native-tls -> openssl-sys, which cannot cross-compile to musl
  # without a target OpenSSL sysroot this script doesn't provide — so this
  # target ships with `find` built out entirely (`akron find` prints the
  # no-semantic-feature message on this binary) rather than blocking the release.
  build_target "x86_64-unknown-linux-musl" --no-default-features
else
  echo "==> skipping x86_64-unknown-linux-musl: x86_64-linux-musl-gcc not on PATH"
  echo "    install with: brew install FiloSottile/musl-cross/musl-cross"
fi

echo "==> done"
ls -lh dist/
