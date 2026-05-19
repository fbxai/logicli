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
  list    List connected receivers and their paired devices
  pair    Discover and pair a new device
  unpair  Remove a device from a slot (1-6)
  debug   Dump raw register reads (development)
  help    Print help
```

### `logicli list`

Shows all connected Logitech receivers and their paired devices with names
and types:

```
$ logicli list
Receiver: Bolt Receiver  (VID 0x046D  PID 0xC548)
  Path: DevSrvsID:4299230602
  Product: USB Receiver
  Paired devices (2):
    Slot 1: MX KEYS B (keyboard [WPID B380])
    Slot 2: MX Master 3S (mouse [WPID B035])
```

### `logicli pair`

Discovers nearby devices in pairing mode and pairs the first one found.
For keyboards, displays a 6-digit passkey to type. For mice, displays a
left/right button sequence.

```sh
logicli pair                    # 30s discovery window
logicli pair --timeout 60       # longer window
logicli pair --short-auth       # 4 button presses for mouse (instead of 10)
logicli pair --path DevSrvsID:… # target a specific receiver
```

**Keyboard pairing** — type the displayed passkey and press Enter:

```
[4/5] Pairing with keyboard "MX KEYS B"...
      Pair command accepted. Waiting for authentication...

      ╔══════════════════════════════════════╗
      ║  PASSKEY: 753582                     ║
      ╚══════════════════════════════════════╝
      Type this passkey on the keyboard, then press ENTER.
```

**Mouse pairing** — press buttons in sequence, then both together:

```
[4/5] Pairing with mouse "MX Master 3S"...
      Pair command accepted. Waiting for authentication...

      ╔══════════════════════════════════════╗
      ║  BUTTON SEQUENCE:                    ║
      ╚══════════════════════════════════════╝
      Press buttons in this order:
         1. LEFT
         2. RIGHT
         3. LEFT
         4. RIGHT
      Then press LEFT + RIGHT simultaneously.
```

The `--short-auth` flag reduces mouse entropy from 10 bits (10 presses) to
4 bits (4 presses). Security tradeoff is minimal — an attacker would need to
be in BLE range during the few-second pairing window.

### `logicli unpair`

Removes the device in a given slot:

```sh
logicli unpair 1                    # remove device in slot 1
logicli unpair 2 --path DevSrvsID:… # target a specific receiver
```

## How pairing works

1. The CLI enables HID++ notifications on the receiver
2. Starts BLE discovery — the receiver scans for devices in pairing mode
3. Discovered devices are identified by name, type, and BT address
4. A pair command is sent with the chosen device's address and auth parameters
5. For keyboards: a 6-digit passkey is displayed (type it + Enter)
6. For mice: a binary button sequence is displayed (left/right clicks + both)
7. On success, the device is assigned to the next free slot

## Protocol notes

Communication uses HID++ 1.0 short/long reports. Key registers:

| Register | Type | Purpose |
|----------|------|---------|
| `0x02` | Short R | Connection state (device count) |
| `0xB5` | Long R (via short req) | Device names and pairing info |
| `0xC0` | Short W | BLE discovery (start/cancel) |
| `0xC1` | Long W | Pair/unpair commands |

**Important**: GET_LONG_REGISTER (0x83) requests must be sent as SHORT HID
reports (7 bytes) on Bolt receivers. The response comes back as a LONG report
(20 bytes). Sending 0x83 in a LONG report frame returns INVALID_SUB_ID.

See [PROGRESS.md](PROGRESS.md) for full protocol documentation.

## License

MIT
