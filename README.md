# logicli

A Rust CLI for managing Logitech USB wireless receivers (Bolt, Unifying, Nano,
Lightspeed) over HID++. List paired devices, pair new ones with interactive
discovery and authentication, and unpair by slot.

## Features

- **List** paired devices with names, types, and WPIDs
- **Pair** keyboards (6-digit passkey) and mice (button sequence) via BLE discovery
- **Unpair** devices by slot number
- Supports Bolt (0xC548) and Unifying (0xC52B) receivers
- `--short-auth` flag for faster mouse pairing (4 presses instead of 10)
- Auto-selects first discovered device after a 3-second grace period

## Requirements

- macOS or Linux
- Rust toolchain (`cargo`, edition 2024)
- `hidapi` system library
  - macOS: `brew install hidapi`
  - Debian/Ubuntu: `sudo apt install libhidapi-dev libudev-dev`

On **macOS**, the terminal running `logicli` must have **Input Monitoring**
permission (System Settings → Privacy & Security → Input Monitoring).

## Build

```sh
cargo build --release
# Binary at ./target/release/logicli

# Or install to PATH:
cargo install --path .
```

## Usage

```
logicli <COMMAND>

Commands:
  list     List connected receivers and their paired devices
  pair     Discover and pair a new device (keyboard or mouse)
  unpair   Unpair a device from a slot (1-6)
  version  Print version information
  debug    Dump raw register reads (development)
  help     Print help
```

### `logicli list`

Shows all connected Logitech receivers and their paired devices:

```
Logitech Bolt Receiver (USB PID C548) @ DevSrvsID:4299230602
  Slot 1: MX KEYS B (keyboard) — WPID B380
  Slot 2: MX Master 3S (mouse) — WPID B035
```

### `logicli pair`

Opens the receiver for pairing and discovers nearby devices:

```sh
# Pair any device (auto-detects keyboard vs mouse)
logicli pair

# With shorter auth for mice (4 presses instead of 10)
logicli pair --short-auth

# Specify a longer discovery timeout
logicli pair --timeout 45
```

The pairing flow:
1. Opens receiver for discovery
2. Shows devices as they appear (deduplicated by BT address)
3. Auto-selects the first device after 3 seconds, or immediately if only one found
4. For keyboards: displays a 6-digit passkey to type on the device + Enter
5. For mice: prompts you to press a button sequence (10 presses, or 4 with `--short-auth`)
6. Confirms pairing success with slot assignment

### `logicli unpair <slot>`

```sh
# Remove device from slot 2
logicli unpair 2
```

### `logicli version`

```
logicli v0.2.0
  commit: abc1234
  built:  2025-01-15T10:30:00Z
```

## Architecture

```
src/
├── main.rs              — CLI struct (clap), dispatch, version
└── backend/
    ├── mod.rs           — module declarations
    └── logitech.rs      — HID++ protocol: constants, register I/O,
                           list/pair/unpair/debug commands, unit tests
```

The Logitech backend implements the HID++ 1.0 register protocol:
- Short reports (7 bytes, report ID 0x10) for register requests
- Long reports (20 bytes, report ID 0x11) for responses
- Device discovery via SET_REGISTER 0xB2 + notification polling
- Authentication via passkey exchange (keyboards) or entropy-based button sequence (mice)

## CI

- **ci.yml**: Runs on push/PR — `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
- **release.yml**: Triggered by `v*` tags — cross-compiles for macOS (ARM64 + x86_64) and Linux (x86_64)

## License

MIT
