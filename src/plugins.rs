//! Plugin registry + on-device persistence for the user-configurable nav.
//!
//! A "plugin" is just a screen the user can reach via the encoder. The
//! registry below is the source of truth for every screen the firmware
//! knows how to render — `id`/`name`/`description` are what the web
//! manager surfaces, `screen` is the in-app routing target, and
//! `default_visible` is what a freshly-flashed device shows before the
//! user has touched the web UI.
//!
//! Persistence lives in NVS (namespace `cfg`, key `plugins`) as a simple
//! comma-separated `id:0|1` list. The list is ordered — entry order
//! becomes the on-device nav order on the next boot. The web manager
//! writes this key and then triggers a reboot; the next boot re-reads it
//! in `main` and rebuilds the visible-screen vector.

use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};

use crate::Screen;

const NVS_NS: &str = "cfg";
const KEY_PLUGINS: &str = "plugins";

pub struct PluginInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub screen: Screen,
    pub default_visible: bool,
}

/// Every screen the firmware knows how to render. Order here matches the
/// out-of-the-box on-device nav order. New plugins added at any point in
/// the slice get appended to existing users' configs on next boot — they
/// keep their custom ordering, the new entry shows up at the end.
pub const REGISTRY: &[PluginInfo] = &[
    PluginInfo {
        id: "home",
        name: "Home",
        description: "Dual world clock — Miami (primary) and Seattle (secondary) in 12h format, with blinking colons and an animated wave at the bottom that syncs to SNTP.",
        screen: Screen::Home,
        default_visible: true,
    },
    PluginInfo {
        id: "fortune",
        name: "Fortune",
        description: "Zoltar-cat oracle. Press the wheel to draw a fortune from the seven babes — Eelbert, Axolittle, Snuggy J, Coffeerito, Margarita Pizza, Bussy Brown Bear Jones, and Herbie — picked from a 1000-entry corpus.",
        screen: Screen::Fortune,
        default_visible: true,
    },
    PluginInfo {
        id: "game",
        name: "Safe",
        description: "Safe-cracking minigame. Rotate the wheel to slide a cursor into the green sweet-spot, click to lock the tumbler. Three rounds, each narrower than the last. Score factors elapsed time and miss count.",
        screen: Screen::Game,
        default_visible: true,
    },
    PluginInfo {
        id: "system",
        name: "System",
        description: "Device info — version, chip, WiFi SSID, IP, MAC, uptime, free heap, OTA host — plus the on-device control cheatsheet.",
        screen: Screen::System,
        default_visible: true,
    },
    PluginInfo {
        id: "image",
        name: "Image",
        description: "Lazy-loaded remote JPEG (≤ 2 MiB), aspect-preserving downscale, animated hot-pink/cyan border. Off by default — flip on to expose it in the nav cycle.",
        screen: Screen::Image,
        default_visible: false,
    },
    PluginInfo {
        id: "status",
        name: "Status",
        description: "Periodic GET against an HTTPS endpoint with pretty-printed, token-colored JSON. Off by default — flip on to expose it in the nav cycle.",
        screen: Screen::Status,
        default_visible: false,
    },
];

pub fn lookup(id: &str) -> Option<&'static PluginInfo> {
    REGISTRY.iter().find(|p| p.id == id)
}

/// One row of the on-device nav list, in display order.
#[derive(Clone, Debug)]
pub struct PluginEntry {
    pub id: String,
    pub visible: bool,
}

/// Read the current plugin config from NVS. Returns the default config if
/// NVS is empty, the key is missing, or the stored value is corrupt — that
/// way a freshly-flashed device with no `cfg` namespace behaves identically
/// to the pre-feature firmware.
pub fn load(part: &EspNvsPartition<NvsDefault>) -> Vec<PluginEntry> {
    let nvs = match EspNvs::new(part.clone(), NVS_NS, true) {
        Ok(n) => n,
        Err(e) => {
            log::warn!("plugins: NVS open failed: {e:?}; using defaults");
            return default_config();
        }
    };
    let mut buf = vec![0u8; 512];
    let raw = match nvs.get_str(KEY_PLUGINS, &mut buf) {
        Ok(Some(s)) => s.to_string(),
        Ok(None) => return default_config(),
        Err(e) => {
            log::warn!("plugins: NVS read failed: {e:?}; using defaults");
            return default_config();
        }
    };
    let parsed = parse(&raw);
    if parsed.is_empty() {
        default_config()
    } else {
        parsed
    }
}

pub fn save(part: &EspNvsPartition<NvsDefault>, entries: &[PluginEntry]) -> anyhow::Result<()> {
    let nvs = EspNvs::new(part.clone(), NVS_NS, true)?;
    let s = serialize(entries);
    nvs.set_str(KEY_PLUGINS, &s)?;
    log::info!("plugins: saved config ({} bytes): {s}", s.len());
    Ok(())
}

pub fn default_config() -> Vec<PluginEntry> {
    REGISTRY
        .iter()
        .map(|p| PluginEntry {
            id: p.id.to_string(),
            visible: p.default_visible,
        })
        .collect()
}

fn serialize(entries: &[PluginEntry]) -> String {
    entries
        .iter()
        .map(|e| format!("{}:{}", e.id, if e.visible { 1 } else { 0 }))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse(s: &str) -> Vec<PluginEntry> {
    let mut entries: Vec<PluginEntry> = s
        .split(',')
        .filter_map(|piece| {
            let mut it = piece.splitn(2, ':');
            let id = it.next()?.trim();
            let vis = it.next()?.trim();
            // Drop unknown ids — firmware downgrade is the only way to hit
            // this, and we don't want to crash on it.
            if lookup(id).is_none() {
                return None;
            }
            Some(PluginEntry {
                id: id.to_string(),
                visible: vis == "1",
            })
        })
        .collect();
    // If new plugins were added to REGISTRY after this config was saved,
    // append them at the end with their default visibility so the user
    // doesn't lose their existing ordering. They'll see the new entries
    // in the web UI on next visit.
    for p in REGISTRY {
        if !entries.iter().any(|e| e.id == p.id) {
            entries.push(PluginEntry {
                id: p.id.to_string(),
                visible: p.default_visible,
            });
        }
    }
    entries
}

/// Build the runtime nav list (in display order, hidden screens omitted)
/// from a loaded config.
pub fn visible_screens(entries: &[PluginEntry]) -> Vec<Screen> {
    entries
        .iter()
        .filter(|e| e.visible)
        .filter_map(|e| lookup(&e.id))
        .map(|info| info.screen)
        .collect()
}
