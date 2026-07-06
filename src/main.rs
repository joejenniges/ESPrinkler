//! Melnor Bluetooth water-timer emulator / sprinkler controller for ESP32.
//!
//! Presents itself as a real Melnor timer so any Melnor BLE client — the
//! Melnor phone app, Home Assistant's `melnor` integration, or anything else
//! speaking the same protocol — discovers, connects to, and drives it.
//!
//! Two halves have to line up:
//!   1. The advertisement carries manufacturer id 13 (0x000D) with payload
//!      starting 0x59. This is the ONLY thing a Melnor client matches on.
//!   2. A GATT server exposes the characteristics the client reads/writes.
//!
//! WHY passive echo: clients read valve state back from the same
//! characteristics they write to (turning a valve on writes is_watering=1 to
//! 0xEC0B, then reads it back). NimBLE stores the last written value and
//! returns it on read, so the emulation needs almost no logic.
//!
//! When valve GPIO pins are configured (see config.rs / flash.sh) this also
//! acts as a real controller: writes to 0xEC0B drive physical valves. With no
//! pins configured it stays a pure emulator.

mod config;
mod valves;

use core::sync::atomic::{AtomicU8, Ordering};

use esp32_nimble::{
    utilities::BleUuid, BLEAdvertisementData, BLEDevice, NimbleProperties,
};
use esp_idf_svc::hal::delay::FreeRtos;

/// Desired per-valve watering state as a bitmask (bit i = valve i on). Written
/// by the BLE callback, read by the control loop. Lock-free so the BLE host
/// task never blocks on the actuator.
static DESIRED: AtomicU8 = AtomicU8::new(0);

/// Bluetooth base-UUID 16-bit shorthand. The library builds its UUIDs as
/// `0000ecXX-0000-1000-8000-00805f9b34fb`, which is exactly the base UUID with
/// a 16-bit value, so registering the 16-bit form here produces an identical
/// 128-bit UUID on the wire.
fn uuid16(x: u16) -> BleUuid {
    BleUuid::from_uuid16(x)
}

// --- Characteristic UUIDs (see melnor_bluetooth/constants.py) ---
const MANUFACTURER_NAME: u16 = 0x2A29; // read: "model + valve count" string
const DEVICE_NAME: u16 = 0xEC01; // r/w: device user name (app-facing)
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

    let cfg = config::load();

    let ble_device = BLEDevice::take();
    let ble_advertising = ble_device.get_advertising();
    let server = ble_device.get_server();

    server.on_connect(|_server, desc| {
        log::info!("Melnor client connected: {desc:?}");
    });
    // NimBLE stops advertising once connected; restart on disconnect so the
    // client can reconnect. BLEDevice::take() returns the same singleton.
    server.on_disconnect(|_desc, reason| {
        log::info!("Melnor client disconnected ({reason:?}); re-advertising");
        BLEDevice::take().get_advertising().lock().start().ok();
    });

    // Device Information Service holds the manufacturer-name string that
    // _read_model() parses: model = string[0:5], valve_count = int(string[6:7]).
    // The digit at index 6 is what makes a client create that many zones.
    let dis = server.create_service(uuid16(0x180A));
    let name = dis
        .lock()
        .create_characteristic(uuid16(MANUFACTURER_NAME), NimbleProperties::READ);
    name.lock().set_value(cfg.name_string);

    let svc = server.create_service(uuid16(MELNOR_SERVICE));

    // Device user name (0xEC01, DEVICE_USER_NAME_UUID in the library). The
    // library defines this UUID but never uses it; the Melnor app is the most
    // likely reader/writer. Seeded from config; echoes client writes.
    let device_name = svc.lock().create_characteristic(
        uuid16(DEVICE_NAME),
        NimbleProperties::READ | NimbleProperties::WRITE,
    );
    device_name.lock().set_value(cfg.device_name.as_bytes());

    // Battery percentage from config (default 100%). Encoded by battery_bytes
    // to match the library's parse_battery_value.
    let battery = svc
        .lock()
        .create_characteristic(uuid16(BATTERY), NimbleProperties::READ);
    battery.lock().set_value(&cfg.battery);

    // Idle manual settings, 5 bytes per valve: is_watering=0, minutes=20 (0x0014),
    // minutes duplicate. A client overwrites this whole 20-byte blob to toggle
    // valves; byte 0 of each 5-byte group is is_watering.
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
    // Translate a client's valve on/off write into the desired-state bitmask
    // the control loop actuates. NimBLE still stores the written value for
    // readback, so is_watering echoes back as before.
    let n_valves = cfg.valves as usize;
    manual.lock().on_write(move |args| {
        let data = args.recv_data();
        let mut mask = 0u8;
        for i in 0..n_valves {
            let offset = i * 5;
            if data.len() > offset && data[offset] != 0 {
                mask |= 1 << i;
            }
        }
        DESIRED.store(mask, Ordering::Relaxed);
        log::info!("valve request: {mask:04b}");
    });

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
    // starting 0x59. The 0x59 start is the match key every Melnor client uses;
    // the model_byte follows for authenticity.
    ble_advertising.lock().set_data(
        BLEAdvertisementData::new()
            .name("YM_Timer")
            .manufacturer_data(&[0x0D, 0x00, 0x59, cfg.model_byte]),
    )?;
    ble_advertising.lock().start()?;

    log::info!(
        "Emulating {} ({}-valve Melnor); advertising",
        cfg.model,
        cfg.valves
    );

    if !cfg.actuation_enabled {
        log::info!("no valve pins configured; running BLE-only (pure emulator)");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }

    // Controller mode: reconcile physical valves to the requested state.
    log::info!(
        "actuation: {:?}, pulse {}ms, active_low {}",
        cfg.drive,
        cfg.pulse_ms,
        cfg.active_low
    );
    let mut actuators = valves::build(&cfg)?;
    let mut actual = [false; 4];
    loop {
        let mask = DESIRED.load(Ordering::Relaxed);
        for (i, actuator) in actuators.iter_mut().enumerate() {
            let want = mask & (1 << i) != 0;
            if let Err(e) = actuator.apply(want, &mut actual[i]) {
                log::error!("valve {i} actuation error: {e}");
            }
        }
        FreeRtos::delay_ms(20);
    }
}
