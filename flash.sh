#!/usr/bin/env bash
# Build + flash the Melnor emulator to a chosen ESP chip and valve count.
#   ./flash.sh [chip] [valves] [extra cargo args...]
# chip   defaults to esp32s3 (BLE-capable chips only).
# valves defaults to 4; one of 1, 2, 4 -> model 93015 / 93100 / 93280.
set -euo pipefail

chip="${1:-esp32s3}"
shift || true

# Optional second positional: a bare number is treated as the valve count.
valves=4
if [[ "${1:-}" =~ ^[0-9]+$ ]]; then
  case "$1" in
    1|2|4) valves="$1"; shift ;;
    *) echo "valves must be 1, 2, or 4 (got '$1')" >&2; exit 1 ;;
  esac
fi

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

case "$valves" in
  1) model=93015 ;;
  2) model=93100 ;;
  4) model=93280 ;;
esac

echo "==> $chip ($triple), ${valves}-valve (model $model)"
exec env MCU="$chip" CARGO_BUILD_TARGET="$triple" MELNOR_VALVES="$valves" \
  cargo run --release "$@"
