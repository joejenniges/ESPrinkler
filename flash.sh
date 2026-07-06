#!/usr/bin/env bash
# Build + flash the Melnor emulator/controller to a chosen ESP chip.
#
#   ./flash.sh [chip] [cargo args...]
#
# chip defaults to esp32s3 (BLE-capable chips only). Device behavior — valves,
# pins, drive mode, pulse width, battery, device name — comes from config.yaml,
# which is baked into the firmware at build time. Edit config.yaml and re-flash
# to change it.
set -euo pipefail

chip="${1:-esp32s3}"
shift || true

# Remaining args (optionally after a --) pass through to cargo, e.g. --port.
if [[ "${1:-}" == "--" ]]; then
  shift
fi
cargo_args=("$@")

case "$chip" in
  esp32)            triple=xtensa-esp32-espidf ;;
  esp32s3)          triple=xtensa-esp32s3-espidf ;;
  esp32c2|esp32c3)  triple=riscv32imc-esp-espidf ;;
  esp32c6|esp32h2)  triple=riscv32imac-esp-espidf ;;
  esp32s2)
    echo "esp32s2 has no BLE radio — cannot emulate a Melnor on it." >&2
    exit 1 ;;
  *)
    echo "unknown chip '$chip'. Supported: esp32 esp32s3 esp32c2 esp32c3 esp32c6 esp32h2" >&2
    exit 1 ;;
esac

echo "==> $chip ($triple); device config from config.yaml"
exec env MCU="$chip" CARGO_BUILD_TARGET="$triple" \
  cargo run --release ${cargo_args[@]+"${cargo_args[@]}"}
