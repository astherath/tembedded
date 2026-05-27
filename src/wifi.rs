//! WiFi credential storage + on-device provisioning portal.
//!
//! Flow:
//!   1. `load_creds` reads SSID/PASS from NVS namespace `wifi`.
//!   2. If present, `try_connect` brings up STA mode and returns true on success.
//!   3. If creds are missing or STA fails, `provision` starts an open AP
//!      (`T-Embed-Setup`), serves a small HTML form on port 80, and blocks
//!      until the user POSTs new credentials. On submit it writes them to
//!      NVS and reboots.
//!   4. `clear_creds` wipes NVS — main triggers it on a long encoder press.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use embedded_svc::{
    http::Method,
    io::{Read as _, Write as _},
};
use esp_idf_svc::{
    http::server::{Configuration as HttpServerConfig, EspHttpServer},
    nvs::{EspNvs, EspNvsPartition, NvsDefault},
    wifi::{
        AccessPointConfiguration, AccessPointInfo, AuthMethod, BlockingWifi, ClientConfiguration,
        Configuration as WifiConfig, EspWifi,
    },
};

const NVS_NS: &str = "wifi";
const KEY_SSID: &str = "ssid";
const KEY_PASS: &str = "pass";

pub const AP_SSID: &str = "T-Embed-Setup";
/// Default softAP IP that esp-idf hands out on the AP_NETIF. If you change
/// this you also need to change esp-idf's CONFIG_LWIP_SOFTAP_*_ADDR.
pub const AP_IP: &str = "192.168.71.1";

#[derive(Debug, Clone)]
pub struct Creds {
    pub ssid: String,
    pub pass: String,
}

pub fn load_creds(part: &EspNvsPartition<NvsDefault>) -> Option<Creds> {
    let nvs = EspNvs::new(part.clone(), NVS_NS, true).ok()?;
    let mut sbuf = [0u8; 64];
    let ssid = nvs.get_str(KEY_SSID, &mut sbuf).ok()??.to_string();
    if ssid.is_empty() {
        return None;
    }
    let mut pbuf = [0u8; 128];
    let pass = nvs
        .get_str(KEY_PASS, &mut pbuf)
        .ok()
        .flatten()
        .unwrap_or("")
        .to_string();
    Some(Creds { ssid, pass })
}

pub fn save_creds(part: &EspNvsPartition<NvsDefault>, creds: &Creds) -> anyhow::Result<()> {
    let nvs = EspNvs::new(part.clone(), NVS_NS, true)?;
    nvs.set_str(KEY_SSID, &creds.ssid)?;
    nvs.set_str(KEY_PASS, &creds.pass)?;
    log::info!("wifi: saved creds for SSID={}", creds.ssid);
    Ok(())
}

pub fn clear_creds(part: &EspNvsPartition<NvsDefault>) -> anyhow::Result<()> {
    let nvs = EspNvs::new(part.clone(), NVS_NS, true)?;
    let _ = nvs.remove(KEY_SSID);
    let _ = nvs.remove(KEY_PASS);
    log::info!("wifi: NVS creds cleared");
    Ok(())
}

/// Try a single STA connect. Returns true on success. On failure leaves the
/// wifi driver stopped so the caller can reconfigure it for AP mode.
pub fn try_connect(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    creds: &Creds,
    mut on_status: impl FnMut(&str),
) -> bool {
    on_status(&format!("connecting {}...", creds.ssid));

    let ssid = match creds.ssid.as_str().try_into() {
        Ok(s) => s,
        Err(_) => {
            on_status("ssid too long");
            return false;
        }
    };
    let password = match creds.pass.as_str().try_into() {
        Ok(p) => p,
        Err(_) => {
            on_status("password too long");
            return false;
        }
    };
    let auth_method = if creds.pass.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };

    if let Err(e) = wifi.set_configuration(&WifiConfig::Client(ClientConfiguration {
        ssid,
        password,
        auth_method,
        ..Default::default()
    })) {
        on_status("cfg err");
        log::warn!("wifi cfg err: {e:?}");
        return false;
    }
    if let Err(e) = wifi.start() {
        on_status("start err");
        log::warn!("wifi start err: {e:?}");
        return false;
    }
    if let Err(e) = wifi.connect() {
        on_status("connect failed");
        log::warn!("wifi connect err: {e:?}");
        let _ = wifi.stop();
        return false;
    }
    if let Err(e) = wifi.wait_netif_up() {
        on_status("dhcp failed");
        log::warn!("wifi dhcp err: {e:?}");
        let _ = wifi.stop();
        return false;
    }
    true
}

/// Stages of the on-device provisioning flow. The UI renders a different
/// screen for each.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ProvisionStep {
    /// Intro screen explaining the 3-step process. Auto-advances after a
    /// short delay (or on user input — implemented by the caller).
    Explain,
    /// "Join the T-Embed-Setup network." Auto-advances when at least one
    /// station associates with our soft-AP.
    AwaitJoin,
    /// "Scan the QR / browse to <ip>." Server is up, accepting POSTs.
    AwaitForm,
    /// Form submitted; saving + rebooting. Brief — provision() restarts after.
    Saving,
}

fn ap_station_count() -> usize {
    let mut list: esp_idf_svc::sys::wifi_sta_list_t = unsafe { std::mem::zeroed() };
    let r = unsafe { esp_idf_svc::sys::esp_wifi_ap_get_sta_list(&mut list) };
    if r == 0 {
        list.num as usize
    } else {
        0
    }
}

/// Switch the device into AP mode and serve a one-page setup form until the
/// user submits credentials. On submit the creds are persisted and the chip
/// reboots — this function returns `!`. The caller's `on_step` is invoked at
/// every stage transition so the UI can refresh.
pub fn provision(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    part: EspNvsPartition<NvsDefault>,
    mut on_step: impl FnMut(ProvisionStep),
    mut continue_press: impl FnMut() -> bool,
) -> ! {
    // Step 1: show the explanation first so the user has a moment to read
    // before the AP comes up.
    on_step(ProvisionStep::Explain);

    // Scan now while we hold the radio. Background prep so the AP/HTTP
    // server is ready by the time the user finishes reading.
    let _ = wifi.set_configuration(&WifiConfig::Client(ClientConfiguration::default()));
    let _ = wifi.start();
    let mut aps: Vec<AccessPointInfo> = wifi.scan().unwrap_or_default();
    aps.sort_by(|a, b| b.signal_strength.cmp(&a.signal_strength));
    aps.dedup_by(|a, b| a.ssid == b.ssid);
    log::info!("wifi: scan found {} APs", aps.len());
    let _ = wifi.stop();

    // Wait for the user to press the encoder button to advance past the
    // explanation. We poll the caller-provided closure — it returns true
    // exactly once on the press edge.
    while !continue_press() {
        std::thread::sleep(Duration::from_millis(20));
    }

    let ap_cfg = WifiConfig::AccessPoint(AccessPointConfiguration {
        ssid: AP_SSID.try_into().expect("ap ssid"),
        channel: 1,
        auth_method: AuthMethod::None,
        ssid_hidden: false,
        max_connections: 4,
        ..Default::default()
    });
    wifi.set_configuration(&ap_cfg).expect("ap set cfg");
    wifi.start().expect("ap start");

    // Step 2: AP is up — tell the user to join it. Block here until at
    // least one station associates.
    on_step(ProvisionStep::AwaitJoin);
    log::info!("wifi: AP up as {AP_SSID}, awaiting client join");
    loop {
        if ap_station_count() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    log::info!("wifi: client(s) joined, advancing to form step");

    let saved = Arc::new(AtomicBool::new(false));
    let server_html = build_setup_html(&aps);

    let saved_for_post = Arc::clone(&saved);
    let part_for_post = part.clone();

    let mut server = EspHttpServer::new(&HttpServerConfig::default()).expect("server");

    let html_for_get = server_html.clone();
    server
        .fn_handler::<anyhow::Error, _>("/", Method::Get, move |req| {
            let mut resp = req.into_ok_response()?;
            resp.write_all(html_for_get.as_bytes())?;
            Ok(())
        })
        .expect("get /");

    // Common captive-portal probe endpoints — return the same form so the
    // phone's "internet check" lands us right on setup.
    let html_for_probe = server_html.clone();
    server
        .fn_handler::<anyhow::Error, _>("/generate_204", Method::Get, move |req| {
            let mut resp = req.into_ok_response()?;
            resp.write_all(html_for_probe.as_bytes())?;
            Ok(())
        })
        .expect("get /generate_204");

    server
        .fn_handler::<anyhow::Error, _>("/save", Method::Post, move |mut req| {
            let mut body = vec![0u8; 1024];
            let n = req.read(&mut body)?;
            let raw = std::str::from_utf8(&body[..n]).unwrap_or("");
            log::info!("wifi: /save body ({} bytes)", n);
            let (ssid, pass) = parse_form(raw)?;
            save_creds(&part_for_post, &Creds { ssid, pass })?;
            saved_for_post.store(true, Ordering::Relaxed);
            let mut resp = req.into_ok_response()?;
            resp.write_all(SAVED_HTML.as_bytes())?;
            Ok(())
        })
        .expect("post /save");

    // Step 3: server is live, QR + URL on-screen, awaiting form POST.
    on_step(ProvisionStep::AwaitForm);

    // Block until the POST handler flags success, then reboot.
    while !saved.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
    on_step(ProvisionStep::Saving);
    // Give the response time to flush over the AP link before the radio dies.
    std::thread::sleep(Duration::from_millis(1500));
    unsafe { esp_idf_svc::sys::esp_restart() };
    #[allow(unreachable_code)]
    loop {}
}

const SAVED_HTML: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<title>Saved</title>
<style>body{font-family:-apple-system,sans-serif;max-width:380px;margin:3em auto;padding:1em;background:#0a0e18;color:#e8eaf0;text-align:center}
h1{color:#4ade80}</style></head>
<body><h1>✓ Saved</h1><p>The T-Embed will reboot and join your network.</p>
<p style="color:#8c94a8;font-size:0.85em">You can close this tab.</p></body></html>"#;

fn build_setup_html(aps: &[AccessPointInfo]) -> String {
    let mut options = String::new();
    if aps.is_empty() {
        options.push_str("<option value=\"\">(no networks found — type below)</option>");
    }
    for ap in aps.iter().take(20) {
        options.push_str(&format!(
            "<option value=\"{}\">{} &nbsp;&nbsp; {} dBm</option>",
            html_escape(ap.ssid.as_str()),
            html_escape(ap.ssid.as_str()),
            ap.signal_strength
        ));
    }
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>T-Embed Setup</title>
<style>
*{{box-sizing:border-box}}
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;max-width:380px;margin:2em auto;padding:1em;background:#0a0e18;color:#e8eaf0}}
h1{{color:#ec4899;font-size:1.4em;margin:0 0 0.25em}}
.sub{{color:#8c94a8;font-size:0.85em;margin-bottom:1.5em}}
label{{display:block;margin:1em 0 0.25em;color:#8c94a8;font-size:0.8em;text-transform:uppercase;letter-spacing:0.05em}}
input,select{{width:100%;padding:11px;background:#161c2c;border:1px solid #2c3650;color:#e8eaf0;border-radius:6px;font-size:16px;font-family:inherit}}
input:focus,select:focus{{outline:none;border-color:#ec4899}}
button{{margin-top:1.5em;padding:13px;width:100%;background:#ec4899;color:white;border:none;font-size:16px;border-radius:6px;font-weight:600;cursor:pointer}}
button:hover{{background:#db2777}}
</style></head>
<body>
<h1>T-Embed Setup</h1>
<div class="sub">Pick a network and enter the password.</div>
<form method="post" action="/save">
  <label>Network</label>
  <select name="ssid" required>{options}</select>
  <label>Password</label>
  <!--
    Belt + suspenders to keep password managers and browser autofill out of
    this field — it's a one-shot WiFi key, not a credential anyone should
    save or generate. Different vendors honor different attributes:
      autocomplete="off"           → generic browser hint
      data-form-type="other"       → tells Bitwarden / 1Password this isn't a login
      data-lpignore="true"         → LastPass-specific opt-out
      data-1p-ignore="true"        → 1Password-specific opt-out
      data-bwignore="true"         → Bitwarden-specific opt-out
      spellcheck=false             → no red squiggle on the password
      autocapitalize/autocorrect   → keep mobile keyboards from mangling it
    Also no `placeholder` text (some PMs trigger off "password" patterns).
  -->
  <input
    type="password"
    name="pass"
    autocomplete="off"
    data-form-type="other"
    data-lpignore="true"
    data-1p-ignore="true"
    data-bwignore="true"
    spellcheck="false"
    autocapitalize="off"
    autocorrect="off"
    inputmode="text"
  >
  <button type="submit">Connect</button>
</form>
</body></html>"#,
        options = options
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn parse_form(body: &str) -> anyhow::Result<(String, String)> {
    let mut ssid = String::new();
    let mut pass = String::new();
    for pair in body.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = it.next().unwrap_or("");
        let val = url_decode(it.next().unwrap_or(""));
        match key {
            "ssid" => ssid = val,
            "pass" => pass = val,
            _ => {}
        }
    }
    if ssid.is_empty() {
        anyhow::bail!("missing ssid");
    }
    Ok((ssid, pass))
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(b' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
            } else {
                out.push(b'%');
            }
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}
