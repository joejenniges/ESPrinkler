#!/usr/bin/env bash
# Build + flash the Melnor emulator to a chosen ESP chip.
#   ./flash.sh [chip] [extra cargo args...]
# chip defaults to esp32s3. Only BLE-capable chips are supported.
set -euo pipefail

chip="${1:-esp32s3}"
shift || true

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

echo "==> $chip ($triple)"
exec env MCU="$chip" CARGO_BUILD_TARGET="$triple" cargo run --release "$@"
