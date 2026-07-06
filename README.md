# Melnor ESP

Makes an ESP32 present itself as a Melnor Bluetooth water timer, so any Melnor
BLE client — the **Melnor phone app**, Home Assistant's `melnor` integration, or
anything else speaking the protocol — discovers it, connects, and drives it.

Two ways to use it:

- **Emulator** (no GPIO): a virtual timer for testing Melnor clients without
  owning hardware.
- **Controller**: wire valve outputs to GPIO pins and it becomes a real
  sprinkler controller you drive from the Melnor app. See
  [Controller mode](#controller-mode-driving-real-valves).

Written in Rust on `esp-idf` + [`esp32-nimble`].

## Why an ESP, not a Mac

A Melnor client matches on exactly one thing: a BLE advertisement with
**manufacturer id 13 (0x000D)** whose payload starts with **0x59**. macOS
CoreBluetooth refuses to set manufacturer data in advertisements, so a Mac can
never be discovered as a Melnor. An ESP can.

## Supported chips

Any BLE-capable ESP. Pass the chip name to `flash.sh`:

| Chip      | Target triple            |
|-----------|--------------------------|
| `esp32`   | `xtensa-esp32-espidf`    |
| `esp32s3` | `xtensa-esp32s3-espidf`  |
| `esp32c2` | `riscv32imc-esp-espidf`  |
| `esp32c3` | `riscv32imc-esp-espidf`  |
| `esp32c6` | `riscv32imac-esp-espidf` |
| `esp32h2` | `riscv32imac-esp-espidf` |

`esp32s2` is intentionally unsupported — it has no BLE radio.

## Prerequisites (one time)

```sh
cargo install espup ldproxy espflash
espup install          # installs the `esp` Rust toolchain + ESP-IDF deps
. $HOME/export-esp.sh  # sets up the environment (re-source per shell)
```

## Build + flash

```sh
./flash.sh              # esp32s3 (default)
./flash.sh esp32c3      # any chip from the table
./flash.sh esp32 -- --port /dev/tty.usbserial-XXXX
```

`flash.sh` only picks the **chip** (which sets the Rust target/toolchain); it
runs `cargo run --release`, building, flashing, and opening the serial monitor.
Everything about the device's *behavior* lives in [`config.yaml`](#configuration).
For a build only:

```sh
MCU=esp32c6 CARGO_BUILD_TARGET=riscv32imac-esp-espidf cargo build --release
```

## Configuration

Device behavior is defined in [`config.yaml`](config.yaml) and baked into the
firmware at build time — edit it and re-run `flash.sh` to apply. Keeping it in
one file (rather than flash-time flags) gives future features — analog battery
sensing, RS-232/485 — a place to declare their pins and parameters.

```yaml
device_name: Melnor ESP   # exposed on 0xEC01 (device user name)
valves: 4                 # 1, 2, or 4

drive:
  mode: ""                # "ac" | "latching" | "" to infer from pins
  pulse_ms: 80            # latching pulse width (< 100 ms)
  active_low: false       # ac relay boards that energize on a low output
  pins: []                # AC: one GPIO per valve, e.g. [4, 5, 6, 7]
  open_pins: []           # latching: open-pulse GPIO per valve
  close_pins: []          # latching: close-pulse GPIO per valve

battery:
  mode: fixed             # analog / rs485 planned
  level: 100              # percent reported to clients
```

### Valve count

`valves` selects how many zones the timer exposes. Each maps to a real Melnor
SKU (see `melnor_bluetooth/constants.py`):

| Valves | Model (0x2A29) | Internal code |
|--------|----------------|---------------|
| 1      | `93015`        | 5912          |
| 2      | `93100`        | 5910          |
| 4      | `93280`        | 5908          |

The GATT layout doesn't change with valve count — real 1/2-valve timers still
use the 4-valve byte format — only the reported model string and zone count do.

### Controller mode (driving real valves)

Set valve pins and the firmware drives physical valves whenever a client turns a
zone on. Leave the pin lists empty (the default) and it stays a pure emulator.

**AC valves via relays** — one GPIO per valve driving a relay module; the
relay's NO contact switches a 24VAC line into the valve. Held on for the whole
watering period.

```yaml
drive:
  mode: ac
  active_low: false       # set true for active-low relay boards
  pins: [4, 5, 6, 7]
```

**DC latching solenoids** — two GPIO per valve. A brief pulse on the open line
latches the valve on; a pulse on the close line latches it off. The solenoid
holds position mechanically between pulses.

```yaml
drive:
  mode: latching
  pulse_ms: 80
  open_pins:  [4, 6, 8, 10]
  close_pins: [5, 7, 9, 11]
```

Notes:

- Pin lists are ordered by valve (valve 1 first). You need as many entries as
  `valves`, or actuation stays disabled and it runs as a pure emulator.
- At boot, latching valves are pulsed **closed** so firmware state matches the
  physical valve (a latching solenoid powers up in an unknown position).
- Actuation follows the **manual** on/off signal (`0xEC0B`). Schedule-driven
  watering (the client's frequency schedule) is not yet evaluated on-device.
- Drive real relay/solenoid loads through proper drivers (transistor/MOSFET or
  a relay module), not straight off a GPIO. Avoid strapping/flash pins.

### Device name

`device_name` is exposed on characteristic `0xEC01` — the library's
`DEVICE_USER_NAME_UUID`, which the Melnor app is the most likely client to read.
It's read/write, so a client renaming the device is echoed back. Per-valve
naming isn't supported: the library models none, and the real app's UUID/format
would need a BLE capture to reproduce faithfully.

## Verifying it works

1. Flash the board; the monitor prints `Emulating <model> (<n>-valve Melnor); advertising`.
2. In a client (Melnor app or HA) a Melnor timer should appear whose name is the
   board's BLE MAC. Pair/confirm it.
3. You get one device with the configured number of zones. Toggling a zone's
   manual switch flips the matching GPIO (in controller mode) and the state
   sticks on readback.

The client needs a Bluetooth radio (or a Bluetooth proxy) in range of the board.

## What it emulates (byte map)

All values are the idle state; characteristics a client writes to are echoed
back on read, which is why valve on/off "sticks".

| UUID     | Dir | Meaning                                                        |
|----------|-----|----------------------------------------------------------------|
| `0x2A29` | R   | Manufacturer string, e.g. `"9328004"` → model `93280`, **4 valves** |
| `0xEC01` | R/W | Device user name, seeded from `device_name`                   |
| `0xEC08` | R   | Battery, 2 bytes voltage-encoded (from `battery.level`)        |
| `0xEC0B` | R/W | 4×5 bytes: `is_watering`, minutes, minutes — client writes to toggle; drives GPIO in controller mode |
| `0xEC06` | R   | 4×5 bytes: per-valve manual end timestamps (idle: zero)        |
| `0xEC0A` | R/W | 4 bytes: per-valve schedule-enabled flags                     |
| `0xEC0F`–`0xEC12` | R/W | 8 bytes each: `>BIHB` frequency schedule per valve   |
| `0xEC09` | R/W | u32 timestamp a client writes on `push_state`                 |

The digit at index 6 of the `0x2A29` string is what sets the zone count
(`int(string[6:7])` in `melnor_bluetooth`) — `valves` in `config.yaml` drives it.

## Status

Builds clean for the `esp32` target against `esp-idf-svc` 0.52 / `esp-idf-hal`
0.46 / `esp32-nimble` 0.12 (see `Cargo.lock`). **Not yet runtime-tested on a
board** — it has never been flashed and run, so BLE behavior against a real
Melnor client and the GPIO valve driving are still unverified on hardware.

Notes:

- YAML parsing uses `serde_yaml` (archived but stable, pure-Rust); swap for
  `serde_yml` / `serde_norway` if you prefer an actively-maintained fork.
- Runtime pin selection uses `unsafe AnyIOPin::steal(n)` — each GPIO number must
  be assigned to only one valve (config enforces distinct pins).

[`esp32-nimble`]: https://crates.io/crates/esp32-nimble
