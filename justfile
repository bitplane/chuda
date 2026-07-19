# Chuda build and release recipes. Cargo remains the build system; these are
# short, memorable entry points for the local workflow.

# Run the same checks as CI before cutting a release.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --locked --all-targets -- -D warnings
    cargo test --workspace --locked --all-targets
    cargo package --locked -p chuda

# Build and install the CPU Python extension into the active virtualenv.
python-dev:
    maturin develop --release

# Compile both backends. This does not require a working GPU.
check-cuda:
    cargo check --workspace --all-targets --features cuda

# Run ignored CPU/CUDA parity tests on a machine with a working NVIDIA GPU.
test-gpu:
    cargo test --workspace --features cuda -- --ignored

# Bump the version (patch/minor/major or X.Y.Z), commit, tag vX.Y.Z and push.
# The pushed tag triggers the release workflow, which publishes the Rust crate,
# Python package, and GitHub release after its checks pass.
# Cut and push a versioned release; defaults to a patch bump.
release level="patch":
    cargo release {{level}} --execute --no-confirm
