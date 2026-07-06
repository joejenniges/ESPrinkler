# Melnor ESP32 emulator

Emulates a 4-valve Melnor Bluetooth water timer so Home Assistant's `melnor`
integration discovers it, connects, and can toggle the valves — without owning
a real timer. Written in Rust on `esp-idf` + [`esp32-nimble`].

## Why an ESP, not a Mac

The integration and the `melnor-bluetooth` library match on exactly one thing:
a BLE advertisement with **manufacturer id 13 (0x000D)** whose payload starts
with **0x59**. macOS CoreBluetooth refuses to set manufacturer data in
advertisements, so a Mac can never be discovered as a Melnor. An ESP can.

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
./flash.sh              # defaults to esp32s3
./flash.sh esp32c3      # or any chip from the table
./flash.sh esp32 -- --port /dev/tty.usbserial-XXXX
```

`flash.sh` sets `MCU` and `CARGO_BUILD_TARGET` for the chosen chip, then runs
`cargo run --release` (which builds, flashes, and opens the serial monitor).
For a build only:

```sh
MCU=esp32c6 CARGO_BUILD_TARGET=riscv32imac-esp-espidf cargo build --release
```

## Verifying discovery

1. Flash the board; the monitor prints `advertising; waiting for Home Assistant`.
2. In HA the `melnor` integration should surface a discovery ("Melnor Bluetooth")
   whose name is the board's BLE MAC. Confirm it.
3. You get one device with **4 zones**, each exposing manual + frequency
   switches, a duration number, a schedule time, and battery/RSSI sensors.

HA needs a Bluetooth adapter or a Bluetooth proxy in range of the board.

## What it emulates (byte map)

All values are the idle state; characteristics HA writes to are echoed back on
read, which is why valve on/off "sticks".

| UUID     | Dir | Meaning                                                        |
|----------|-----|----------------------------------------------------------------|
| `0x2A29` | R   | Manufacturer string `"5907004"` → model `59070`, **4 valves**  |
| `0xEC08` | R   | Battery, 2 bytes voltage-encoded (`02 D8` ≈ 90%)               |
| `0xEC0B` | R/W | 4×5 bytes: `is_watering`, minutes, minutes — HA writes to toggle |
| `0xEC06` | R   | 4×5 bytes: per-valve manual end timestamps (idle: zero)        |
| `0xEC0A` | R/W | 4 bytes: per-valve schedule-enabled flags                     |
| `0xEC0F`–`0xEC12` | R/W | 8 bytes each: `>BIHB` frequency schedule per valve   |
| `0xEC09` | R/W | u32 timestamp HA writes on `push_state`                       |

The `4` at index 6 of the `0x2A29` string is what makes HA create four zones
(`int(string[6:7])` in `melnor_bluetooth`), so don't change it unless you want
a 1/2-valve device.

## Caveat

This was written against the `esp32-nimble` 0.8 / `esp-idf-svc` 0.49 APIs but
**not compile-tested on hardware here** — if you're on a different crate
version, the BLE builder calls (`BLEAdvertisementData::manufacturer_data`,
`create_characteristic`, `NimbleProperties`) are the most likely spots to need
a small tweak. Pin the versions in `Cargo.toml` or adjust to your installed
ones.

[`esp32-nimble`]: https://crates.io/crates/esp32-nimble
