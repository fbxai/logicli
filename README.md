# logicli

A small Rust CLI for inspecting and managing Logitech USB receivers
(Unifying, Nano, Lightspeed, Bolt) over HID++.

It can:

- enumerate connected Logitech receivers and, where supported, the
  peripherals paired to them
- open a receiver's pairing window so a new peripheral can be paired
- remove an existing pairing by slot

## Requirements

- macOS or Linux
- Rust toolchain (`cargo`, tested with 1.90)
- `hidapi` system library
  - macOS: `brew install hidapi`
  - Debian/Ubuntu: `sudo apt install libhidapi-dev`
- [`just`](https://github.com/casey/just) (optional, for the recipes
  below)

On **macOS**, opening the receiver for `pair` / `unpair` requires the
terminal you launch `logicli` from to be granted **Input Monitoring**
under *System Settings → Privacy & Security*.

## Build / install

Using `just`:

```sh
just build         # debug build at ./target/debug/logicli
just release       # optimised build at ./target/release/logicli
just install       # cargo install --path .  (drops it in ~/.cargo/bin)
```

Or plain cargo:

```sh
cargo build --release
cargo install --path .
```

## Usage

```text
logicli <COMMAND>

Commands:
  list    List connected Logitech receivers (and any paired devices it reports)
  pair    Open the pairing lock on the receiver so a peripheral can be paired
  unpair  Unpair the peripheral in a slot (1..=6)
  help    Print this message or the help of the given subcommand(s)
```

### `logicli list`

Enumerates every Logitech receiver visible to the OS HID stack and prints
its identification plus, where supported, its paired slots.

```sh
$ logicli list
Receiver: Bolt Receiver  (VID 0x046D  PID 0xC548)
  Path        : DevSrvsID:4299209781
  Manufacturer: Logitech
  Product     : USB Receiver
  Release     : 5.03
  Paired slots: (none reported)
```

The `Path` field is the HID path the OS assigns to the receiver. Pass it
to `--path` on `pair` / `unpair` when you have more than one receiver
plugged in.

> Note: paired-slot enumeration uses the legacy HID++ 1.0 register
> `0xB5`. That register is not present on the Bolt receiver, which
> requires HID++ 2.0 feature discovery; `list` reports `(none reported)`
> for Bolt.

### `logicli pair`

Opens the receiver's pairing window so a peripheral can be paired during
the window.

```sh
logicli pair                       # 30-second window on the first receiver
logicli pair --timeout 60          # 60-second window
logicli pair --path DevSrvsID:…    # target a specific receiver
```

Power-cycle the peripheral (off/on, or hold its connect button) while
the window is open. The receiver will assign it to the next free slot.

Equivalent via `just`:

```sh
just pair            # 30 s
just pair 60         # 60 s
```

### `logicli unpair`

Removes the pairing in slot `<SLOT>` (`1..=6`).

```sh
logicli unpair 1                   # drop the device in slot 1
logicli unpair 2 --path DevSrvsID:…
```

Via `just`:

```sh
just unpair 1
```

## Development

```sh
just check        # cargo check
just test         # cargo test
just fmt          # cargo fmt --all
just lint         # cargo clippy --all-targets -- -D warnings
just ci           # fmt-check + lint + test
just run -- list  # cargo run with args
just clean
```

## Protocol notes

`logicli` talks to the receiver over HID++ 1.0 short reports
(`0x10 FF <sub-id> <register> <p0> <p1> <p2>`):

| Action | sub-id | register (Bolt) | register (Unifying) | params           |
| ------ | ------ | --------------- | ------------------- | ---------------- |
| Open pairing  | `0x80` (set) | `0xC0` | `0xB2` | `01 <timeout> 00` |
| Unpair slot   | `0x80` (set) | `0xC0` | `0xB2` | `03 <slot> 00`    |
| List slot N   | `0x83` (get long) | n/a    | `0xB5` | `2N 00 00`     |

Register choice is keyed on USB PID: `0xC548` (Bolt) uses `0xC0`;
everything else falls back to `0xB2`.

## Caveats

- Bolt paired-device listing is not implemented (would require HID++ 2.0
  feature traversal).
- Pair/unpair has not been round-tripped against a live peripheral —
  command structure has been validated, but timing and edge-case
  behaviour on every receiver firmware revision has not.
- `pair` / `unpair` mutate receiver state; double-check the receiver
  with `list` before running them.
