bin := "./target/debug/logicli"
release_bin := "./target/release/logicli"

# List available recipes.
default:
    @just --list

# Type-check without producing a binary.
check:
    cargo check

# Debug build.
build:
    cargo build

# Optimised build.
release:
    cargo build --release

# Install the binary to the system - by default this is ~/.cargo/bin
install:
    cargo install --path .

# Run the test suite.
test:
    cargo test

# Format the source tree.
fmt:
    cargo fmt --all

# Verify formatting without rewriting (CI-style).
fmt-check:
    cargo fmt --all -- --check

# Lint with clippy, denying warnings.
lint:
    cargo clippy --all-targets -- -D warnings

# fmt-check + lint + test. Run before committing.
ci: fmt-check lint test

# Run the CLI with arbitrary args, e.g. `just run -- list`.
run *ARGS:
    cargo run -- {{ARGS}}

# Build (if needed) then list connected receivers.
[group('dev release cli')]
list: build
    {{bin}} list

# Open the pairing lock. Optional TIMEOUT in seconds (default 30).
[group('dev release cli')]
pair TIMEOUT="30": build
    {{bin}} pair --timeout {{TIMEOUT}}

# Unpair a slot (1..=6).
[group('dev release cli')]
unpair SLOT: build
    {{bin}} unpair {{SLOT}}

# Run the release binary against `list` (smoke test the optimised build).
[group('dev release cli')]
list-release: release
    {{release_bin}} list

# Remove all build artefacts.
clean:
    cargo clean
