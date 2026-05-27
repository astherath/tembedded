//! On-device HTTP management UI.
//!
//! Once WiFi STA is up, main() calls `start()` to bring up a small server
//! on port 80 that hands out:
//!   GET  /              → the SPA in `assets/manage.html`
//!   GET  /api/info      → device info JSON (model, network, runtime, OTA)
//!   GET  /api/plugins   → ordered list of plugins with visibility flags
//!   POST /api/plugins   → save a new ordering + visibility set, then reboot
//!
//! The server handle is held by main() for the lifetime of the device.

use embedded_svc::{
    http::Method,
    io::{Read as _, Write as _},
};
use esp_idf_svc::{
    http::server::{Configuration as HttpServerConfig, EspHttpServer},
    nvs::{EspNvsPartition, NvsDefault},
};

use crate::{ota, plugins};

/// Snapshot of values that don't change at runtime once WiFi is up.
/// Captured at server start so each request doesn't have to re-query the
/// WiFi driver.
#[derive(Clone)]
pub struct DeviceCtx {
    pub ssid: String,
    pub ip: String,
    pub mac: String,
}

const INDEX_HTML: &str = include_str!("../assets/manage.html");

pub fn start(
    ctx: DeviceCtx,
    nvs: EspNvsPartition<NvsDefault>,
) -> anyhow::Result<EspHttpServer<'static>> {
    let mut server = EspHttpServer::new(&HttpServerConfig::default())?;

    // GET /  — single-page UI
    server.fn_handler::<anyhow::Error, _>("/", Method::Get, move |req| {
        let mut resp = req.into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "text/html; charset=utf-8"),
                ("Cache-Control", "no-store"),
            ],
        )?;
        resp.write_all(INDEX_HTML.as_bytes())?;
        Ok(())
    })?;

    // GET /api/info
    let ctx_for_info = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/info", Method::Get, move |req| {
        let body = info_json(&ctx_for_info);
        let mut resp = req.into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "application/json"),
                ("Cache-Control", "no-store"),
            ],
        )?;
        resp.write_all(body.as_bytes())?;
        Ok(())
    })?;

    // GET /api/plugins
    let nvs_for_get = nvs.clone();
    server.fn_handler::<anyhow::Error, _>("/api/plugins", Method::Get, move |req| {
        let body = plugins_get_json(&nvs_for_get);
        let mut resp = req.into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "application/json"),
                ("Cache-Control", "no-store"),
            ],
        )?;
        resp.write_all(body.as_bytes())?;
        Ok(())
    })?;

    // POST /api/plugins
    let nvs_for_post = nvs.clone();
    server.fn_handler::<anyhow::Error, _>("/api/plugins", Method::Post, move |mut req| {
        let mut body = vec![0u8; 4096];
        let n = req.read(&mut body).unwrap_or(0);
        let raw = std::str::from_utf8(&body[..n]).unwrap_or("");
        match apply_plugins_post(&nvs_for_post, raw) {
            Ok(()) => {
                let mut resp = req.into_response(
                    200,
                    Some("OK"),
                    &[("Content-Type", "application/json")],
                )?;
                resp.write_all(br#"{"ok":true}"#)?;
                // Reboot from a side thread so the response has time to
                // flush over the socket. 1.2 s is enough on a local LAN.
                std::thread::Builder::new()
                    .name("reboot".into())
                    .stack_size(2048)
                    .spawn(|| {
                        std::thread::sleep(std::time::Duration::from_millis(1200));
                        log::info!("webconfig: rebooting to apply plugin config");
                        unsafe { esp_idf_svc::sys::esp_restart() };
                    })?;
            }
            Err(e) => {
                let msg = format!(
                    r#"{{"ok":false,"error":{}}}"#,
                    json_string_escape(&e.to_string())
                );
                let mut resp = req.into_response(
                    400,
                    Some("Bad Request"),
                    &[("Content-Type", "application/json")],
                )?;
                resp.write_all(msg.as_bytes())?;
            }
        }
        Ok(())
    })?;

    log::info!("webconfig: server up on :80");
    Ok(server)
}

fn info_json(ctx: &DeviceCtx) -> String {
    let uptime_us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
    let uptime_s = (uptime_us as u64) / 1_000_000;
    let free_heap = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };

    let ota_state = match ota::background_state() {
        ota::BgState::Idle => "idle",
        ota::BgState::Checking => "checking",
        ota::BgState::Downloading => "downloading",
        ota::BgState::Ready => "ready",
        ota::BgState::Failed => "failed",
    };
    let ota_target = ota::background_target_version();
    let ota_pct = ota::background_download_pct();

    let v = serde_json::json!({
        "model": "LilyGo T-Embed K211",
        "chip": "ESP32-S3 (LX7)",
        "version": ota::CURRENT_VERSION,
        "ssid": ctx.ssid,
        "ip": ctx.ip,
        "mac": ctx.mac,
        "uptime_seconds": uptime_s,
        "free_heap_kib": free_heap / 1024,
        "ota_host": "binsbucket.blob.core.windows.net",
        "healthz": crate::URL,
        "image_url": crate::IMAGE_URL,
        "ota_state": ota_state,
        "ota_target_version": ota_target,
        "ota_progress_percent": ota_pct,
    });
    v.to_string()
}

fn plugins_get_json(nvs: &EspNvsPartition<NvsDefault>) -> String {
    let entries = plugins::load(nvs);
    let arr: Vec<_> = entries
        .iter()
        .filter_map(|e| {
            plugins::lookup(&e.id).map(|info| {
                serde_json::json!({
                    "id": info.id,
                    "name": info.name,
                    "description": info.description,
                    "visible": e.visible,
                })
            })
        })
        .collect();
    serde_json::json!({ "plugins": arr }).to_string()
}

fn apply_plugins_post(nvs: &EspNvsPartition<NvsDefault>, raw: &str) -> anyhow::Result<()> {
    #[derive(serde::Deserialize)]
    struct Req {
        plugins: Vec<ReqPlugin>,
    }
    #[derive(serde::Deserialize)]
    struct ReqPlugin {
        id: String,
        visible: bool,
    }

    let req: Req =
        serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("JSON parse: {e}"))?;
    if req.plugins.is_empty() {
        anyhow::bail!("plugins list empty");
    }

    // Validate ids before saving — refuse the whole batch if any is unknown
    // so we never leave a half-applied state in NVS.
    for p in &req.plugins {
        if plugins::lookup(&p.id).is_none() {
            anyhow::bail!("unknown plugin id: {}", p.id);
        }
    }
    // Guard against the soft-brick of an empty nav cycle.
    if !req.plugins.iter().any(|p| p.visible) {
        anyhow::bail!("at least one plugin must be visible");
    }
    // Refuse duplicates so a malformed client doesn't make the list grow
    // unboundedly across saves.
    for (i, a) in req.plugins.iter().enumerate() {
        if req.plugins[..i].iter().any(|b| b.id == a.id) {
            anyhow::bail!("duplicate plugin id: {}", a.id);
        }
    }

    let entries: Vec<plugins::PluginEntry> = req
        .plugins
        .into_iter()
        .map(|p| plugins::PluginEntry {
            id: p.id,
            visible: p.visible,
        })
        .collect();
    plugins::save(nvs, &entries)?;
    Ok(())
}

/// Quote-and-escape a Rust string for safe inclusion as a JSON string
/// literal. We need this for the error path because we're hand-building a
/// JSON envelope around an arbitrary anyhow message.
fn json_string_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
