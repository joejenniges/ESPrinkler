//! ESPrinkler — a WiFi sprinkler controller for the ESP32.
//!
//! Boot flow:
//!   1. Load valve config from config.toml; initialise the GPIO (valves closed).
//!   2. Read home-WiFi credentials from NVS. If present, join as a station (STA);
//!      if absent or the join fails, start a SoftAP for setup.
//!   3. STA: advertise mDNS (<hostname>.local + _sprinkler._tcp) and serve a
//!      status page. AP: serve a WiFi-setup form that saves creds and reboots.
//!
//! Manual valve control, schedules, delays and timers are deferred.

mod config;
mod valves;
mod wifi;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let cfg = config::load();
    log::info!("ESPrinkler starting; {} valve(s) configured", cfg.valves.len());

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs_part = EspDefaultNvsPartition::take()?;

    // Bring up valve GPIO (all closed). Control logic is deferred; hold the
    // drivers so the pins stay claimed for the program's lifetime.
    let _actuators = valves::build(&cfg.valves)?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs_part.clone()))?,
        sys_loop,
    )?;

    let creds_nvs = EspNvs::new(nvs_part.clone(), "wifi", true)?;
    let mut in_sta = false;
    if let Some((ssid, pass)) = wifi::load_creds(&creds_nvs) {
        log::info!("joining home WiFi '{ssid}'…");
        match wifi::connect_sta(&mut wifi, &ssid, &pass) {
            Ok(()) => {
                in_sta = true;
                if let Ok(ip) = wifi.wifi().sta_netif().get_ip_info() {
                    log::info!("connected; ip = {}", ip.ip);
                }
            }
            Err(e) => log::warn!("WiFi join failed ({e}); starting setup AP"),
        }
    }
    if !in_sta {
        log::info!("no WiFi / join failed; starting setup AP '{}'", cfg.ap.ssid);
        wifi::start_ap(&mut wifi, &cfg.ap.ssid, &cfg.ap.password)?;
    }

    // mDNS (esp_idf_svc::mdns) needs the espressif/mdns managed component wired
    // into the esp-idf-sys build; deferred to a focused follow-up.
    if in_sta {
        log::info!(
            "mDNS TODO: would advertise {}.local , {}._tcp:80",
            cfg.mdns.hostname,
            cfg.mdns.service
        );
    }

    let mut server = EspHttpServer::new(&HttpConfig::default())?;
    let valve_names: Vec<String> = cfg.valves.iter().map(|v| v.name.clone()).collect();

    // GET / : status page (STA) or WiFi setup form (AP).
    server.fn_handler::<anyhow::Error, _>("/", Method::Get, move |req| {
        let html = if in_sta {
            status_page(&valve_names)
        } else {
            setup_page()
        };
        req.into_ok_response()?.write_all(html.as_bytes())?;
        Ok(())
    })?;

    // POST /save : store WiFi creds and reboot to join.
    let save_nvs = nvs_part.clone();
    server.fn_handler::<anyhow::Error, _>("/save", Method::Post, move |mut req| {
        let mut buf = [0u8; 512];
        let n = req.read(&mut buf)?;
        let body = core::str::from_utf8(&buf[..n]).unwrap_or("");
        let ssid = form_field(body, "ssid");
        let pass = form_field(body, "password");
        if ssid.is_empty() {
            req.into_status_response(400)?
                .write_all(b"Missing SSID")?;
            return Ok(());
        }
        if let Ok(mut nvs) = EspNvs::new(save_nvs.clone(), "wifi", true) {
            wifi::save_creds(&mut nvs, &ssid, &pass).ok();
        }
        req.into_ok_response()?
            .write_all(b"Saved. Rebooting to join your WiFi\xE2\x80\xA6")?;
        FreeRtos::delay_ms(800);
        esp_idf_svc::hal::reset::restart();
    })?;

    log::info!(
        "HTTP server on :80 ({} mode)",
        if in_sta { "STA" } else { "AP" }
    );

    // Hold wifi/mdns/server for the program's lifetime; they run on their own tasks.
    loop {
        FreeRtos::delay_ms(1000);
    }
}

/// Extract a URL-encoded form field value ("a=1&b=2" -> field).
fn form_field(body: &str, key: &str) -> String {
    for pair in body.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return url_decode(v);
            }
        }
    }
    String::new()
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |c: u8| match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => 0,
                };
                out.push(hex(bytes[i + 1]) << 4 | hex(bytes[i + 2]));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn setup_page() -> String {
    "<!doctype html><html><head><meta name=viewport content='width=device-width,initial-scale=1'>\
<title>ESPrinkler setup</title><style>body{font-family:sans-serif;max-width:24rem;margin:2rem auto;padding:0 1rem}\
input{width:100%;padding:.5rem;margin:.25rem 0 1rem;box-sizing:border-box}button{padding:.6rem 1rem}</style></head>\
<body><h1>ESPrinkler setup</h1><p>Connect this device to your home WiFi.</p>\
<form method=post action=/save><label>Network (SSID)</label><input name=ssid required>\
<label>Password</label><input name=password type=password>\
<button type=submit>Save &amp; connect</button></form></body></html>"
        .to_string()
}

fn status_page(valve_names: &[String]) -> String {
    let mut rows = String::new();
    for (i, name) in valve_names.iter().enumerate() {
        rows.push_str(&format!("<li>Valve {}: {}</li>", i + 1, name));
    }
    format!(
        "<!doctype html><html><head><meta name=viewport content='width=device-width,initial-scale=1'>\
<title>ESPrinkler</title><style>body{{font-family:sans-serif;max-width:28rem;margin:2rem auto;padding:0 1rem}}</style></head>\
<body><h1>ESPrinkler</h1><p>Connected. {} valve(s):</p><ul>{}</ul>\
<p><em>Manual control &amp; schedules coming soon.</em></p></body></html>",
        valve_names.len(),
        rows
    )
}
