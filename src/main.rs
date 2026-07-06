//! Melnor 4-valve Bluetooth water-timer emulator for the ESP32-S3.
//!
//! Emulates enough of a real Melnor timer for Home Assistant's `melnor`
//! integration (and the `melnor-bluetooth` library it depends on) to discover
//! it, connect, and drive it.
//!
//! Two halves have to line up:
//!   1. The advertisement carries manufacturer id 13 (0x000D) with payload
//!      starting 0x59. This is the ONLY thing HA/the library match on.
//!   2. A GATT server exposes the characteristics the library reads/writes.
//!
//! WHY passive echo: the library reads valve state back from the same
//! characteristics it writes to (e.g. turning a valve on writes is_watering=1
//! to 0xEC0B, then reads it back). NimBLE stores the last written value and
//! returns it on read, so we get working control with almost no logic.

use esp32_nimble::{
    utilities::BleUuid, BLEAdvertisementData, BLEDevice, NimbleProperties,
};
use esp_idf_svc::hal::delay::FreeRtos;

/// Bluetooth base-UUID 16-bit shorthand. The library builds its UUIDs as
/// `0000ecXX-0000-1000-8000-00805f9b34fb`, which is exactly the base UUID with
/// a 16-bit value, so registering the 16-bit form here produces an identical
/// 128-bit UUID on the wire.
fn uuid16(x: u16) -> BleUuid {
    BleUuid::from_uuid16(x)
}

// --- Characteristic UUIDs (see melnor_bluetooth/constants.py) ---
const MANUFACTURER_NAME: u16 = 0x2A29; // read: "model + valve count" string
const BATTERY: u16 = 0xEC08; // read: 2-byte voltage encoding
const MANUAL_SETTINGS: u16 = 0xEC0B; // r/w: 4x5 bytes, is_watering + minutes
const MANUAL_STATES: u16 = 0xEC06; // read: 4x5 bytes, manual end timestamps
const ON_OFF: u16 = 0xEC0A; // r/w: 4 bytes, per-valve schedule enabled
const VALVE_0_MODE: u16 = 0xEC0F; // r/w: 8-byte frequency schedule
const VALVE_1_MODE: u16 = 0xEC10;
const VALVE_2_MODE: u16 = 0xEC11;
const VALVE_3_MODE: u16 = 0xEC12;
const UPDATED_AT: u16 = 0xEC09; // write: u32 timestamp on push_state

// Arbitrary container for the vendor characteristics. bleak resolves
// characteristics by UUID across all services, so the service UUID is cosmetic.
const MELNOR_SERVICE: u16 = 0xEC00;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let ble_device = BLEDevice::take();
    let ble_advertising = ble_device.get_advertising();
    let server = ble_device.get_server();

    server.on_connect(|_server, desc| {
        log::info!("HA connected: {desc:?}");
    });
    // NimBLE stops advertising once connected; restart on disconnect so HA can
    // reconnect after a drop. BLEDevice::take() returns the same singleton.
    server.on_disconnect(|_desc, reason| {
        log::info!("HA disconnected ({reason:?}); re-advertising");
        BLEDevice::take().get_advertising().lock().start().ok();
    });

    // Device Information Service holds the manufacturer-name string that
    // _read_model() parses: model = string[0:5], valve_count = int(string[6:7]).
    // "5907004" -> model "59070", valve_count 4. The '4' at index 6 is what
    // makes HA create four zones.
    let dis = server.create_service(uuid16(0x180A));
    let name = dis
        .lock()
        .create_characteristic(uuid16(MANUFACTURER_NAME), NimbleProperties::READ);
    name.lock().set_value(b"5907004");

    let svc = server.create_service(uuid16(MELNOR_SERVICE));

    // Battery ~90%. parse_battery_value: (b0 + b1/256 - 2.35) * 181.818, so
    // 0x02,0xD8 -> ~89%. b0==0xEE && b1==0xEE would mean 0%.
    let battery = svc
        .lock()
        .create_characteristic(uuid16(BATTERY), NimbleProperties::READ);
    battery.lock().set_value(&[0x02, 0xD8]);

    // Idle manual settings, 5 bytes per valve: is_watering=0, minutes=20 (0x0014),
    // minutes duplicate. HA overwrites this whole 20-byte blob to toggle valves.
    let manual = svc.lock().create_characteristic(
        uuid16(MANUAL_SETTINGS),
        NimbleProperties::READ | NimbleProperties::WRITE,
    );
    manual.lock().set_value(&[
        0x00, 0x00, 0x14, 0x00, 0x14, // valve 0
        0x00, 0x00, 0x14, 0x00, 0x14, // valve 1
        0x00, 0x00, 0x14, 0x00, 0x14, // valve 2
        0x00, 0x00, 0x14, 0x00, 0x14, // valve 3
    ]);

    // Manual end-time states, read-only: byte0 + u32 timestamp per valve. All
    // zero = nothing counting down.
    let states = svc
        .lock()
        .create_characteristic(uuid16(MANUAL_STATES), NimbleProperties::READ);
    states.lock().set_value(&[0u8; 20]);

    // Per-valve frequency-schedule enabled flags (read at offset = valve id).
    let on_off = svc.lock().create_characteristic(
        uuid16(ON_OFF),
        NimbleProperties::READ | NimbleProperties::WRITE,
    );
    on_off.lock().set_value(&[0u8; 4]);

    // Frequency schedule per valve: >BIHB (0, start_ts, duration, interval).
    // All-zero = schedule disabled (interval 0 -> no next run).
    for mode in [VALVE_0_MODE, VALVE_1_MODE, VALVE_2_MODE, VALVE_3_MODE] {
        let c = svc.lock().create_characteristic(
            uuid16(mode),
            NimbleProperties::READ | NimbleProperties::WRITE,
        );
        c.lock().set_value(&[0u8; 8]);
    }

    // push_state writes a u32 timestamp here; present so the write succeeds.
    let updated = svc.lock().create_characteristic(
        uuid16(UPDATED_AT),
        NimbleProperties::READ | NimbleProperties::WRITE,
    );
    updated.lock().set_value(&[0u8; 4]);

    // Manufacturer data: company id 13 little-endian (0x0D 0x00) + payload
    // starting 0x59. 0x59 0x07 mimics a model-5907 4-valve unit. This is the
    // match key for both HA's manifest matcher and the library scanner.
    ble_advertising.lock().set_data(
        BLEAdvertisementData::new()
            .name("YM_Timer")
            .manufacturer_data(&[0x0D, 0x00, 0x59, 0x07]),
    )?;
    ble_advertising.lock().start()?;

    log::info!("Melnor emulator advertising; waiting for Home Assistant");
    loop {
        FreeRtos::delay_ms(1000);
    }
}
