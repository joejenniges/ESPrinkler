//! Build-time configuration from `config.toml` (baked in via `include_str!`,
//! parsed at boot). WiFi credentials are runtime (NVS), not here.

use serde::Deserialize;

const CONFIG_TOML: &str = include_str!("../config.toml");

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    ap: Ap,
    #[serde(default)]
    mdns: Mdns,
    #[serde(default)]
    valve: Vec<Valve>,
}

#[derive(Deserialize, Clone)]
pub struct Ap {
    pub ssid: String,
    pub password: String,
}

impl Default for Ap {
    fn default() -> Self {
        Self {
            ssid: "ESPrinkler-Setup".to_string(),
            password: "sprinkler".to_string(),
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct Mdns {
    pub hostname: String,
    pub service: String,
}

impl Default for Mdns {
    fn default() -> Self {
        Self {
            hostname: "esprinkler".to_string(),
            service: "_sprinkler".to_string(),
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct Valve {
    pub name: String,
    /// "ac" (relay, single pin) or "latching" (solenoid, open/close pins).
    #[serde(rename = "type")]
    pub kind: String,
    pub pin: Option<u8>,
    #[serde(default)]
    pub active_low: bool,
    pub open_pin: Option<u8>,
    pub close_pin: Option<u8>,
    #[serde(default = "default_pulse_ms")]
    pub pulse_ms: u32,
}

fn default_pulse_ms() -> u32 {
    80
}

pub struct Config {
    pub ap: Ap,
    pub mdns: Mdns,
    pub valves: Vec<Valve>,
}

pub fn load() -> Config {
    let raw: Raw = toml::from_str(CONFIG_TOML).unwrap_or_else(|e| {
        log::error!("config.toml parse error ({e}); using defaults");
        Raw {
            ap: Ap::default(),
            mdns: Mdns::default(),
            valve: Vec::new(),
        }
    });
    Config {
        ap: raw.ap,
        mdns: raw.mdns,
        valves: raw.valve,
    }
}
