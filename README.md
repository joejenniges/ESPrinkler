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
./flash.sh              # esp32s3, 4-valve (default)
./flash.sh esp32c3      # any chip from the table, 4-valve
./flash.sh esp32s3 1    # 1-valve unit
./flash.sh esp32s3 2    # 2-valve unit
./flash.sh esp32 4 --port /dev/tty.usbserial-XXXX
```

`flash.sh` sets `MCU`, `CARGO_BUILD_TARGET`, and `MELNOR_VALVES` for the chosen
chip/valve count, then runs `cargo run --release` (which builds, flashes, and
opens the serial monitor). For a build only:

```sh
MCU=esp32c6 CARGO_BUILD_TARGET=riscv32imac-esp-espidf MELNOR_VALVES=2 \
  cargo build --release
```

### Valve count

The second argument selects how many zones the emulated timer exposes. Each
maps to a real Melnor SKU (see `melnor_bluetooth/constants.py`):

| Valves | Model (0x2A29) | Internal code |
|--------|----------------|---------------|
| 1      | `93015`        | 5912          |
| 2      | `93100`        | 5910          |
| 4      | `93280`        | 5908          |

Valve count is baked in at build time (`MELNOR_VALVES`, read via `option_env!`),
so changing it re-flashes new firmware. The GATT layout itself doesn't change —
real 1/2-valve timers still use the 4-valve byte format — only the reported
model string and zone count differ.

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
| `0x2A29` | R   | Manufacturer string, e.g. `"9328004"` → model `93280`, **4 valves** |
| `0xEC08` | R   | Battery, 2 bytes voltage-encoded (`02 D8` ≈ 90%)               |
| `0xEC0B` | R/W | 4×5 bytes: `is_watering`, minutes, minutes — HA writes to toggle |
| `0xEC06` | R   | 4×5 bytes: per-valve manual end timestamps (idle: zero)        |
| `0xEC0A` | R/W | 4 bytes: per-valve schedule-enabled flags                     |
| `0xEC0F`–`0xEC12` | R/W | 8 bytes each: `>BIHB` frequency schedule per valve   |
| `0xEC09` | R/W | u32 timestamp HA writes on `push_state`                       |

The digit at index 6 of the `0x2A29` string is what sets the zone count
(`int(string[6:7])` in `melnor_bluetooth`) — the valve-count argument to
`flash.sh` drives it.

## Caveat

This was written against the `esp32-nimble` 0.8 / `esp-idf-svc` 0.49 APIs but
**not compile-tested on hardware here** — if you're on a different crate
version, the BLE builder calls (`BLEAdvertisementData::manufacturer_data`,
`create_characteristic`, `NimbleProperties`) are the most likely spots to need
a small tweak. Pin the versions in `Cargo.toml` or adjust to your installed
ones.

[`esp32-nimble`]: https://crates.io/crates/esp32-nimble
