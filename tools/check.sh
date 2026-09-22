#!/bin/sh
# Shared local/CI release gate. Explicitly select the pinned toolchain even
# when a caller has RUSTUP_TOOLCHAIN set to a different version.
set -eu
cd "$(dirname "$0")/.."
toolchain=$(cat rust-toolchain)
cargo +"$toolchain" fmt --all -- --check
cargo +"$toolchain" clippy --workspace --locked --all-targets -- -D warnings
cargo +"$toolchain" test --workspace --locked --all-targets
cargo +"$toolchain" test -p chuda --no-default-features --locked --all-targets
cargo +"$toolchain" package --locked -p chuda
