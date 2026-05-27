//! OTA over HTTPS from an Azure Blob Storage container.
//!
//! Flow per boot:
//!   1. `mark_valid_if_pending()` — cancels the bootloader rollback the very
//!      first time a freshly-installed app reaches a known-healthy state.
//!   2. `fetch_manifest()` — GET <BASE_URL>/manifest.json.
//!   3. If the remote version differs from `CURRENT_VERSION`,
//!      `apply_update(&m.url, on_progress)` streams the .bin into the
//!      inactive OTA slot.
//!   4. `restart()` — bootloader brings up the new slot.
//!   5. If the new app never re-runs `mark_valid_if_pending()`, the
//!      bootloader rolls back on the next reset (CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use embedded_svc::{http::client::Client, io::Write};
use esp_idf_svc::{
    http::client::{Configuration as HttpConfig, EspHttpConnection, FollowRedirectsPolicy},
    ota::EspOta,
};
use serde::Deserialize;

/// Bump in lockstep with `CONFIG_APP_PROJECT_VER` in sdkconfig.defaults.
/// They don't *have* to match — the comparison below is purely string equality
/// against the manifest — but keeping them in sync avoids confusion.
pub const CURRENT_VERSION: &str = "1.17.0";

/// Base of the public Azure Blob container. Must end with a trailing slash.
/// Account: `binsbucket` (resource group `claudisplay`).
/// Container `firmware` must have `--public-access blob` set; see README.
pub const BASE_URL: &str = "https://binsbucket.blob.core.windows.net/firmware/";

const MANIFEST_PATH: &str = "manifest.json";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// Absolute URL of the firmware binary. Manifest publishers should write
    /// the full URL (not just a filename) so we don't need to resolve paths
    /// on-device.
    pub url: String,
}

/// On the very first boot after an OTA, the bootloader has loaded the new
/// slot in `PENDING_VERIFY` state. Calling this cancels the rollback timer
/// and commits the slot as the new permanent active app.
///
/// Safe to call every boot — it's a no-op when the running slot is already
/// marked valid, and also a no-op when there's no OTA setup (single-app build).
pub fn mark_valid_if_pending() {
    match EspOta::new() {
        Ok(mut ota) => {
            if let Err(e) = ota.mark_running_slot_valid() {
                log::warn!("mark_running_slot_valid: {e:?}");
            } else {
                log::info!("running slot marked valid");
            }
        }
        Err(e) => log::warn!("EspOta::new: {e:?}"),
    }
}

pub fn fetch_manifest() -> anyhow::Result<Manifest> {
    let url = format!("{BASE_URL}{MANIFEST_PATH}");
    log::info!("OTA: GET {url}");

    let conn = new_http_connection()?;
    let mut client = Client::wrap(conn);
    let request = client.get(&url)?;
    let mut response = request.submit()?;
    let status = response.status();
    if status != 200 {
        anyhow::bail!("manifest HTTP {status}");
    }

    let mut body = Vec::with_capacity(512);
    let mut buf = [0u8; 512];
    loop {
        let n = response.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
        if body.len() > 64 * 1024 {
            anyhow::bail!("manifest larger than 64 KiB; refusing");
        }
    }

    let manifest: Manifest = serde_json::from_slice(&body)
        .map_err(|e| anyhow::anyhow!("manifest parse: {e}; body={:?}", String::from_utf8_lossy(&body)))?;
    log::info!("OTA: manifest version={}", manifest.version);
    Ok(manifest)
}

pub fn is_update_available(manifest: &Manifest) -> bool {
    is_newer(manifest.version.trim(), CURRENT_VERSION)
}

/// Semver-aware "greater than" — splits each version into up to three
/// dot-separated u32 components and compares them lexicographically. A
/// missing component is treated as 0 (so "1.2" == "1.2.0"). Non-numeric
/// parts parse as 0, which is fine for our simple X.Y.Z scheme.
///
/// This replaces an earlier `!=` check that would cheerfully OTA-downgrade
/// the device whenever the bucket lagged behind a USB-flashed build.
fn is_newer(remote: &str, local: &str) -> bool {
    parse_semver(remote) > parse_semver(local)
}

fn parse_semver(v: &str) -> (u32, u32, u32) {
    let mut it = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let a = it.next().unwrap_or(0);
    let b = it.next().unwrap_or(0);
    let c = it.next().unwrap_or(0);
    (a, b, c)
}

/// Download `url` into the inactive OTA slot. `progress` is called after each
/// chunk so callers can drive a UI. On success the slot is marked bootable and
/// `ota_data` is flipped to point at it; caller should call `restart()`.
pub fn apply_update<F: FnMut(usize, Option<usize>)>(url: &str, mut progress: F) -> anyhow::Result<()> {
    log::info!("OTA: GET {url}");

    let conn = new_http_connection()?;
    let mut client = Client::wrap(conn);
    let request = client.get(url)?;
    let mut response = request.submit()?;
    let status = response.status();
    if status != 200 {
        anyhow::bail!("firmware HTTP {status}");
    }

    let total: Option<usize> = response
        .header("Content-Length")
        .and_then(|s| s.parse::<usize>().ok());
    log::info!("OTA: firmware size = {:?}", total);

    let mut ota = EspOta::new()?;
    let mut update = ota.initiate_update()?;

    let mut buf = vec![0u8; 4096];
    let mut written: usize = 0;
    progress(0, total);

    let outcome: anyhow::Result<()> = (|| {
        loop {
            let n = response
                .read(&mut buf)
                .map_err(|e| anyhow::anyhow!("socket read: {e:?}"))?;
            if n == 0 {
                break;
            }
            update
                .write_all(&buf[..n])
                .map_err(|e| anyhow::anyhow!("flash write: {e}"))?;
            written += n;
            progress(written, total);
        }
        if let Some(t) = total {
            if written != t {
                anyhow::bail!("short read: wrote {written} of {t} bytes");
            }
        }
        Ok(())
    })();

    match outcome {
        Ok(()) => {
            update.complete()?;
            log::info!("OTA: update.complete() ok, {written} bytes written");
            Ok(())
        }
        Err(e) => {
            let _ = update.abort();
            log::error!("OTA: aborted ({e}); rolling back");
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Background OTA polling
// ---------------------------------------------------------------------------
//
// While the device is running we poll the manifest every BG_POLL_INTERVAL
// seconds. When a newer version is found we stream it into the inactive OTA
// slot and finalize via `EspOta::complete()`. The bootloader will pick it
// up on the *next* reboot — we deliberately do NOT call `esp_restart()` so
// the user finishes whatever they were doing first.
//
// All shared state lives in atomics/mutexes so the UI thread can poll without
// blocking. The poller stops once it's queued an update (no point pulling
// more bytes when one is already pending).

const BG_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BgState {
    Idle = 0,
    Checking = 1,
    Downloading = 2,
    Ready = 3,
    Failed = 4,
}

impl BgState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => BgState::Checking,
            2 => BgState::Downloading,
            3 => BgState::Ready,
            4 => BgState::Failed,
            _ => BgState::Idle,
        }
    }
}

static BG_STATE: AtomicU8 = AtomicU8::new(0);
static BG_DOWNLOAD_PCT: AtomicU8 = AtomicU8::new(0);
static BG_TARGET_VERSION: Mutex<String> = Mutex::new(String::new());

pub fn background_state() -> BgState {
    BgState::from_u8(BG_STATE.load(Ordering::Relaxed))
}

pub fn background_download_pct() -> u8 {
    BG_DOWNLOAD_PCT.load(Ordering::Relaxed)
}

pub fn background_target_version() -> String {
    BG_TARGET_VERSION
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
}

fn set_bg_state(s: BgState) {
    BG_STATE.store(s as u8, Ordering::Relaxed);
}

/// Spawns a daemon thread that polls `manifest.json` every 30 s. Caller is
/// expected to have already finished the boot-time OTA check + WiFi STA up
/// before calling this; the poller assumes the network is usable.
pub fn spawn_background_poller() {
    std::thread::Builder::new()
        .stack_size(16 * 1024)
        .name("ota-poll".into())
        .spawn(|| {
            // Give the rest of boot a moment to settle (display init +
            // initial render) before hitting the network. Shortened in
            // v1.16.0 from 15 s to 3 s now that boot doesn't run a
            // synchronous OTA check first.
            std::thread::sleep(Duration::from_secs(3));

            loop {
                set_bg_state(BgState::Checking);
                log::info!("OTA bg: checking manifest");
                match fetch_manifest() {
                    Ok(manifest) => {
                        if is_update_available(&manifest) {
                            log::info!(
                                "OTA bg: {} -> {} available, downloading",
                                CURRENT_VERSION,
                                manifest.version
                            );
                            if let Ok(mut tv) = BG_TARGET_VERSION.lock() {
                                *tv = manifest.version.clone();
                            }
                            BG_DOWNLOAD_PCT.store(0, Ordering::Relaxed);
                            set_bg_state(BgState::Downloading);

                            let result = apply_update(&manifest.url, |written, total| {
                                if let Some(t) = total {
                                    if t > 0 {
                                        let pct = ((written as u64 * 100) / t as u64) as u8;
                                        BG_DOWNLOAD_PCT.store(pct, Ordering::Relaxed);
                                    }
                                }
                            });

                            match result {
                                Ok(()) => {
                                    log::info!(
                                        "OTA bg: {} staged, reboot to apply",
                                        manifest.version
                                    );
                                    set_bg_state(BgState::Ready);
                                    // Stop polling — an update is queued.
                                    return;
                                }
                                Err(e) => {
                                    log::warn!("OTA bg: apply failed: {e}");
                                    set_bg_state(BgState::Failed);
                                    // Don't give up — try again next interval.
                                }
                            }
                        } else {
                            set_bg_state(BgState::Idle);
                        }
                    }
                    Err(e) => {
                        log::warn!("OTA bg: manifest fetch failed: {e}");
                        set_bg_state(BgState::Failed);
                    }
                }

                std::thread::sleep(BG_POLL_INTERVAL);
            }
        })
        .expect("spawn ota poller");
}

fn new_http_connection() -> anyhow::Result<EspHttpConnection> {
    let conn = EspHttpConnection::new(&HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        follow_redirects_policy: FollowRedirectsPolicy::FollowAll,
        buffer_size: Some(4096),
        buffer_size_tx: Some(1024),
        timeout: Some(std::time::Duration::from_secs(20)),
        ..Default::default()
    })?;
    Ok(conn)
}
