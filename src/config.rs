//! Device configuration, defined in `config.yaml` at the repo root and baked
//! into the firmware at build time (`include_str!`), then parsed once at boot.
//!
//! Keeping config in one YAML file (rather than flash-time flags) gives future
//! features — analog-input battery sensing, RS-232/485 reporting — a place to
//! declare their pins and parameters without growing the flash command.

use serde::Deserialize;

const CONFIG_YAML: &str = include_str!("../config.yaml");

/// How the physical valves are driven.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveMode {
    /// One output per valve driving a relay whose NO contact feeds 24VAC to the
    /// valve. Level-held while watering.
    Ac,
    /// DC latching solenoid: two outputs per valve. A brief pulse on the open
    /// line latches it on; a pulse on the close line latches it off.
    Latching,
}

// ---------- YAML schema (defaults let a partial config still parse) ----------

#[derive(Deserialize)]
#[serde(default)]
struct RawConfig {
    device_name: String,
    valves: u8,
    drive: RawDrive,
    battery: RawBattery,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            device_name: "Melnor ESP".to_string(),
            valves: 4,
            drive: RawDrive::default(),
            battery: RawBattery::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct RawDrive {
    mode: String,
    pulse_ms: u32,
    active_low: bool,
    pins: Vec<u8>,
    open_pins: Vec<u8>,
    close_pins: Vec<u8>,
}

impl Default for RawDrive {
    fn default() -> Self {
        Self {
            mode: String::new(),
            pulse_ms: 80,
            active_low: false,
            pins: Vec::new(),
            open_pins: Vec::new(),
            close_pins: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct RawBattery {
    mode: String,
    level: u8,
}

impl Default for RawBattery {
    fn default() -> Self {
        Self {
            mode: "fixed".to_string(),
            level: 100,
        }
    }
}

// ---------- Resolved config the firmware actually uses ----------

pub struct Config {
    pub device_name: String,
    pub valves: u8,
    /// 0x2A29 value: marketing model number (5 chars) + pad + valve-count digit.
    pub name_string: &'static [u8],
    /// Low manufacturer-data byte (0x59XX -> internal model code "59XX").
    pub model_byte: u8,
    pub model: &'static str,
    pub drive: DriveMode,
    pub ac_pins: Vec<u8>,
    pub open_pins: Vec<u8>,
    pub close_pins: Vec<u8>,
    pub pulse_ms: u32,
    pub active_low: bool,
    /// True only when enough pins are configured for `valves` in `drive` mode.
    pub actuation_enabled: bool,
    /// Two-byte 0xEC08 payload the client decodes into a battery percentage.
    pub battery: [u8; 2],
}

pub fn load() -> Config {
    let raw: RawConfig = serde_yaml::from_str(CONFIG_YAML).unwrap_or_else(|e| {
        log::error!("config.yaml parse error ({e}); using defaults");
        RawConfig::default()
    });

    // Valve count -> emulated SKU (melnor_bluetooth/constants.py):
    //   1 -> 93015 (code 5912), 2 -> 93100 (5910), 4 -> 93280 (5908).
    let valves = match raw.valves {
        1 | 2 | 4 => raw.valves,
        other => {
            log::warn!("valves={other} invalid (use 1, 2, or 4); using 4");
            4
        }
    };
    let (name_string, model_byte, model): (&[u8], u8, &str) = match valves {
        1 => (b"9301501", 0x12, "93015"),
        2 => (b"9310002", 0x10, "93100"),
        _ => (b"9328004", 0x08, "93280"),
    };

    // Explicit mode wins; otherwise infer from which pins were given.
    let drive = match raw.drive.mode.as_str() {
        "latching" => DriveMode::Latching,
        "ac" => DriveMode::Ac,
        _ if !raw.drive.open_pins.is_empty() || !raw.drive.close_pins.is_empty() => {
            DriveMode::Latching
        }
        _ => DriveMode::Ac,
    };

    let need = valves as usize;
    let actuation_enabled = match drive {
        DriveMode::Ac => raw.drive.pins.len() >= need,
        DriveMode::Latching => {
            raw.drive.open_pins.len() >= need && raw.drive.close_pins.len() >= need
        }
    };

    if raw.battery.mode != "fixed" {
        log::warn!(
            "battery.mode='{}' not implemented yet; reporting fixed level",
            raw.battery.mode
        );
    }

    Config {
        device_name: if raw.device_name.is_empty() {
            "Melnor ESP".to_string()
        } else {
            raw.device_name
        },
        valves,
        name_string,
        model_byte,
        model,
        drive,
        ac_pins: raw.drive.pins,
        open_pins: raw.drive.open_pins,
        close_pins: raw.drive.close_pins,
        pulse_ms: raw.drive.pulse_ms,
        active_low: raw.drive.active_low,
        actuation_enabled,
        battery: battery_bytes(raw.battery.level),
    }
}

/// Encode a battery percentage into the 2 bytes 0xEC08 returns. Inverts the
/// library's `parse_battery_value`: pct = (b0 + b1/256 - 2.35) * 181.818.
fn battery_bytes(pct: u8) -> [u8; 2] {
    let pct = pct.min(100);
    if pct >= 100 {
        // Overshoots; the client clamps to 100.
        return [0x03, 0x00];
    }
    let x = pct as f32 / 181.818 + 2.35;
    let b0 = x.floor() as u8;
    let b1 = (((x - b0 as f32) * 256.0).round() as i32).clamp(0, 255) as u8;
    // (0xEE, 0xEE) is the library's "0%" sentinel; nudge off it if we land there.
    if b0 == 0xEE && b1 == 0xEE {
        [0xEE, 0xED]
    } else {
        [b0, b1]
    }
}
