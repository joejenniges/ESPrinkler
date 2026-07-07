//! WiFi provisioning: join saved home WiFi (STA), or fall back to a SoftAP for
//! setup. Credentials live in NVS (namespace "wifi", keys "ssid"/"pass").

use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi,
};

/// Read saved home-WiFi credentials, if any.
pub fn load_creds(nvs: &EspNvs<NvsDefault>) -> Option<(String, String)> {
    let mut ssid_buf = [0u8; 64];
    let mut pass_buf = [0u8; 128];
    let ssid = match nvs.get_str("ssid", &mut ssid_buf) {
        Ok(Some(s)) if !s.is_empty() => s.to_string(),
        _ => return None,
    };
    let pass = match nvs.get_str("pass", &mut pass_buf) {
        Ok(Some(s)) => s.to_string(),
        _ => String::new(),
    };
    Some((ssid, pass))
}

/// Persist home-WiFi credentials.
pub fn save_creds(nvs: &mut EspNvs<NvsDefault>, ssid: &str, pass: &str) -> anyhow::Result<()> {
    nvs.set_str("ssid", ssid)?;
    nvs.set_str("pass", pass)?;
    Ok(())
}

/// Join home WiFi as a station. Blocks until connected + netif up.
pub fn connect_sta(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    pass: &str,
) -> anyhow::Result<()> {
    let auth = if pass.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow::anyhow!("ssid too long"))?,
        password: pass
            .try_into()
            .map_err(|_| anyhow::anyhow!("password too long"))?,
        auth_method: auth,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    Ok(())
}

/// Start a SoftAP for provisioning.
pub fn start_ap(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    pass: &str,
) -> anyhow::Result<()> {
    let auth = if pass.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow::anyhow!("ap ssid too long"))?,
        password: pass
            .try_into()
            .map_err(|_| anyhow::anyhow!("ap password too long"))?,
        auth_method: auth,
        max_connections: 4,
        ..Default::default()
    }))?;
    wifi.start()?;
    Ok(())
}
