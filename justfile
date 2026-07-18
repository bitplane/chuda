# Chuda build and release recipes. Cargo remains the build system; these are
# short, memorable entry points for the local workflow.

# Run the same checks as CI before cutting a release.
check:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked --all-targets
    cargo package --locked

# Bump the version (patch/minor/major or X.Y.Z), commit, tag vX.Y.Z and push.
# The pushed tag triggers .github/workflows/release-check.yml, which publishes
# the crate to crates.io after its own checks pass.
# Cut and push a versioned release; defaults to a patch bump.
release level="patch":
    cargo release {{level}} --execute --no-confirm
