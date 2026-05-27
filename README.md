# tembedded

Rust firmware for the **LilyGo T-Embed** (original K211, ESP32-S3) with
HTTPS over-the-air updates from Azure Blob Storage, an encoder-driven
multi-screen UI, remote JPEG image rendering, and an on-device WiFi
captive-portal for first-time setup.

Standing on `esp-idf-svc` 0.52 + `esp-idf-hal` 0.46 + `mipidsi` 0.10 +
`embedded-graphics` 0.8.

---

## What it does

Once flashed and on WiFi, the device cycles through a user-configurable
set of screens via a double-click on the encoder:

| Screen | What |
|---|---|
| **HOME** | Dual world clock — Miami (primary, big) and Seattle (secondary), 12h, blinking colons, animated wave at the bottom. |
| **FORTUNE** | Zoltar-cat oracle. Press the wheel to draw from the seven babes' 1000-entry corpus. |
| **GAME** | Safe-cracking minigame: rotate to align a cursor inside a green sweet-spot, click to lock. Three tumblers, each narrower. |
| **SYSTEM** | Device info — version, WiFi, IP, MAC, uptime, free heap, OTA host — and the **manage URL** (`http://<ip>/`). |
| **STATUS** | Off by default. Periodic GETs of an HTTPS endpoint with pretty-printed, token-colored JSON. |
| **IMAGE** | Off by default. Lazy-loaded remote JPEG with an animated hot-pink/cyan border. |

Bottom strip is always-on: page dots on the left (one per visible
screen), OTA / status text on the right.

**Controls:**
- **Rotate the encoder** → scroll the current screen (or steer the game cursor)
- **Click the encoder** → screen-specific action (start game, consult fortune)
- **Double-click** → next visible screen
- **Hold the encoder ~2 s** → "forget WiFi" — wipes NVS, reboots into the captive-portal AP

**Management UI:** the device hosts a small web app at `http://<device-ip>/`
(also surfaced on the SYSTEM screen as **MANAGE**). Two tabs: **Info**
mirrors what the SYSTEM screen shows, **Plugins** lets you drag-reorder
and show/hide screens. Hit **Apply & Reboot** to commit — the config is
persisted to NVS and the device picks it up on the next boot. See
[Web management UI](#web-management-ui) below.

**OTA:** every boot, the device polls a manifest in the bucket; if the
version differs, it streams the new firmware into the inactive OTA slot,
flips `ota_data`, and reboots. Bootloader rollback is enabled, so a brick
self-heals on the next reset.

---

## Hardware

- **MCU:** ESP32-S3R8 (Xtensa LX7, 8 MB OPI PSRAM, 16 MB flash)
- **Display:** 1.9" ST7789 IPS LCD, 170×320 native, driven landscape (320×170 visible)
- **Input:** rotary encoder + push-button (A=GPIO2, B=GPIO1, button=GPIO0)
- **Connectivity:** native USB-C (USB Serial JTAG console), WiFi, Bluetooth
- **Other peripherals (not currently used in firmware):** INMP441 mic, APA102 RGB LED, IR TX, microSD

---

## Quick start (someone cloning this repo)

There are a few one-time setup steps. If you just want to read the
high-level path: clone → install toolchain → either edit `src/wifi_creds.rs`
or leave it empty → set up an Azure bucket (or any HTTPS static host) →
update three URLs in source → `cargo run --release` over USB. After that,
all subsequent updates go OTA via `./scripts/release.sh`.

### 1. Install the toolchain

```sh
# Install rustup if you don't have it.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install espup — manages the custom Xtensa-LX7 Rust toolchain.
cargo install espup --locked

# Install the Xtensa Rust toolchain pinned to the version this repo expects.
# (We pin the version because espup's default may 404 if esp-rs hasn't shipped
# a matching release for the latest mainline rustc yet.)
espup install --targets esp32s3 -v 1.88.0.0

# Source the env file each shell. Add this to your .zshrc/.bashrc if you want.
. ~/export-esp.sh

# Install the flashing tools — espflash (standalone) is required by cargo run;
# cargo-espflash adds the cargo subcommand; ldproxy is required for esp-idf builds.
cargo install espflash --locked
cargo install cargo-espflash --locked
cargo install ldproxy --locked
```

### 2. Set up the OTA bucket

This repo points at Azure Blob Storage by default, but the device only
needs an HTTPS host that serves two files publicly:

- `manifest.json` (~100 bytes)
- `tembedded-X.Y.Z.bin` (~1.5 MB)

If you have an Azure storage account already, run:

```sh
az login
# Override these if your account/RG aren't named the same as this repo's defaults.
export AZ_STORAGE_ACCOUNT=your-account
export AZ_RESOURCE_GROUP=your-rg
export AZ_CONTAINER=firmware
./scripts/azure_setup.sh
```

That script:
1. Sets `allowBlobPublicAccess=true` on the storage account.
2. Assigns "Storage Blob Data Contributor" on the account to your AAD identity (needs Owner / User Access Administrator).
3. Creates the container with `--public-access blob` — anonymous reads on individual blobs are allowed, listing the container is not.

After it finishes, blobs are reachable at
`https://<account>.blob.core.windows.net/<container>/<blob>`.

Not using Azure? Any public-read HTTPS host works — S3 public bucket,
Cloudflare R2, GitHub Releases, a static-file nginx. Just make sure the
device can do an anonymous GET and the URL is reachable from your network.

### 3. Point the firmware at your bucket

Three URLs in source code currently hardcode the bucket. Update them if
you're using your own:

| File | Constant | What |
|---|---|---|
| `src/ota.rs` | `BASE_URL` | OTA manifest+binary base URL. Must end with `/`. |
| `src/main.rs` | `URL` | The `/healthz` endpoint shown on the Status screen. |
| `src/main.rs` | `IMAGE_URL` | The JPEG the Image screen fetches. |

### 4. Optional: skip captive portal on first boot

`src/wifi_creds.rs` is empty by default. The device will boot into the
captive-portal AP on first run and you'll provision over the air. If
you'd rather have it auto-connect on first boot, fill in your SSID/password
locally **before** building (the file is committed but with empty values — your
edits stay in your working copy):

```rust
pub const SSID: &str = "your-network";
pub const PASS: &str = "your-password";
```

These are migrated into NVS on first successful connect and then never read again.

### 5. First flash (USB)

Plug the board in over USB-C, then:

```sh
. ~/export-esp.sh
cargo run --release
```

This writes bootloader + partition table + app to the board over USB,
starts the serial monitor. First build is **5–10 minutes** (compiles the
entire ESP-IDF C tree); subsequent builds are seconds for Rust-only
changes, ~30 s for full release with incremental cache.

When the device boots:
- If `wifi_creds.rs` is filled, it'll connect and migrate to NVS.
- Otherwise it'll start the **T-Embed-Setup** AP. See the *WiFi provisioning* section below.

Once it's online, it'll do its first OTA manifest check and, if your bucket has
a newer version, OTA itself. To seed the bucket initially, run
`./scripts/release.sh <version>` from this same source tree.

---

## WiFi provisioning (captive portal)

When NVS has no credentials and `wifi_creds.rs` is empty, the device boots
into a 3-stage setup flow:

1. **Overview screen** — text-only summary of what's about to happen. *Press the middle (encoder) button to advance.* This step never auto-advances — you control the pace.
2. **STEP 1 / 2** — "Join `T-Embed-Setup`" with a pulsing-bordered card showing the SSID. It's an **open** network (no password). The device polls `esp_wifi_ap_get_sta_list()` and auto-advances the moment your phone associates.
3. **STEP 2 / 2** — QR code + URL (`http://192.168.71.1/`). Scan or type the URL. The form scans the air for nearby WiFi networks and offers them in a dropdown. Submit your SSID + password.
4. **Saving** — device writes creds to NVS, reboots, joins your network. Total elapsed time: about 30 seconds.

The web form aggressively opts out of browser password managers
(1Password / LastPass / Bitwarden) so it doesn't get suggested as a saved
credential — it's a one-shot WiFi key, not a login.

**Forget WiFi:** hold the encoder for ~2 seconds at any time. The LCD will
say "Forgetting WiFi — reconnect on AP" and reboot into setup.

---

## Web management UI

Once the device is on your WiFi it serves a small SPA at `http://<device-ip>/`.
The exact URL is also shown on the SYSTEM screen as **MANAGE**.

The page is a single HTML/CSS/JS bundle compiled into the firmware
(`assets/manage.html`, embedded via `include_str!`), served by Rust on
ESP32-S3 via the esp-idf-svc HTTP server. No external dependencies — no
React, no CDN, no auth. Local-network only.

### Tabs

| Tab | Contents |
|---|---|
| **Info** | Model, chip, firmware, WiFi SSID, IP, MAC, OTA host, healthz/image URLs, uptime, free heap, OTA status. Auto-refreshes every 3 s. |
| **Plugins** | All six screens (Home, Fortune, Game, System, Image, Status) with toggles for visibility and drag-handles for reordering. **Apply & Reboot** writes the new config to NVS and restarts the device; the browser auto-reloads when the device comes back. |

### API

The page is just a client of three Rust handlers — they're also usable
by anything else on the LAN that speaks JSON:

| Method | Path | Body | Response |
|---|---|---|---|
| `GET` | `/` | — | the SPA HTML |
| `GET` | `/api/info` | — | `{model, chip, version, ssid, ip, mac, uptime_seconds, free_heap_kib, ota_state, ota_progress_percent, ota_target_version, ota_host, healthz, image_url}` |
| `GET` | `/api/plugins` | — | `{plugins: [{id, name, description, visible}, ...]}` — order = on-device nav order |
| `POST` | `/api/plugins` | `{plugins: [{id, visible}, ...]}` | `{ok: true}` then reboots after ~1.2 s |

Server-side validation refuses payloads that have unknown ids, duplicate
ids, or zero visible entries (the last would soft-brick the nav cycle).

### NVS storage

The plugin config is stored in NVS namespace `cfg`, key `plugins`, as a
compact comma-separated string:

```
home:1,fortune:1,game:1,system:1,image:0,status:0
```

A freshly-flashed device has no `cfg` key yet, so it falls back to the
defaults in `plugins::REGISTRY` (the same set listed in the table above).
Adding a new plugin to the registry will append it to existing users'
configs with its `default_visible` value on next boot — they keep their
ordering, the new entry shows up at the end.

---

## OTA architecture

```
       ┌──────────────────────────────────────┐
       │  Azure Blob: <account>/<container>/  │
       │    manifest.json                     │
       │    tembedded-1.3.0.bin               │
       │    tembedded-1.4.0.bin   ← latest    │
       │    image.jpg                         │
       └────────────────┬─────────────────────┘
                        │ HTTPS GET (anonymous)
                        ▼
   ┌─────────────────────────────────────────┐
   │  T-Embed flash (16 MB)                  │
   │   bootloader | parts | nvs | otadata    │
   │   ota_0 (6 MB)         ota_1 (6 MB)     │
   └─────────────────────────────────────────┘
```

Boot sequence:
1. `ota::mark_valid_if_pending()` — commits the currently-running slot so the bootloader's rollback timer stops watching.
2. Load WiFi creds from NVS (or fallback to `wifi_creds.rs`, or enter captive portal).
3. `ota::fetch_manifest()` — GET `<BASE_URL>/manifest.json`.
4. If `manifest.version != CURRENT_VERSION`, stream the .bin into the inactive OTA slot via `EspOta::initiate_update()`, flip `ota_data`, `esp_restart()`.
5. New slot boots; if it crashes before step 1 runs, the bootloader rolls back to the previous slot on the next reset (`CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE=y`).

The version comparison is plain string equality — push a manifest with the
older version string to roll back manually if you need to.

### Manifest format

```json
{
  "version": "1.4.0",
  "url": "https://your-account.blob.core.windows.net/firmware/tembedded-1.4.0.bin"
}
```

Both fields required. `url` must be absolute; the device does not resolve relative paths.

---

## Cutting a release

```sh
# 1. Bump version in *both* places — release.sh refuses to publish if they disagree.
#    src/ota.rs    → pub const CURRENT_VERSION: &str = "1.5.0";
#    sdkconfig.defaults → CONFIG_APP_PROJECT_VER="1.5.0"

# 2. Build + publish.
. ~/export-esp.sh
./scripts/release.sh 1.5.0
```

What it does:
1. Verifies the two version strings in source match the CLI arg.
2. `cargo build --release` (incremental — subsequent small edits compile in 15–25 s).
3. `espflash save-image` converts the ELF to a flashable .bin.
4. Generates `manifest.json` pointing at the new binary.
5. Uploads both files **in parallel** with `az storage blob upload --auth-mode login`. Manifest is marked `Cache-Control: no-cache, no-store, must-revalidate` so devices don't get a stale version.
6. Prints the public URLs.

Power-cycle any device on an older version; it picks up the new manifest on next boot.

---

## Using the image screen

Upload any JPEG to your bucket at the path the firmware expects:

```sh
az storage blob upload \
  --account-name "$AZ_STORAGE_ACCOUNT" \
  --container-name "$AZ_CONTAINER" \
  --name image.jpg \
  --file path/to/your.jpg \
  --overwrite --auth-mode login
```

Constraints:
- **Format:** JPEG only (baseline, RGB or grayscale). PNG/WebP won't decode.
- **Size on disk:** ≤ 2 MiB (hard cap in `image.rs`).
- **Dimensions:** anything works. Images larger than the 320×170 display
  are scaled down with aspect ratio preserved; smaller images render at
  native size with the animated border around them.

Click the encoder to switch to the Image tab — the device fetches and
decodes the JPEG on first entry (5–10 s for a typical image), caches it in
memory, and re-renders the animated border at ~20 fps.

If the fetch or decode fails, you get an on-screen error card. Cycle to
another screen and back to retry.

---

## Repository layout

```
.
├── Cargo.toml              ← Rust deps + release profile (incremental on)
├── build.rs                ← embuild plumbing for esp-idf-sys
├── partitions.csv          ← Dual-OTA partition table (2 × 6 MiB slots)
├── rust-toolchain.toml     ← Pins to the `esp` toolchain
├── sdkconfig.defaults      ← ESP-IDF config (16 MB flash, PSRAM, cert bundle,
│                              custom partitions, rollback enabled, 32 KiB main stack)
├── .cargo/config.toml      ← `cargo run` flashes via espflash; xtensa target
├── scripts/
│   ├── azure_setup.sh      ← One-time Azure bucket setup (env-var overridable)
│   └── release.sh          ← Build + publish a new OTA version
├── assets/
│   └── manage.html        ← SPA for the on-device manager (embedded via include_str!)
└── src/
    ├── main.rs            ← App state, screen routing, render(), event loop
    ├── ota.rs             ← Manifest fetch, OTA write loop, slot commit/restart
    ├── wifi.rs            ← NVS creds, STA connect, 3-step captive provisioning
    ├── wifi_creds.rs      ← Optional compile-time SSID/password (empty by default)
    ├── image.rs           ← Remote JPEG fetch + decode + animated render
    ├── home.rs            ← Dual-clock Home screen
    ├── fortune.rs         ← Zoltar-cat oracle + 1000-entry corpus
    ├── game.rs            ← Safe-cracking minigame
    ├── plugins.rs         ← Plugin registry + NVS-backed visibility/order config
    └── webconfig.rs       ← Web manager HTTP server (port 80, JSON API + HTML)
```

Modules `wifi.rs` and `image.rs` are intentionally generic enough that you
can drop them into another firmware project — pass them an HTTPS URL and a
framebuffer, they handle the rest. `plugins.rs` + `webconfig.rs` are
similarly self-contained — point at any NVS partition and they'll wire up
a configurable, user-editable nav cycle for any screen-based UI.

---

## Configuration touchpoints

If you're forking this for your own setup, you almost certainly want to
edit at least these:

| File | What | Default |
|---|---|---|
| `src/ota.rs` `BASE_URL` | OTA bucket base URL | `https://binsbucket.blob.core.windows.net/firmware/` |
| `src/main.rs` `URL` | Status-screen health endpoint | `https://api.oliecrypto.com/healthz` |
| `src/main.rs` `IMAGE_URL` | Image-screen JPEG URL | `https://binsbucket.blob.core.windows.net/firmware/image.jpg` |
| `scripts/azure_setup.sh` `AZ_*` vars | Account / RG / container | `binsbucket` / `claudisplay` / `firmware` |
| `scripts/release.sh` `AZ_*` vars | Same defaults | matches setup script |

For Azure, the script defaults are overridable via env vars — no source edits required:

```sh
export AZ_STORAGE_ACCOUNT=myaccount
export AZ_RESOURCE_GROUP=myrg
export AZ_CONTAINER=firmware
./scripts/azure_setup.sh
./scripts/release.sh 1.5.0
```

---

## Known-good display config

The ST7789 panel needs specific init values to come up correctly on this board:

- `display_size(170, 320)`, `display_offset(35, 0)`
- `Rotation::Deg90` for landscape (320×170 visible)
- `ColorOrder::Bgr`, `ColorInversion::Inverted`
- **Must drive `GPIO46` (PWR_ON) HIGH before init**, then `GPIO15` (BL) HIGH for the backlight. Without these the panel stays dark.
- SPI2 @ 40 MHz over GPIO12 (MOSI) / GPIO11 (SCLK) / GPIO10 (CS) / GPIO13 (DC) / GPIO9 (RST)

The framebuffer is a `Vec<Rgb565>` (~110 KiB) backed by the heap allocator,
which transparently lands in PSRAM — saves SRAM for stacks and TLS buffers.

---

## Troubleshooting

| Symptom | Most likely cause | Fix |
|---|---|---|
| First `cargo build` fails with "Partition table CSV file ... not found" | `CONFIG_PARTITION_TABLE_CUSTOM_FILENAME` in `sdkconfig.defaults` uses an absolute path tied to one machine. | Edit that line to your repo's absolute path. (esp-idf-sys's CMake `PROJECT_DIR` is its synthesized build dir, not the repo root, so relative paths don't work.) |
| `espup install` hits HTTP 404 | Mainline rustc shipped a version the esp-rs project hasn't released for yet. | Pin the version: `espup install --targets esp32s3 -v 1.88.0.0` (or the latest known release). |
| `cargo run` errors with "Failed to initialize input reader" | The runner uses `--non-interactive` because Claude/CI has no TTY. | Run interactively from your terminal, or just use `cargo build --release` then `espflash flash --monitor target/.../release/tembedded`. |
| Device boots, gets to "OTA: check failed" in red | Bucket isn't public-read OR doesn't have `manifest.json` yet. | Run `./scripts/azure_setup.sh`, then `./scripts/release.sh <version>` once to seed the bucket. |
| Captive portal page never opens automatically on phone | DNS hijack isn't implemented (yet). | Manually browse to `http://192.168.71.1/` after joining `T-Embed-Setup`. |
| Image screen reboots the device | Stack overflow during JPEG decode (fixed in v0.11.0+) OR your image isn't actually JPEG. | Confirm `CONFIG_ESP_MAIN_TASK_STACK_SIZE=32768` in `sdkconfig.defaults`. Check the file with `file path/to/image.jpg`. |
| OTA "downgrades" device to an older version | Version compare is `!=`, not `>`. If the bucket has an older version than what's running, the device pulls it. | Either always push monotonically increasing versions, or fix the comparison in `ota.rs` to be semver-aware. |
| Build takes 5+ minutes every time | ESP-IDF C tree is being recompiled because `sdkconfig.defaults` or `partitions.csv` changed. | Only the first build does this; subsequent code-only edits are seconds. Don't touch those files in a tight inner loop. |

---

## What works (status)

- Boot over native USB-C, console via USB Serial JTAG
- OPI PSRAM detected — ~8.7 MB free heap at boot
- LCD: ST7789 over SPI2 @ 40 MHz
- WiFi STA + AP modes, switch between them at runtime
- HTTPS with Mozilla cert bundle (covers Azure's DigiCert / Microsoft IT roots)
- OTA: HTTPS pull, dual-slot writes, bootloader rollback
- NVS-backed credentials with 3-step captive-portal fallback
- Rotary encoder: polled at 200 Hz, debounced, scroll + click + long-press gestures
- Multi-screen UI (Status, Image, System) with per-screen scroll
- Remote JPEG image rendering (max 2 MiB, aspect-preserving downscale, animated border)
- QR-code rendering (Project Nayuki port)

---

## Not yet done

- APA102 RGB LED on data=GPIO42 / clock=GPIO45 (`smart-leds` + RMT)
- Background OTA polling (currently only checks at boot)
- SHA-256 verification of downloaded firmware against a manifest field
- DNS hijack in the setup AP for true captive-portal popup behavior
- Semver-aware OTA version comparison (currently `!=`, which can downgrade)
- Compressed firmware images to speed up downloads (~50% smaller)
- INMP441 mic, IR TX, microSD bringup
