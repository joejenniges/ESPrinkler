# ESPrinkler

A WiFi sprinkler-valve controller for the ESP32, in Rust on `esp-idf`.

- **Provisioning**: on first boot (or if it can't join saved WiFi) it hosts a
  SoftAP (`ESPrinkler-Setup`); connect and open its page to enter your home WiFi.
- **mDNS**: once on your network it advertises `esprinkler.local` and
  `_sprinkler._tcp`.
- **Valves**: configured in `config.toml` — any mix of AC relays and DC latching
  solenoids, with their GPIO pins.
- Manual control, schedules, delays and timers are planned.

> Previously this repo was a Melnor BLE water-timer emulator; that work lives on
> the `melnor-emulator` branch.

## Supported chips

Any WiFi-capable ESP. Pass the chip to `flash.sh`:

| Chip      | Target triple            |
|-----------|--------------------------|
| `esp32`   | `xtensa-esp32-espidf`    |
| `esp32s3` | `xtensa-esp32s3-espidf`  |
| `esp32c3` | `riscv32imc-esp-espidf`  |
| `esp32c6` | `riscv32imac-esp-espidf` |

## Prerequisites (one time)

```sh
cargo install espup ldproxy espflash
espup install
. $HOME/export-esp.sh   # re-source per shell
```

## Build + flash

```sh
./flash.sh              # esp32s3 (default)
./flash.sh esp32        # any chip from the table
```

`flash.sh` selects the chip/target and runs `cargo run --release` (build, flash,
serial monitor). Device behavior comes from `config.toml`.

## Configuration (`config.toml`)

```toml
[ap]
ssid = "ESPrinkler-Setup"
password = "sprinkler"     # >= 8 chars, or "" for open

[mdns]
hostname = "esprinkler"    # esprinkler.local
service = "_sprinkler"     # _sprinkler._tcp

# AC relay valve (single output pin)
[[valve]]
name = "Zone 1"
type = "ac"
pin = 4
active_low = false         # true for active-low relay boards

# DC latching solenoid valve (open/close pins, brief pulse)
[[valve]]
name = "Zone 2"
type = "latching"
open_pin = 6
close_pin = 7
pulse_ms = 80
```

WiFi credentials are **not** in config — they're entered via the setup page and
stored in NVS. To re-provision, erase NVS (or add a reset route later).

## First run

1. Flash. With no saved WiFi, the serial log shows `starting setup AP`.
2. Join `ESPrinkler-Setup` from a phone/laptop, open `http://192.168.71.1/`
   (the SoftAP gateway), enter your home WiFi, submit.
3. It reboots, joins your network, and is reachable at `http://esprinkler.local/`.

## Status

Foundation: TOML config, WiFi STA/AP provisioning, NVS credential storage, HTTP
setup + status pages, mDNS. Valve GPIO is initialised (closed) but manual/
scheduled control is not wired yet.
