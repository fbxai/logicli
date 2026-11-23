# Contributor guide

These instructions apply to agents and developers working anywhere in this repository. Follow any more specific instructions in nested `AGENTS.md` files as well.

## Project layout

`logicli` is a Rust 2024 CLI for managing Logitech USB receivers over HID++. It uses `clap` for arguments, `hidapi` for device access, and `anyhow` for errors.

| Path | Purpose |
| --- | --- |
| `src/main.rs` | CLI definitions, receiver discovery, HID++ I/O, register helpers, and list/pair/unpair/debug commands |
| `Cargo.toml`, `Cargo.lock` | Binary definition and pinned dependency resolution |
| `Justfile` | Local build, test, lint, and device-operation recipes |
| `README.md` | Setup, CLI examples, and protocol notes |
| `.github/workflows/ci.yml` | Formatting, lint, release build, and tests on macOS and Linux |
| `.github/workflows/release.yml` | Builds and publishes binaries for version tags |
| `graft/` | Regenerable local Trail-Code index; do not commit generated graph data |
| `target/` | Generated Cargo build output |

## Setup and commands

Use stable Rust with `rustfmt` and `clippy`. The project supports macOS and Linux. Install the system dependencies with `brew install hidapi` on macOS or `sudo apt install libhidapi-dev libudev-dev` on Debian/Ubuntu. macOS device access requires Input Monitoring permission for the terminal running the CLI. `just` is optional; all recipes have Cargo equivalents.

| Task | Command |
| --- | --- |
| Show recipes | `just --list` |
| Type-check | `cargo check` or `just check` |
| Debug build | `cargo build` or `just build` |
| Release build | `cargo build --release` or `just release` |
| Format | `cargo fmt --all` or `just fmt` |
| Check formatting | `cargo fmt --all -- --check` or `just fmt-check` |
| Lint all targets | `cargo clippy --all-targets -- -D warnings` or `just lint` |
| Run tests | `cargo test` or `just test` |
| Run a specific test | `cargo test <test_name>` |
| Local verification suite | `just ci` (format check, lint, tests) |
| Show CLI help | `cargo run -- --help` |
| List receivers | `cargo run -- list` |
| Install locally | `cargo install --path .` or `just install` |

For agents in environments requiring RTK, prefix shell commands with `rtk`, for example `rtk cargo test`.

## Before changing code

- Read the relevant source, callers, README, Justfile, and workflow definitions. Identify affected CLI arguments, protocol messages, output, tests, and documentation before editing.
- When the local `graft/` index and Trail-Code are available, start structural discovery with `trail map` or `trail ask "<task>" --source`; trace callers with `trail callers <symbol>`. Verify findings against source. Fall back to `rg` when the graph or tool is unavailable.
- Keep changes focused on the requested behavior and preserve unrelated work in the checkout. Avoid unrelated formatting, dependency upgrades, or restructuring.

## Coding conventions

- Follow `rustfmt`, Rust naming conventions, and the surrounding organization: `snake_case` functions and variables, `PascalCase` types, and `SCREAMING_SNAKE_CASE` constants.
- Keep the current single-binary structure unless the change needs a module. Reuse existing helpers and prefer the standard library or installed dependencies over new dependencies and abstractions.
- Use `anyhow::Result`, add useful context to I/O errors, and use `bail!` for actionable failures. Do not introduce production `unwrap()`/`expect()` calls on user input or device data, or silently substitute valid-looking values for malformed responses.
- Name protocol constants and use explicit byte widths. Check report lengths before indexing or slicing, validate values at the CLI and HID boundaries, and keep waits bounded.
- Keep Bolt and Unifying behavior distinct where the protocols differ. In particular, Bolt GET_LONG_REGISTER (`0x83`) uses a short 7-byte request and receives a long 20-byte response; do not normalize report sizes without protocol evidence.
- Preserve receiver notification flags and clean up temporary discovery/pairing state on success, failure, and timeout when changing those flows. Do not discard asynchronous notifications needed by the operation.
- Treat authentication changes as protocol changes: verify supported entropy and passkey encoding rather than assuming every device accepts them. Do not log passkeys outside the required interactive prompt.
- Use comments for non-obvious protocol constraints and decisions. Update CLI help and README examples when user-visible behavior changes. Keep Markdown prose paragraphs on one source line.

## Tests are required for implementation changes

- Always add or update automated tests when implementing a feature, fixing a bug, or refactoring behavior. A passing build or manual device check does not replace tests. Documentation-only edits do not need code tests.
- Every bug fix must include a regression test that fails before the fix. Cover normal behavior, relevant boundaries, malformed or truncated reports, and error paths for the changed logic.
- There are currently no test modules or integration tests. Add unit tests beside the code using `#[cfg(test)] mod tests`; use `tests/` for binary-level integration tests when appropriate.
- Prefer small, pure parsing, request-encoding, and decision helpers that can be tested without a receiver. Extract only what the change needs; use Rust's built-in test framework and avoid a new testing dependency unless necessary.
- Test observable results and protocol bytes, not a duplicate of the implementation. Normal automated tests must be deterministic and must not require USB hardware, pairing actions, network access, or timing-sensitive sleeps.
- Keep hardware-dependent checks explicitly opt-in and document the device/receiver model, platform, command, and observed result. Never make normal `cargo test` alter receiver pairings.

## Verification and handoff

Before submitting an implementation change, run `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, and `cargo test --release`. `just ci` is a useful local check but does not include the release build or release-mode tests used by CI. Ensure the existing macOS/Linux CI matrix passes; report platform coverage you could not verify locally.

For device-facing changes, supplement automated tests with a check on the affected hardware when available. Pairing and unpairing change receiver state; only run them on an explicitly selected test setup with authorization. The `debug` command also writes receiver registers, so do not treat it as a read-only smoke test. Use `--path` where supported to select the intended receiver.

Review the final diff for unrelated edits, accidental generated files, and missing tests. Report what changed, which checks actually ran, their results, and any remaining hardware or platform limitations. Do not claim hardware compatibility from unit tests alone, and do not suppress failing checks to complete a change.
