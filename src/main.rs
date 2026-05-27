use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_graphics_framebuf::{FrameBuf, backends::FrameBufferBackend};
use embedded_svc::http::client::Client;
use esp_idf_hal::{
    delay::Ets,
    gpio::{AnyIOPin, PinDriver, Pull},
    peripherals::Peripherals,
    spi::{config::Config as SpiConfig, SpiDeviceDriver, SpiDriverConfig},
    units::FromValueType,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::client::{Configuration as HttpConfig, EspHttpConnection},
    nvs::{EspNvsPartition, NvsDefault},
    wifi::{BlockingWifi, EspWifi},
};
use mipidsi::{
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, ColorOrder, Orientation, Rotation},
    Builder,
};
use u8g2_fonts::{
    fonts,
    types::{FontColor, VerticalPosition},
    FontRenderer,
};

mod birthdays;
mod cal;
mod fasting;
mod fortune;
mod game;
mod home;
mod image;
mod ota;
mod plugins;
mod quotes;
mod webconfig;
mod wifi;
mod worldcup;
// wifi_creds.rs is still present as a one-time migration source: if NVS is
// empty on first boot of this firmware, we try to connect with the baked-in
// creds and then persist them. After that the file is unused.
mod wifi_creds;

pub const URL: &str = "https://api.oliecrypto.com/healthz";
/// Default URL for the image screen. Replace with your own blob path —
/// JPEG, ideally ≤ 320×170 (it'll scale down preserving aspect if bigger).
pub const IMAGE_URL: &str = "https://binsbucket.blob.core.windows.net/firmware/image.jpg";

pub const W: usize = 320;
pub const H: usize = 170;

// --- palette --------------------------------------------------------------
pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb565 {
    Rgb565::new(r >> 3, g >> 2, b >> 3)
}
pub const BG: Rgb565 = rgb(10, 14, 24);
pub const PANEL: Rgb565 = rgb(22, 28, 44);
pub const FG: Rgb565 = rgb(232, 234, 240);
pub const MUTED: Rgb565 = rgb(140, 148, 168);
pub const ACCENT: Rgb565 = rgb(236, 72, 153);
pub const OK: Rgb565 = rgb(74, 222, 128);
pub const ERR: Rgb565 = rgb(248, 113, 113);
pub const WARN: Rgb565 = rgb(250, 204, 21);
// JSON token colors — pretty-printer marks each line with a hint char so
// render can pick a color without needing a real tokenizer.
const JSON_KEY: Rgb565 = rgb(125, 211, 252);   // sky-300
const JSON_STR: Rgb565 = rgb(134, 239, 172);   // green-300
const JSON_NUM: Rgb565 = rgb(251, 191, 36);    // amber-400
const JSON_PUNCT: Rgb565 = rgb(165, 180, 200);

const F_HEADER: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const F_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_profont12_mf>();
const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_HUGE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_logisoso24_tn>();

/// Ben Buxton's half-step encoder state machine. Index by current state's
/// low nibble, then by the new (A<<1)|B pin state.
/// Reference: http://www.buxtronix.net/2011/10/rotary-encoders-done-properly.html
const ENC_R_START: u8 = 0x0;
const ENC_H_CCW_BEGIN: u8 = 0x1;
const ENC_H_CW_BEGIN: u8 = 0x2;
const ENC_H_START_M: u8 = 0x3;
const ENC_H_CW_BEGIN_M: u8 = 0x4;
const ENC_H_CCW_BEGIN_M: u8 = 0x5;
const ENC_DIR_CW: u8 = 0x10;
const ENC_DIR_CCW: u8 = 0x20;

#[rustfmt::skip]
const ENC_TTABLE: [[u8; 4]; 6] = [
    // R_START — both lines high, no rotation in progress.
    [ENC_H_START_M,                ENC_H_CW_BEGIN,    ENC_H_CCW_BEGIN,   ENC_R_START],
    // H_CCW_BEGIN — saw the first CCW transition.
    [ENC_H_START_M | ENC_DIR_CCW,  ENC_R_START,       ENC_H_CCW_BEGIN,   ENC_R_START],
    // H_CW_BEGIN — saw the first CW transition.
    [ENC_H_START_M | ENC_DIR_CW,   ENC_H_CW_BEGIN,    ENC_R_START,       ENC_R_START],
    // H_START_M — half-way detent (both lines low).
    [ENC_H_START_M,                ENC_H_CCW_BEGIN_M, ENC_H_CW_BEGIN_M,  ENC_R_START],
    // H_CW_BEGIN_M — second-half CW transition.
    [ENC_H_START_M,                ENC_H_START_M,     ENC_H_CW_BEGIN_M,  ENC_R_START | ENC_DIR_CW],
    // H_CCW_BEGIN_M — second-half CCW transition.
    [ENC_H_START_M,                ENC_H_CCW_BEGIN_M, ENC_H_START_M,     ENC_R_START | ENC_DIR_CCW],
];

pub struct VecFb(pub Vec<Rgb565>);
impl FrameBufferBackend for VecFb {
    type Color = Rgb565;
    fn set(&mut self, i: usize, c: Rgb565) { self.0[i] = c; }
    fn get(&self, i: usize) -> Rgb565 { self.0[i] }
    fn nr_elements(&self) -> usize { self.0.len() }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Screen {
    Home = 0,
    Image = 1,
    Game = 2,
    Fortune = 3,
    Status = 4,
    System = 5,
    /// On-device walkthrough for the web manager: how to reach it, the
    /// URL, and a scannable QR. New in v1.15.0.
    Manage = 6,
    /// Scrollable list of upcoming birthdays. v1.17.0.
    Birthdays = 7,
    /// 16/8 intermittent fasting — schedule + manual timer. v1.17.0.
    Fasting = 8,
    /// 100 motivational quotes auto-rotating every 60 s. v1.17.0.
    Quotes = 9,
    /// Next 10 World Cup 2026 matches from a public JSON schedule
    /// with a baked-in fallback. v1.17.0.
    Worldcup = 10,
}
/// Sized for the Screen enum (highest discriminant + 1). Used to size
/// `screen_scroll`; entries for currently-hidden screens are harmless.
const NUM_SCREENS: usize = 11;

/// Manage screen scroll step — each encoder detent moves the body this
/// many pixels. Tuned so a typical full scroll takes about half a wheel
/// revolution.
const MANAGE_SCROLL_STEP_PX: i32 = 8;

/// Compute the next screen in the visible-cycle list. If `current` isn't in
/// the list (e.g. it was just hidden via the web manager and reboot hasn't
/// happened yet), wrap to the first visible screen. Returns `Home` as a
/// last-resort fallback if the list is empty — server validation prevents
/// this in normal operation.
fn next_screen(current: Screen, visible: &[Screen]) -> Screen {
    if visible.is_empty() {
        return Screen::Home;
    }
    let idx = visible.iter().position(|&v| v == current).unwrap_or(0);
    visible[(idx + 1) % visible.len()]
}

/// Position of `s` within the current visible list — for the bottom-strip
/// page dots. Hidden screens fall back to dot 0.
fn visible_index(s: Screen, visible: &[Screen]) -> usize {
    visible.iter().position(|&v| v == s).unwrap_or(0)
}

struct UiState {
    /// Persistent device info — surfaced in the System screen, not in headers.
    wifi_ssid: String,
    ip: String,
    mac: String,
    method: String,
    status: Option<u16>,
    /// One pretty-printed line of the JSON body per element.
    body_lines: Vec<String>,
    /// Scroll offsets, one per screen, indexed by Screen as usize.
    screen_scroll: [usize; NUM_SCREENS],
    current_screen: Screen,
    /// User-configurable nav cycle — rebuilt at boot from the plugin
    /// config in NVS. Always non-empty in normal operation; the web
    /// manager refuses to save an empty list and `plugins::load` falls
    /// back to defaults on a corrupt entry.
    visible_screens: Vec<Screen>,
    /// Free-running tick (5 ms increments) for animation phases.
    tick: u32,
    error: Option<String>,
    ota_line: String,
    ota_progress: Option<f32>,
    ota_color: Rgb565,
    /// When Some, render() takes over with the AP setup screen.
    ap_setup: Option<ApSetupView>,
    /// Image screen state. Always present once main() finishes setup.
    image: Option<image::RemoteImage>,
    /// Safe-cracking minigame state.
    game: game::Game,
    /// Fortune-teller oracle state.
    fortune: fortune::Fortune,
    /// Intermittent-fasting timer state. v1.17.0.
    fasting: fasting::Fasting,
    /// Motivational-quote rotation state. v1.17.0.
    quotes: quotes::Quotes,
}

impl Default for UiState {
    fn default() -> Self {
        // Default to the registry-default cycle so render() works before
        // main() has had a chance to load the real config from NVS.
        let visible_screens = plugins::visible_screens(&plugins::default_config());
        Self {
            wifi_ssid: String::new(),
            ip: String::new(),
            mac: String::new(),
            method: String::new(),
            status: None,
            body_lines: Vec::new(),
            screen_scroll: [0; NUM_SCREENS],
            current_screen: *visible_screens.first().unwrap_or(&Screen::Home),
            visible_screens,
            tick: 0,
            error: None,
            ota_line: String::new(),
            ota_progress: None,
            ota_color: ACCENT,
            ap_setup: None,
            image: None,
            game: game::Game::new(),
            fortune: fortune::Fortune::new(),
            fasting: fasting::Fasting::new(),
            quotes: quotes::Quotes::new(),
        }
    }
}

struct ApSetupView {
    /// Side length in QR modules.
    qr_size: usize,
    /// Row-major dark/light bits, length = qr_size * qr_size.
    qr_modules: Vec<bool>,
    url: String,
    ap_ssid: String,
    /// Current stage in the 3-step provisioning flow. Updated by the
    /// `on_step` callback passed to `wifi::provision`.
    step: wifi::ProvisionStep,
}

// --- layout constants -----------------------------------------------------
// v1.0.0: no persistent header. Each screen owns its own slim title strip
// inside the body. Bottom strip carries the page-dot indicator + OTA status.
const STRIP_H: i32 = 16;
const STRIP_TOP: i32 = H as i32 - STRIP_H; // 154
const BODY_TOP: i32 = 0;
const BODY_BOTTOM: i32 = STRIP_TOP - 1; // 153
const BODY_LINE_H: i32 = 11;
pub const BODY_LEFT: i32 = 10;

/// Slim title bar each screen draws inside its own body, leaving the rest
/// of the body for scrollable content.
const TITLE_H: i32 = 22;

fn status_body_visible_lines() -> usize {
    // Status screen reserves a 30-px region for the endpoint title + HTTP
    // status badge above the scrollable JSON body.
    let scroll_top = BODY_TOP + 38;
    ((BODY_BOTTOM - scroll_top) / BODY_LINE_H) as usize
}

fn system_body_visible_lines() -> usize {
    ((BODY_BOTTOM - (BODY_TOP + TITLE_H + 4)) / BODY_LINE_H) as usize
}

fn image_body_rect() -> Rectangle {
    Rectangle::new(
        Point::new(0, BODY_TOP),
        Size::new(W as u32, (BODY_BOTTOM - BODY_TOP) as u32),
    )
}

fn render(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState) {
    fb.clear(BG).unwrap();

    // AP-mode setup screen takes over the whole middle section.
    if let Some(view) = &s.ap_setup {
        render_ap_setup(fb, s, view);
        return;
    }

    // Each screen owns the body region in full.
    match s.current_screen {
        Screen::Home => home::render(fb, BODY_TOP, BODY_BOTTOM, s.tick),
        Screen::Image => render_image_body(fb, s),
        Screen::Game => game::render(fb, &s.game, BODY_TOP, BODY_BOTTOM),
        Screen::Fortune => fortune::render(fb, &s.fortune, BODY_TOP, BODY_BOTTOM, s.tick),
        Screen::Status => render_status_body(fb, s),
        Screen::System => render_system_body(fb, s),
        Screen::Manage => render_manage_body(fb, s),
        Screen::Birthdays => birthdays::render(
            fb,
            s.screen_scroll[Screen::Birthdays as usize],
            BODY_TOP,
            BODY_BOTTOM,
        ),
        Screen::Fasting => fasting::render(fb, &s.fasting, BODY_TOP, BODY_BOTTOM, s.tick),
        Screen::Quotes => quotes::render(fb, &s.quotes, BODY_TOP, BODY_BOTTOM, s.tick),
        Screen::Worldcup => worldcup::render(
            fb,
            s.screen_scroll[Screen::Worldcup as usize],
            BODY_TOP,
            BODY_BOTTOM,
        ),
    }

    draw_bottom_strip(fb, s);
}

/// Render a screen-title strip at the top of the body. Slim — leaves most of
/// the body for the actual content.
fn draw_screen_title(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    label: &str,
    accent_text: Option<&str>,
) {
    let _ = F_MICRO.render(
        label,
        Point::new(BODY_LEFT, BODY_TOP + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    if let Some(t) = accent_text {
        let _ = F_MICRO.render(
            t,
            Point::new(W as i32 - 90, BODY_TOP + 5),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
    }
    // Underline
    Rectangle::new(
        Point::new(BODY_LEFT, BODY_TOP + TITLE_H - 4),
        Size::new((W as i32 - BODY_LEFT * 2) as u32, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(rgb(60, 70, 90)))
    .draw(fb)
    .unwrap();
}

// ----- Status screen ------------------------------------------------------

fn render_status_body(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState) {
    // Slim header: endpoint name + big response code on the right.
    let _ = F_MICRO.render(
        "ENDPOINT",
        Point::new(BODY_LEFT, BODY_TOP + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let _ = F_HEADER.render(
        s.method.as_str(),
        Point::new(BODY_LEFT, BODY_TOP + 14),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
    if let Some(code) = s.status {
        let color = if (200..400).contains(&code) { OK } else { ERR };
        let text = format!("{code}");
        let _ = F_HUGE.render(
            text.as_str(),
            Point::new(W as i32 - 70, BODY_TOP + 2),
            VerticalPosition::Top,
            FontColor::Transparent(color),
            fb,
        );
    }

    let scroll_top = BODY_TOP + 38;
    Rectangle::new(
        Point::new(BODY_LEFT, scroll_top - 4),
        Size::new((W as i32 - BODY_LEFT * 2) as u32, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(rgb(60, 70, 90)))
    .draw(fb)
    .unwrap();

    // Scrollable JSON body
    let total = s.body_lines.len();
    let cap = status_body_visible_lines();
    let scroll = s.screen_scroll[Screen::Status as usize];
    let start = scroll.min(total.saturating_sub(1));
    let end = (start + cap).min(total);

    if total == 0 {
        // Async fetch hasn't landed yet (or the screen was just opened
        // for the first time on a brand-new device). Spinner-style
        // placeholder so it doesn't look frozen.
        let phase = ((s.tick / 40) % 4) as usize;
        let dots: &str = match phase {
            0 => "",
            1 => ".",
            2 => "..",
            _ => "...",
        };
        let msg = format!("fetching{dots}");
        let _ = F_BODY.render(
            msg.as_str(),
            Point::new(BODY_LEFT + 4, scroll_top + BODY_LINE_H),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
    } else {
        for (i, line) in s.body_lines[start..end].iter().enumerate() {
            let y = scroll_top + (i as i32) * BODY_LINE_H;
            let color = line_color(line);
            let _ = F_BODY.render(
                line.as_str(),
                Point::new(BODY_LEFT + 4, y),
                VerticalPosition::Top,
                FontColor::Transparent(color),
                fb,
            );
        }
        draw_scrollbar(fb, scroll_top, BODY_BOTTOM, start, end, total, cap);
    }
}

// ----- Image screen -------------------------------------------------------

fn render_image_body(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState) {
    if let Some(img) = &s.image {
        img.draw_in(fb, image_body_rect(), s.tick);
    }
}

// ----- System screen ------------------------------------------------------

fn system_rows(s: &UiState) -> Vec<(String, String)> {
    let uptime_us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
    let uptime_s = (uptime_us as u64) / 1_000_000;
    let h = uptime_s / 3600;
    let m = (uptime_s % 3600) / 60;
    let sec = uptime_s % 60;
    let uptime = if h > 0 {
        format!("{h}h {m}m {sec}s")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    };

    let free_heap = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };

    let manage_url = if s.ip.is_empty() {
        "(offline)".into()
    } else {
        format!("http://{}/", s.ip)
    };

    vec![
        ("VERSION".into(), format!("v{}", ota::CURRENT_VERSION)),
        ("CHIP".into(), "ESP32-S3 (LX7)".into()),
        ("WIFI".into(), s.wifi_ssid.clone()),
        ("IP".into(), s.ip.clone()),
        ("MAC".into(), s.mac.clone()),
        ("UPTIME".into(), uptime),
        ("FREE HEAP".into(), format!("{} KiB", free_heap / 1024)),
        ("MANAGE".into(), manage_url),
        ("OTA HOST".into(), "binsbucket.blob".into()),
        ("HEALTHZ".into(), "api.oliecrypto.com".into()),
        ("IMAGE URL".into(), "binsbucket/firmware/image.jpg".into()),
        ("─".into(), "─────────────────".into()),
        ("CONTROLS".into(), String::new()),
        ("ROTATE".into(), "scroll (or game cursor)".into()),
        ("DOUBLE CLICK".into(), "next screen".into()),
        ("CLICK (game)".into(), "lock the tumbler".into()),
        ("HOLD 2s".into(), "forget WiFi".into()),
    ]
}

fn render_system_body(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState) {
    draw_screen_title(fb, "SYSTEM", Some(&format!("v{}", ota::CURRENT_VERSION)));

    let rows = system_rows(s);
    let scroll_top = BODY_TOP + TITLE_H + 4;
    let total = rows.len();
    let cap = system_body_visible_lines();
    let scroll = s.screen_scroll[Screen::System as usize];
    let start = scroll.min(total.saturating_sub(1));
    let end = (start + cap).min(total);

    for (i, (key, val)) in rows[start..end].iter().enumerate() {
        let y = scroll_top + (i as i32) * BODY_LINE_H;
        // Slight visual rhythm — alternate row tint.
        if (i + start) % 2 == 0 {
            Rectangle::new(
                Point::new(BODY_LEFT - 2, y - 1),
                Size::new((W as i32 - BODY_LEFT * 2 + 4 - 6) as u32, BODY_LINE_H as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(rgb(16, 20, 32)))
            .draw(fb)
            .unwrap();
        }
        let key_color = if key == "─" || key == "CONTROLS" { ACCENT } else { MUTED };
        let _ = F_MICRO.render(
            key.as_str(),
            Point::new(BODY_LEFT, y + 1),
            VerticalPosition::Top,
            FontColor::Transparent(key_color),
            fb,
        );
        let _ = F_BODY.render(
            val.as_str(),
            Point::new(BODY_LEFT + 90, y),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
    }

    draw_scrollbar(fb, scroll_top, BODY_BOTTOM, start, end, total, cap);
}

// ----- Manage screen ------------------------------------------------------

/// Total height of the Manage screen content, in pixels — used to compute
/// the scroll range and the scrollbar thumb. Bumped whenever
/// `render_manage_body` adds or removes content rows.
const MANAGE_CONTENT_H: i32 = 280;

fn manage_visible_h() -> i32 {
    BODY_BOTTOM - (BODY_TOP + TITLE_H + 4)
}

/// Maximum scroll value (in MANAGE_SCROLL_STEP_PX units) such that the
/// last pixel of content lines up with the bottom of the body. Clamped to
/// 0 when the content fits without scrolling.
fn manage_max_scroll_steps() -> usize {
    let overflow = (MANAGE_CONTENT_H - manage_visible_h()).max(0);
    (overflow / MANAGE_SCROLL_STEP_PX) as usize
        + if overflow % MANAGE_SCROLL_STEP_PX != 0 { 1 } else { 0 }
}

fn render_manage_body(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState) {
    draw_screen_title(fb, "MANAGE", Some("remote setup"));

    let scroll_units = s.screen_scroll[Screen::Manage as usize];
    let scroll_px = (scroll_units as i32) * MANAGE_SCROLL_STEP_PX;

    let content_top = BODY_TOP + TITLE_H + 4;
    let content_bottom = BODY_BOTTOM;

    // Mask drawing to the body region by tracking each block's y and
    // skipping ones fully above the viewport. Anything that lands across
    // the mask is partially drawn — set_pixel / Rectangle::draw clip to
    // the framebuf bounds, so out-of-body writes are dropped harmlessly.
    let y0 = content_top - scroll_px;

    let waiting = s.ip.is_empty();
    let url = if waiting {
        String::new()
    } else {
        format!("http://{}/", s.ip)
    };

    let mut y = y0;

    // ---- Intro ----------------------------------------------------------
    draw_text_if_visible(
        fb,
        "Configure plugins and view info",
        BODY_LEFT,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H;
    draw_text_if_visible(
        fb,
        "from your phone over the network.",
        BODY_LEFT,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H + 6;

    // ---- Step 01 — same WiFi -------------------------------------------
    draw_step_marker(fb, "01", y, content_top, content_bottom);
    let step_x: i32 = BODY_LEFT + 32;
    draw_text_if_visible(
        fb,
        "Connect your phone/laptop to",
        step_x,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H;
    draw_text_if_visible(
        fb,
        "the same WiFi network as this",
        step_x,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H;
    draw_text_if_visible(
        fb,
        "device:",
        step_x,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H + 3;
    // SSID card — accented background, monospace for the name.
    let ssid_card_x = step_x;
    let ssid_card_y = y;
    let ssid_card_w: i32 = W as i32 - ssid_card_x - 12;
    let ssid_card_h: i32 = 22;
    if intersects(ssid_card_y, ssid_card_h, content_top, content_bottom) {
        Rectangle::new(
            Point::new(ssid_card_x, ssid_card_y),
            Size::new(ssid_card_w as u32, ssid_card_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(fb)
        .unwrap();
        Rectangle::new(
            Point::new(ssid_card_x, ssid_card_y),
            Size::new(2, ssid_card_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();
        let ssid_text = if s.wifi_ssid.is_empty() {
            "(none)".to_string()
        } else {
            s.wifi_ssid.clone()
        };
        let _ = F_HEADER.render(
            ssid_text.as_str(),
            Point::new(ssid_card_x + 8, ssid_card_y + 5),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
    }
    y += ssid_card_h + 10;

    // ---- Step 02 — URL -------------------------------------------------
    draw_step_marker(fb, "02", y, content_top, content_bottom);
    draw_text_if_visible(
        fb,
        "Open this URL in any browser:",
        step_x,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H + 3;
    // URL card
    let url_card_x = step_x;
    let url_card_y = y;
    let url_card_w: i32 = W as i32 - url_card_x - 12;
    let url_card_h: i32 = 22;
    if intersects(url_card_y, url_card_h, content_top, content_bottom) {
        Rectangle::new(
            Point::new(url_card_x, url_card_y),
            Size::new(url_card_w as u32, url_card_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(fb)
        .unwrap();
        Rectangle::new(
            Point::new(url_card_x, url_card_y),
            Size::new(2, url_card_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(if waiting { WARN } else { OK }),
        )
        .draw(fb)
        .unwrap();
        let url_text = if waiting {
            "waiting for WiFi…".to_string()
        } else {
            url.clone()
        };
        let _ = F_HEADER.render(
            url_text.as_str(),
            Point::new(url_card_x + 8, url_card_y + 5),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
    }
    y += url_card_h + 10;

    // ---- Step 03 — scan QR --------------------------------------------
    draw_step_marker(fb, "03", y, content_top, content_bottom);
    draw_text_if_visible(
        fb,
        "Or scan the QR code below:",
        step_x,
        y,
        &F_BODY,
        FG,
        content_top,
        content_bottom,
    );
    y += BODY_LINE_H + 6;

    // ---- QR code ------------------------------------------------------
    let qr_size_px: i32 = 96;
    if waiting {
        // Placeholder card while WiFi is still coming up.
        let card_x = (W as i32 - qr_size_px) / 2;
        if intersects(y, qr_size_px, content_top, content_bottom) {
            Rectangle::new(
                Point::new(card_x, y),
                Size::new(qr_size_px as u32, qr_size_px as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(PANEL))
            .draw(fb)
            .unwrap();
            let _ = F_MICRO.render(
                "waiting for WiFi…",
                Point::new(card_x + 14, y + qr_size_px / 2 - 4),
                VerticalPosition::Top,
                FontColor::Transparent(MUTED),
                fb,
            );
        }
    } else if let Some((qr_modules_n, qr_bits)) = make_qr(&url) {
        if intersects(y, qr_size_px, content_top, content_bottom) {
            // Center the QR horizontally in the body.
            let quiet: u32 = 2;
            let total_modules = qr_modules_n as u32 + quiet * 2;
            let px_per_module = ((qr_size_px as u32) / total_modules).max(1);
            let actual = (total_modules * px_per_module) as i32;
            let origin = Point::new((W as i32 - actual) / 2, y);
            draw_qr_at(fb, origin, qr_modules_n, &qr_bits, px_per_module);
        }
    }
    y += qr_size_px + 8;

    // ---- Footer note --------------------------------------------------
    draw_text_if_visible(
        fb,
        "Local network only — no login.",
        BODY_LEFT,
        y,
        &F_MICRO,
        MUTED,
        content_top,
        content_bottom,
    );

    // ---- Mask off any content that bled above the title strip ---------
    // The title strip lives at BODY_TOP..content_top; scrolled-up content
    // must not paint there. We clear that band after the body draw so the
    // title stays clean.
    Rectangle::new(
        Point::new(0, BODY_TOP),
        Size::new(W as u32, (content_top - BODY_TOP) as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(BG))
    .draw(fb)
    .unwrap();
    // Re-draw the title since we just wiped it.
    draw_screen_title(fb, "MANAGE", Some("remote setup"));

    // ---- Scrollbar ----------------------------------------------------
    let max_steps = manage_max_scroll_steps();
    if max_steps > 0 {
        let track_x = W as i32 - 6;
        let track_top = content_top + 4;
        let track_bot = content_bottom - 4;
        let track_h = (track_bot - track_top).max(1) as u32;
        Rectangle::new(Point::new(track_x, track_top), Size::new(2, track_h))
            .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
            .draw(fb)
            .unwrap();
        let visible_h = manage_visible_h() as f32;
        let thumb_ratio = (visible_h / MANAGE_CONTENT_H as f32).clamp(0.15, 1.0);
        let thumb_h = ((track_h as f32) * thumb_ratio).max(8.0) as u32;
        let progress = (scroll_units as f32 / max_steps as f32).clamp(0.0, 1.0);
        let thumb_y = track_top + ((track_h - thumb_h) as f32 * progress) as i32;
        Rectangle::new(Point::new(track_x, thumb_y), Size::new(2, thumb_h))
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb)
            .unwrap();
        if scroll_units > 0 {
            triangle_up(fb, Point::new(W as i32 - 14, content_top), 5, ACCENT);
        }
        if scroll_units < max_steps {
            triangle_down(fb, Point::new(W as i32 - 14, content_bottom - 6), 5, ACCENT);
        }
    }
}

/// Render a single text line iff it overlaps the visible body region.
/// Saves the cost of rendering glyphs that would clip entirely out.
fn draw_text_if_visible(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    text: &str,
    x: i32,
    y: i32,
    font: &FontRenderer,
    color: Rgb565,
    region_top: i32,
    region_bottom: i32,
) {
    // F_BODY / F_MICRO are 8-12 px tall; pad both sides so partial lines
    // still draw.
    if y + 14 < region_top || y > region_bottom {
        return;
    }
    let _ = font.render(
        text,
        Point::new(x, y),
        VerticalPosition::Top,
        FontColor::Transparent(color),
        fb,
    );
}

fn draw_step_marker(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    label: &str,
    y: i32,
    region_top: i32,
    region_bottom: i32,
) {
    if y + 14 < region_top || y > region_bottom {
        return;
    }
    let _ = F_HEADER.render(
        label,
        Point::new(BODY_LEFT, y - 2),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
}

fn intersects(y: i32, h: i32, region_top: i32, region_bottom: i32) -> bool {
    y + h >= region_top && y <= region_bottom
}

fn draw_qr_at(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    origin: Point,
    qr_size: usize,
    qr_modules: &[bool],
    px_per_module: u32,
) {
    let quiet: u32 = 2;
    let total_modules = qr_size as u32 + quiet * 2;
    let total_px = total_modules * px_per_module;

    Rectangle::new(origin, Size::new(total_px, total_px))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::WHITE))
        .draw(fb)
        .unwrap();

    let qr_origin = Point::new(
        origin.x + (quiet * px_per_module) as i32,
        origin.y + (quiet * px_per_module) as i32,
    );
    for y in 0..qr_size {
        for x in 0..qr_size {
            if qr_modules[y * qr_size + x] {
                let px = Point::new(
                    qr_origin.x + (x as u32 * px_per_module) as i32,
                    qr_origin.y + (y as u32 * px_per_module) as i32,
                );
                Rectangle::new(px, Size::new(px_per_module, px_per_module))
                    .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                    .draw(fb)
                    .unwrap();
            }
        }
    }
}

// ----- Shared widgets -----------------------------------------------------

fn draw_scrollbar(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    top: i32,
    bottom: i32,
    start: usize,
    end: usize,
    total: usize,
    cap: usize,
) {
    let gutter_x = W as i32 - 14;
    if start > 0 {
        triangle_up(fb, Point::new(gutter_x, top), 5, ACCENT);
    }
    if end < total {
        triangle_down(fb, Point::new(gutter_x, bottom - 6), 5, ACCENT);
    }
    if total > cap {
        let track_x = W as i32 - 6;
        let track_top = top + 8;
        let track_bot = bottom - 8;
        let track_h = (track_bot - track_top).max(1) as u32;
        Rectangle::new(Point::new(track_x, track_top), Size::new(2, track_h))
            .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
            .draw(fb)
            .unwrap();
        let thumb_h = ((track_h as f32) * (cap as f32 / total as f32)).max(6.0) as u32;
        let max_scroll = (total - cap).max(1) as f32;
        let progress = (start as f32 / max_scroll).clamp(0.0, 1.0);
        let thumb_y = track_top + ((track_h - thumb_h) as f32 * progress) as i32;
        Rectangle::new(Point::new(track_x, thumb_y), Size::new(2, thumb_h))
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb)
            .unwrap();
    }
}

/// Bottom strip: page dots on the left, OTA status (with optional progress
/// bar) on the right. Replaces both the old tab bar and the old status strip
/// — single, slim, always-on row.
fn draw_bottom_strip(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState) {
    Rectangle::new(
        Point::new(0, STRIP_TOP),
        Size::new(W as u32, STRIP_H as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(PANEL))
    .draw(fb)
    .unwrap();

    // Page dots — one per visible screen so hidden plugins don't show
    // up in the navigation hint. Visible list is rebuilt at boot from
    // the plugin config in NVS, so this reflects the user's current
    // configuration.
    let dot_y = STRIP_TOP + STRIP_H / 2 - 2;
    let dot_x0 = 8;
    let active = visible_index(s.current_screen, &s.visible_screens);
    let dot_count = s.visible_screens.len();
    for i in 0..dot_count {
        let color = if i == active { ACCENT } else { rgb(70, 80, 100) };
        let x = dot_x0 + (i as i32) * 8;
        let size = if i == active { 5 } else { 3 };
        let offset = if i == active { 0 } else { 1 };
        Rectangle::new(Point::new(x, dot_y + offset), Size::new(size, size))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .unwrap();
    }

    // OTA text — start after the dots region.
    let text_x = dot_x0 + (dot_count as i32) * 8 + 6;
    let _ = F_MICRO.render(
        s.ota_line.as_str(),
        Point::new(text_x, STRIP_TOP + 4),
        VerticalPosition::Top,
        FontColor::Transparent(s.ota_color),
        fb,
    );

    if let Some(p) = s.ota_progress {
        let bar_x = W as i32 - 80;
        let bar_w = 70u32;
        let bar_y = STRIP_TOP + 5;
        Rectangle::new(Point::new(bar_x, bar_y), Size::new(bar_w, 6))
            .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
            .draw(fb)
            .unwrap();
        let fill_w = ((bar_w as f32) * p.clamp(0.0, 1.0)) as u32;
        if fill_w > 0 {
            Rectangle::new(Point::new(bar_x, bar_y), Size::new(fill_w, 6))
                .into_styled(PrimitiveStyle::with_fill(s.ota_color))
                .draw(fb)
                .unwrap();
        }
    }
}

fn triangle_up(fb: &mut FrameBuf<Rgb565, VecFb>, base: Point, size: i32, color: Rgb565) {
    for row in 0..size {
        let half = row;
        let w = (half * 2 + 1) as u32;
        let x = base.x - half;
        let y = base.y + (size - row);
        Rectangle::new(Point::new(x, y), Size::new(w, 1))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .unwrap();
    }
}

fn triangle_down(fb: &mut FrameBuf<Rgb565, VecFb>, base: Point, size: i32, color: Rgb565) {
    for row in 0..size {
        let half = size - 1 - row;
        let w = (half * 2 + 1) as u32;
        let x = base.x - half;
        let y = base.y + row;
        Rectangle::new(Point::new(x, y), Size::new(w, 1))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .unwrap();
    }
}

fn render_ap_setup(fb: &mut FrameBuf<Rgb565, VecFb>, s: &UiState, view: &ApSetupView) {
    // Slim header strip — shared across all 3 steps for visual continuity.
    Rectangle::new(Point::new(0, 0), Size::new(W as u32, 22))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(fb)
        .unwrap();
    let title = match view.step {
        wifi::ProvisionStep::Explain => "SETUP — OVERVIEW",
        wifi::ProvisionStep::AwaitJoin => "SETUP — STEP 1 / 2",
        wifi::ProvisionStep::AwaitForm => "SETUP — STEP 2 / 2",
        wifi::ProvisionStep::Saving => "SAVING...",
    };
    let _ = F_MICRO.render(
        title,
        Point::new(8, 4),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let ver_text = format!("v{}", ota::CURRENT_VERSION);
    let _ = F_MICRO.render(
        ver_text.as_str(),
        Point::new(W as i32 - 36, 4),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let sub = match view.step {
        wifi::ProvisionStep::Explain => "no WiFi credentials saved",
        wifi::ProvisionStep::AwaitJoin => "join the device's setup WiFi network",
        wifi::ProvisionStep::AwaitForm => "open the setup page in your browser",
        wifi::ProvisionStep::Saving => "applying...",
    };
    let _ = F_MICRO.render(
        sub,
        Point::new(8, 12),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    match view.step {
        wifi::ProvisionStep::Explain => render_step_explain(fb),
        wifi::ProvisionStep::AwaitJoin => render_step_await_join(fb, view, s.tick),
        wifi::ProvisionStep::AwaitForm => render_step_await_form(fb, view),
        wifi::ProvisionStep::Saving => render_step_saving(fb),
    }

    draw_bottom_strip(fb, s);
}

fn render_step_explain(fb: &mut FrameBuf<Rgb565, VecFb>) {
    let mut y: i32 = 28;
    let _ = F_HEADER.render(
        "Let's get you online.",
        Point::new(16, y),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
    y += 20;

    // (label, body text). Body text gets word-wrapped to fit the column to
    // the right of the label — no single string in this block is ever
    // allowed to leave the screen.
    let steps: [(&str, &str); 2] = [
        (
            "01",
            "Connect your phone to the device's setup WiFi network.",
        ),
        (
            "02",
            "Scan the QR code, pick your real WiFi network, and enter the password.",
        ),
    ];
    let text_x: i32 = 44;
    // Max width for the right column. F_BODY is profont12, monospace 6 px wide,
    // so 38 chars × 6 px ≈ 228 px fits inside W-text_x-margin.
    let text_max_chars: usize = 38;
    for (num, text) in steps {
        let _ = F_HEADER.render(
            num,
            Point::new(16, y),
            VerticalPosition::Top,
            FontColor::Transparent(ACCENT),
            fb,
        );
        let wrapped = wrap_text(text, text_max_chars);
        let block_top = y + 2;
        for (i, line) in wrapped.iter().enumerate() {
            let _ = F_BODY.render(
                line.as_str(),
                Point::new(text_x, block_top + (i as i32) * 12),
                VerticalPosition::Top,
                FontColor::Transparent(FG),
                fb,
            );
        }
        y += 12 * (wrapped.len() as i32).max(1) + 8;
    }

    let _ = F_MICRO.render(
        "press the middle button to continue >>",
        Point::new(16, BODY_BOTTOM - 16),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
}

/// Crude greedy word-wrapper. Splits on whitespace; never splits a word.
/// `max_chars` is the rendered width in monospace-equivalent characters —
/// fine for ASCII-only English. Empty input returns one empty line.
fn wrap_text(s: &str, max_chars: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if cur.is_empty() {
            // Even a single word longer than max_chars gets pushed as-is;
            // letting it overflow is less bad than dropping it.
            cur.push_str(word);
        } else if cur.len() + 1 + word.len() <= max_chars {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn render_step_await_join(fb: &mut FrameBuf<Rgb565, VecFb>, view: &ApSetupView, tick: u32) {
    let cy = (BODY_TOP + BODY_BOTTOM) / 2;

    let _ = F_MICRO.render(
        "OPEN YOUR PHONE'S WIFI SETTINGS",
        Point::new(28, cy - 50),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let _ = F_MICRO.render(
        "AND JOIN THIS WIFI NETWORK:",
        Point::new(28, cy - 40),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );

    // Big SSID card with a pulsing border (still animating == "we're waiting").
    let card_w: i32 = 244;
    let card_h: i32 = 38;
    let card_x = (W as i32 - card_w) / 2;
    let card_y = cy - 22;
    Rectangle::new(Point::new(card_x, card_y), Size::new(card_w as u32, card_h as u32))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(fb)
        .unwrap();
    let border_color = pulse_pink_cyan(tick);
    draw_frame_outline(
        fb,
        Rectangle::new(Point::new(card_x, card_y), Size::new(card_w as u32, card_h as u32)),
        2,
        border_color,
    );
    let _ = F_HEADER.render(
        view.ap_ssid.as_str(),
        Point::new(card_x + 14, card_y + 10),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );

    let _ = F_MICRO.render(
        "open WiFi network - no password needed",
        Point::new(28, cy + 22),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let phase = ((tick / 40) % 4) as usize;
    let dots: &str = match phase { 0 => "", 1 => ".", 2 => "..", _ => "..." };
    let wait_msg = format!("waiting for your phone to join this WiFi{dots}");
    let _ = F_MICRO.render(
        wait_msg.as_str(),
        Point::new(16, BODY_BOTTOM - 16),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
}

fn render_step_await_form(fb: &mut FrameBuf<Rgb565, VecFb>, view: &ApSetupView) {
    let modules_plus_quiet = view.qr_size as u32 + 4;
    let max_qr_box: u32 = 122;
    let px_per_module = (max_qr_box / modules_plus_quiet).max(1);
    let qr_top_left = Point::new(8, 28);
    draw_qr(fb, qr_top_left, view, px_per_module);

    let col_x = 8 + (modules_plus_quiet * px_per_module) as i32 + 14;
    let mut y: i32 = 30;

    let _ = F_MICRO.render(
        "SCAN OR OPEN",
        Point::new(col_x, y),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    y += 11;
    let _ = F_HEADER.render(
        view.url.as_str(),
        Point::new(col_x, y),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
    y += 22;

    let _ = F_MICRO.render(
        "then pick your network and",
        Point::new(col_x, y),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    y += 10;
    let _ = F_MICRO.render(
        "enter the password.",
        Point::new(col_x, y),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
}

fn render_step_saving(fb: &mut FrameBuf<Rgb565, VecFb>) {
    let cy = (BODY_TOP + BODY_BOTTOM) / 2;
    let _ = F_HEADER.render(
        "All set — rebooting...",
        Point::new(40, cy - 8),
        VerticalPosition::Top,
        FontColor::Transparent(OK),
        fb,
    );
}

fn pulse_pink_cyan(tick: u32) -> Rgb565 {
    let p = ((tick % 220) as f32) / 220.0;
    let t = if p < 0.5 { p * 2.0 } else { (1.0 - p) * 2.0 };
    let a = (236u8, 72u8, 153u8);
    let b = (56u8, 189u8, 248u8);
    let lerp = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t) as u8;
    rgb(lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

fn draw_frame_outline(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    r: Rectangle,
    thickness: u32,
    color: Rgb565,
) {
    Rectangle::new(r.top_left, Size::new(r.size.width, thickness))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
    Rectangle::new(
        Point::new(r.top_left.x, r.top_left.y + r.size.height as i32 - thickness as i32),
        Size::new(r.size.width, thickness),
    ).into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
    Rectangle::new(r.top_left, Size::new(thickness, r.size.height))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
    Rectangle::new(
        Point::new(r.top_left.x + r.size.width as i32 - thickness as i32, r.top_left.y),
        Size::new(thickness, r.size.height),
    ).into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
}

fn draw_qr(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    origin: Point,
    view: &ApSetupView,
    px_per_module: u32,
) {
    let quiet: u32 = 2;
    let total_modules = view.qr_size as u32 + quiet * 2;
    let total_px = total_modules * px_per_module;

    // White background (covers quiet zone too — required for decoders).
    Rectangle::new(origin, Size::new(total_px, total_px))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::WHITE))
        .draw(fb)
        .unwrap();

    let qr_origin = Point::new(
        origin.x + (quiet * px_per_module) as i32,
        origin.y + (quiet * px_per_module) as i32,
    );
    for y in 0..view.qr_size {
        for x in 0..view.qr_size {
            if view.qr_modules[y * view.qr_size + x] {
                let px = Point::new(
                    qr_origin.x + (x as u32 * px_per_module) as i32,
                    qr_origin.y + (y as u32 * px_per_module) as i32,
                );
                Rectangle::new(px, Size::new(px_per_module, px_per_module))
                    .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                    .draw(fb)
                    .unwrap();
            }
        }
    }
}

/// Encode `text` into a QR code (low ECC — enough margin while keeping
/// version small and pixels chunky). Returns (modules-per-side, row-major
/// bits) or None on encode failure (e.g. payload too long).
fn make_qr(text: &str) -> Option<(usize, Vec<bool>)> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(text, QrCodeEcc::Low).ok()?;
    let size = qr.size() as usize;
    let mut bits = Vec::with_capacity(size * size);
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            bits.push(qr.get_module(x, y));
        }
    }
    Some((size, bits))
}

/// Pick a color for a pretty-printed JSON line based on its first non-space
/// content. Cheap, no real tokenizer — works because `to_string_pretty` puts
/// each `"key": value` on its own line.
fn line_color(line: &str) -> Rgb565 {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return MUTED;
    }
    let b = trimmed.as_bytes()[0];
    match b {
        b'{' | b'}' | b'[' | b']' | b',' => JSON_PUNCT,
        b'"' => {
            // Could be a key ("foo": ...) or a string value. Look for ':' after
            // the matching closing quote — if present and value-side is a string,
            // we still color the key as key.
            if let Some(colon_at) = trimmed.find(':') {
                // Treat whole line as a key:value line: render in default and let
                // tokenize-on-the-fly happen via a split below. Simple heuristic:
                // if there's a colon, the line *starts* with a key.
                let _ = colon_at;
                JSON_KEY
            } else {
                JSON_STR
            }
        }
        b'0'..=b'9' | b'-' | b'.' => JSON_NUM,
        b't' | b'f' => JSON_NUM, // true/false
        b'n' => JSON_NUM,        // null
        _ => FG,
    }
}

fn pretty_json(raw: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(v) => serde_json::to_string_pretty(&v)
            .unwrap_or_else(|_| raw.to_string())
            .lines()
            .map(|l| l.to_string())
            .collect(),
        Err(_) => raw.lines().map(|l| l.to_string()).collect(),
    }
}

fn fetch_endpoint(url: &str) -> anyhow::Result<(u16, String)> {
    let conn = EspHttpConnection::new(&HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(Duration::from_secs(15)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(conn);
    let mut response = client.get(url)?.submit()?;
    let status = response.status();

    let mut body = Vec::with_capacity(2048);
    let mut buf = [0u8; 512];
    loop {
        let n = response.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
        if body.len() > 32 * 1024 {
            break;
        }
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    log::info!("boot, version={}", ota::CURRENT_VERSION);

    ota::mark_valid_if_pending();

    let peripherals = Peripherals::take().unwrap();
    let modem = peripherals.modem;
    let spi2 = peripherals.spi2;
    let pins = peripherals.pins;

    // Rotary encoder: A=GPIO2, B=GPIO1, button=GPIO0 (active low). Pull-ups
    // enabled in software; the T-Embed mechanical encoder pulls each line to
    // GND through the detent contact.
    let enc_a = PinDriver::input(pins.gpio2, Pull::Up).unwrap();
    let enc_b = PinDriver::input(pins.gpio1, Pull::Up).unwrap();
    let enc_btn = PinDriver::input(pins.gpio0, Pull::Up).unwrap();

    // --- Display bring-up ---
    let mut lcd_power = PinDriver::output(pins.gpio46).unwrap();
    lcd_power.set_high().unwrap();
    let mut backlight = PinDriver::output(pins.gpio15).unwrap();
    backlight.set_high().unwrap();

    let spi = SpiDeviceDriver::new_single(
        spi2,
        pins.gpio12,
        pins.gpio11,
        None::<AnyIOPin>,
        Some(pins.gpio10),
        &SpiDriverConfig::new(),
        &SpiConfig::new().baudrate(40_u32.MHz().into()),
    )
    .unwrap();

    let dc = PinDriver::output(pins.gpio13).unwrap();
    let rst = PinDriver::output(pins.gpio9).unwrap();

    let mut spi_buf = [0u8; 512];
    let di = SpiInterface::new(spi, dc, &mut spi_buf);
    let mut display = Builder::new(ST7789, di)
        .display_size(170, 320)
        .display_offset(35, 0)
        .orientation(Orientation::new().rotate(Rotation::Deg90))
        .color_order(ColorOrder::Bgr)
        .invert_colors(ColorInversion::Inverted)
        .reset_pin(rst)
        .init(&mut Ets)
        .unwrap();

    let mut fb = FrameBuf::new(VecFb(vec![BG; W * H]), W, H);
    let area = Rectangle::new(Point::zero(), Size::new(W as u32, H as u32));

    let mut state = UiState::default();
    state.method = "GET /healthz".into();
    state.ota_line = "booting...".into();
    state.ota_color = ACCENT;

    macro_rules! present {
        () => {{
            render(&mut fb, &state);
            display
                .fill_contiguous(&area, fb.data.0.iter().copied())
                .unwrap();
        }};
    }
    present!();

    // --- WiFi: load creds from NVS, fall back to AP-mode provisioning ---
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs_part = EspNvsPartition::<NvsDefault>::take().unwrap();
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs_part.clone())).unwrap(),
        sys_loop,
    )
    .unwrap();

    // Rebuild the visible-screen nav cycle from the user's saved plugin
    // config (or defaults if none). Done before the first non-default
    // render so the bottom strip dots are correct from the start.
    let plugin_entries = plugins::load(&nvs_part);
    state.visible_screens = plugins::visible_screens(&plugin_entries);
    if !state.visible_screens.contains(&state.current_screen) {
        state.current_screen = *state.visible_screens.first().unwrap_or(&Screen::Home);
    }
    log::info!(
        "plugins: nav cycle = {:?}",
        state.visible_screens
    );

    let stored = wifi::load_creds(&nvs_part);
    let mut connected_ssid = String::new();
    let mut connected = false;

    if let Some(c) = stored.as_ref() {
        log::info!("wifi: trying stored creds for {}", c.ssid);
        let ok = {
            let state_ref = &mut state;
            let fb_ref = &mut fb;
            let display_ref = &mut display;
            let area_ref = &area;
            wifi::try_connect(&mut wifi, c, |s| {
                state_ref.ota_line = s.to_string();
                state_ref.ota_color = ACCENT;
                render(fb_ref, state_ref);
                let _ = display_ref.fill_contiguous(area_ref, fb_ref.data.0.iter().copied());
            })
        };
        if ok {
            connected = true;
            connected_ssid = c.ssid.clone();
        }
    } else if !wifi_creds::SSID.is_empty() {
        // One-shot migration from the compile-time creds.
        let migration = wifi::Creds {
            ssid: wifi_creds::SSID.into(),
            pass: wifi_creds::PASS.into(),
        };
        log::info!("wifi: NVS empty, trying compile-time creds for migration");
        let ok = {
            let state_ref = &mut state;
            let fb_ref = &mut fb;
            let display_ref = &mut display;
            let area_ref = &area;
            wifi::try_connect(&mut wifi, &migration, |s| {
                state_ref.ota_line = s.to_string();
                state_ref.ota_color = ACCENT;
                render(fb_ref, state_ref);
                let _ = display_ref.fill_contiguous(area_ref, fb_ref.data.0.iter().copied());
            })
        };
        if ok {
            let _ = wifi::save_creds(&nvs_part, &migration);
            connected = true;
            connected_ssid = migration.ssid.clone();
        }
    }

    if !connected {
        let setup_url = format!("http://{}/", wifi::AP_IP);
        let (qr_size, qr_modules) = make_qr(&setup_url).expect("qr encode");
        state.ap_setup = Some(ApSetupView {
            qr_size,
            qr_modules,
            url: setup_url,
            ap_ssid: wifi::AP_SSID.into(),
            step: wifi::ProvisionStep::Explain,
        });
        state.ota_line = "Setup required — follow on-screen steps".into();
        state.ota_color = WARN;
        present!();

        // Closure that polls the encoder button and returns true once per
        // rising press edge (transition from "released" to "stably pressed").
        // 3 consecutive low reads = debounced. Mutable state is held inside
        // the closure so provision() doesn't have to know about debouncing.
        let mut last_low = enc_btn.is_low();
        let mut low_streak: u8 = 0;
        let mut consumed_press: bool = false;
        let continue_press = || -> bool {
            let cur = enc_btn.is_low();
            if cur {
                low_streak = low_streak.saturating_add(1);
            } else {
                low_streak = 0;
                consumed_press = false;
            }
            let stable = low_streak >= 3;
            let edge = stable && !last_low && !consumed_press;
            last_low = stable;
            if edge {
                consumed_press = true;
            }
            edge
        };

        // wifi::provision returns `!` — it restarts the chip once new creds
        // are POSTed to /save. on_step is called at each stage transition;
        // we mutate the AP view and re-render so the LCD always reflects
        // the current step.
        wifi::provision(
            &mut wifi,
            nvs_part.clone(),
            |step| {
                if let Some(view) = state.ap_setup.as_mut() {
                    view.step = step;
                }
                state.ota_line = match step {
                    wifi::ProvisionStep::Explain => "press middle button to continue".into(),
                    wifi::ProvisionStep::AwaitJoin => format!("AP: {}", wifi::AP_SSID),
                    wifi::ProvisionStep::AwaitForm => format!("URL: http://{}", wifi::AP_IP),
                    wifi::ProvisionStep::Saving => "Saved. Rebooting...".into(),
                };
                state.ota_color = match step {
                    wifi::ProvisionStep::Saving => OK,
                    _ => WARN,
                };
                render(&mut fb, &state);
                let _ = display.fill_contiguous(&area, fb.data.0.iter().copied());
            },
            continue_press,
        );
    }

    let ip_info = wifi.wifi().sta_netif().get_ip_info().unwrap();
    log::info!("wifi up, ip={}", ip_info.ip);
    state.wifi_ssid = connected_ssid;
    state.ip = format!("{}", ip_info.ip);
    state.mac = match wifi.wifi().get_mac(esp_idf_svc::wifi::WifiDeviceId::Sta) {
        Ok(m) => format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            m[0], m[1], m[2], m[3], m[4], m[5]
        ),
        Err(_) => "—".into(),
    };

    // Start the on-device management UI now that we know our addresses.
    // Held by `_web_server` for the lifetime of main(); dropping the handle
    // would tear down the server. Failure is non-fatal — the device keeps
    // working without remote configuration.
    let _web_server = match webconfig::start(
        webconfig::DeviceCtx {
            ssid: state.wifi_ssid.clone(),
            ip: state.ip.clone(),
            mac: state.mac.clone(),
        },
        nvs_part.clone(),
    ) {
        Ok(s) => {
            log::info!("webconfig: management UI at http://{}/", state.ip);
            Some(s)
        }
        Err(e) => {
            log::warn!("webconfig: server failed to start: {e}");
            None
        }
    };

    // Kick off SNTP so the Home screen's clocks get real wall-clock time.
    // First sync lands ~3-5 s later; we keep the handle alive for the rest
    // of main() so the SNTP client keeps polling in the background.
    let _sntp = esp_idf_svc::sntp::EspSntp::new_default().ok();
    if _sntp.is_some() {
        log::info!("sntp: started");
    }

    // Daemon thread that polls the OTA manifest every 30 s and stages an
    // update into the inactive slot when one's available. Doesn't reboot —
    // user sees "ready, reboot to apply" in the status strip.
    ota::spawn_background_poller();

    // Image component is in Idle state until the user first switches to
    // the Image tab; that click spawns a thread to fetch + decode so the
    // UI keeps animating the loading spinner while it runs.
    state.image = Some(image::RemoteImage::new(IMAGE_URL));

    // Kick off the Status-screen endpoint fetch in a worker thread so it
    // doesn't block boot. Result lands in `STATUS_RESULT` and gets
    // pulled into `state` by the event loop on the next tick that finds
    // it populated.
    refresh_endpoint_async();

    // Kick off the World Cup schedule fetch in another worker thread.
    // Same pattern: result lands in a static Mutex inside worldcup.rs
    // and the screen renders the fallback list until then.
    worldcup::kick_fetch();

    // Seed the bottom strip from the bg-poller's initial state. The
    // poller fires its first check within a few seconds and the event
    // loop picks up subsequent transitions.
    apply_bg_ota_to_strip(&mut state);
    present!();

    // --- event loop: poll encoder + button, react ---
    //
    // Encoder decoding: Ben Buxton's half-step state-machine decoder.
    // (http://www.buxtronix.net/2011/10/rotary-encoders-done-properly.html)
    //
    // Instead of looking at raw pin states or edges, we track a state in a
    // tiny finite-state machine. Each poll we compute the new pin state
    // (A<<1)|B and look up the next state in TTABLE[current_state][pin].
    // Most transitions just move us along an "in-progress CW" or "CCW"
    // path; bounces / glitches that don't match the expected sequence
    // bounce harmlessly back to R_START without emitting anything. A count
    // is emitted only when a *complete* half-step sequence finishes, which
    // is exactly once per detent on this 24-detent / 12-cycle encoder.
    //
    // Direction is encoded in the high nibble of the state byte:
    //   DIR_CW  = 0x10 → +1
    //   DIR_CCW = 0x20 → -1
    let mut enc_state: u8 = ENC_R_START;
    let mut btn_press_ticks: u32 = 0;
    // 5 ms tick × 400 = 2 s. Hold the encoder button for that long to wipe
    // saved WiFi creds and reboot into the setup AP.
    const LONG_PRESS_TICKS: u32 = 400;
    const ANIMATION_TICK_MOD: u32 = 10; // ≈50ms ≈ 20fps when animation needed
    /// Two button presses within this window count as a double-click
    /// (nav). One press without a second is a single-click.
    const DOUBLE_CLICK_WINDOW_MS: u32 = 280;
    /// Timestamp of the last debounced press while inside the double-click
    /// detection window. None when no click is "pending".
    let mut pending_press_ms: Option<u32> = None;
    let nvs_for_loop = nvs_part.clone();
    let mut last_bg_ota_state = ota::background_state();
    let mut last_bg_pct_drawn: u8 = u8::MAX;

    loop {
        state.tick = state.tick.wrapping_add(1);

        // Buxton half-step state-machine decoder. See preamble above loop.
        let pin_state = ((enc_a.is_high() as u8) << 1) | (enc_b.is_high() as u8);
        enc_state = ENC_TTABLE[(enc_state & 0x0F) as usize][pin_state as usize];
        let delta: i32 = match enc_state & 0x30 {
            ENC_DIR_CW => 1,
            ENC_DIR_CCW => -1,
            _ => 0,
        };

        // Button state machine.
        let cur_btn_low = enc_btn.is_low();
        let mut pressed = false;
        if cur_btn_low {
            btn_press_ticks = btn_press_ticks.saturating_add(1);
            if btn_press_ticks == LONG_PRESS_TICKS {
                state.ota_line = "Forgetting WiFi — reconnect on AP".into();
                state.ota_color = WARN;
                render(&mut fb, &state);
                let _ = display.fill_contiguous(&area, fb.data.0.iter().copied());
                let _ = wifi::clear_creds(&nvs_for_loop);
                std::thread::sleep(Duration::from_millis(900));
                unsafe { esp_idf_svc::sys::esp_restart() };
            }
        } else {
            if btn_press_ticks >= 3 && btn_press_ticks < LONG_PRESS_TICKS {
                pressed = true;
            }
            btn_press_ticks = 0;
        }

        let mut dirty = false;
        let now_ms = state.tick.saturating_mul(5);
        let game_playing = state.current_screen == Screen::Game
            && state.game.phase == game::Phase::Playing;

        // Rotation. In game's Playing phase, the encoder drives the cursor;
        // everywhere else it scrolls the body.
        if delta != 0 {
            if game_playing {
                state.game.move_cursor(delta);
                dirty = true;
            } else {
                let cap = match state.current_screen {
                    Screen::Status => status_body_visible_lines(),
                    Screen::Image => 1,
                    Screen::Game => 1,
                    Screen::Home => 1,
                    Screen::Fortune => 1,
                    Screen::System => system_body_visible_lines(),
                    Screen::Manage => 1,
                    Screen::Birthdays => 4,
                    Screen::Fasting => 1,
                    Screen::Quotes => 1,
                    Screen::Worldcup => 4,
                };
                let total = match state.current_screen {
                    Screen::Status => state.body_lines.len(),
                    Screen::Image => 0,
                    Screen::Game => 0,
                    Screen::Home => 0,
                    Screen::Fortune => 0,
                    Screen::System => system_rows(&state).len(),
                    Screen::Manage => manage_max_scroll_steps() + 1,
                    Screen::Birthdays => birthdays::row_count(),
                    Screen::Fasting => 0,
                    Screen::Quotes => 0,
                    Screen::Worldcup => worldcup::row_count(),
                };
                let max_scroll = total.saturating_sub(cap);
                let idx = state.current_screen as usize;
                let new_scroll = (state.screen_scroll[idx] as i32 + delta)
                    .max(0)
                    .min(max_scroll as i32) as usize;
                if new_scroll != state.screen_scroll[idx] {
                    state.screen_scroll[idx] = new_scroll;
                    dirty = true;
                }
            }
        }

        // Click routing.
        //   Game's Playing phase  → fire instantly on every press (snappy lock).
        //   Other screens / Idle  → buffer; emit a "single" after a 250 ms
        //                            window OR a "double" the moment a second
        //                            press lands inside that window.
        if pressed {
            if game_playing {
                state.game.lock();
                pending_press_ms = None;
                dirty = true;
            } else {
                match pending_press_ms {
                    Some(t) if now_ms.wrapping_sub(t) < DOUBLE_CLICK_WINDOW_MS => {
                        // DOUBLE-CLICK → cycle to next visible screen.
                        pending_press_ms = None;
                        state.current_screen =
                            next_screen(state.current_screen, &state.visible_screens);
                        dirty = true;
                        // Lazy-load image on first arrival at the Image
                        // tab. `img.load()` is non-blocking — it spawns
                        // a worker, transitions to Loading
                        // synchronously, and the event loop's animation
                        // tick keeps redrawing the spinner until the
                        // worker stores Ready/Failed. The call itself
                        // is idempotent: a second invocation while
                        // already Loading or Ready is a no-op.
                        if state.current_screen == Screen::Image {
                            if let Some(img) = state.image.as_ref() {
                                img.load();
                            }
                        }
                    }
                    _ => {
                        // First click of a potential double — start the window.
                        pending_press_ms = Some(now_ms);
                    }
                }
            }
        }

        // SINGLE-CLICK timeout — fires when the double-click window closes
        // without a second press.
        if let Some(t) = pending_press_ms {
            if now_ms.wrapping_sub(t) >= DOUBLE_CLICK_WINDOW_MS {
                pending_press_ms = None;
                // Single click means "screen-specific action" on screens
                // that have one; no-op elsewhere so accidental taps on the
                // way to a double-click don't fire anything.
                match state.current_screen {
                    Screen::Game => {
                        state.game.lock();
                        dirty = true;
                    }
                    Screen::Fortune => {
                        if !state.fortune.is_casting() {
                            state.fortune.consult();
                            dirty = true;
                        }
                    }
                    Screen::Fasting => {
                        state.fasting.click();
                        dirty = true;
                    }
                    _ => {}
                }
            }
        }

        // Visual-only tick for the game's hit/miss flash counters.
        if state.current_screen == Screen::Game
            && (state.game.hit_flash > 0 || state.game.miss_flash > 0)
            && state.tick % 2 == 0
        {
            state.game.tick_visual();
            dirty = true;
        }
        // Animation frames on screens that have moving content.
        let image_anim = matches!(state.current_screen, Screen::Image)
            && state.image.as_ref().map_or(false, |i| i.is_ready());
        // Loading spinner needs the same animation tick as Ready to keep
        // its 8-dot rotation alive while the worker fetches + decodes.
        let image_loading = matches!(state.current_screen, Screen::Image)
            && state.image.as_ref().map_or(false, |i| i.is_loading());
        let game_anim = state.current_screen == Screen::Game
            && state.game.phase == game::Phase::Playing;
        let home_anim = matches!(state.current_screen, Screen::Home);
        let fortune_anim = matches!(state.current_screen, Screen::Fortune);
        // Status screen with an empty body shows an animated "fetching..."
        // placeholder; once the async fetch lands, body is non-empty and
        // the screen is static again.
        let status_loading = matches!(state.current_screen, Screen::Status)
            && state.body_lines.is_empty();
        // Fasting: the live clock + timer countdown both need ~20 fps.
        let fasting_anim = matches!(state.current_screen, Screen::Fasting);
        // Quotes: the tiny countdown ring needs continuous redraw.
        let quotes_anim = matches!(state.current_screen, Screen::Quotes);
        if (image_anim
            || image_loading
            || game_anim
            || home_anim
            || fortune_anim
            || status_loading
            || fasting_anim
            || quotes_anim)
            && state.tick % ANIMATION_TICK_MOD == 0
        {
            // Decrement the fortune's casting timer on each animation tick.
            if state.current_screen == Screen::Fortune && state.fortune.is_casting() {
                state.fortune.tick_anim();
            }
            dirty = true;
        }

        // Pick up async status-fetch results.
        if poll_status_into_state(&mut state) {
            dirty = true;
        }

        // Advance the motivational-quotes rotation timer. tick() returns
        // true on the frame the displayed quote changed.
        if state.current_screen == Screen::Quotes {
            let now_ms = state.tick.wrapping_mul(5);
            if state.quotes.tick(now_ms) {
                dirty = true;
            }
        }

        // Reflect background-OTA state changes in the bottom strip.
        let bg = ota::background_state();
        let bg_pct = ota::background_download_pct();
        if bg != last_bg_ota_state
            || (bg == ota::BgState::Downloading && bg_pct != last_bg_pct_drawn)
        {
            apply_bg_ota_to_strip(&mut state);
            last_bg_ota_state = bg;
            last_bg_pct_drawn = bg_pct;
            dirty = true;
        }

        if dirty {
            render(&mut fb, &state);
            let _ = display.fill_contiguous(&area, fb.data.0.iter().copied());
        }

        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Translate the background OTA poller's atomic state into the bottom
/// status strip's text + color + progress bar.
fn apply_bg_ota_to_strip(state: &mut UiState) {
    match ota::background_state() {
        ota::BgState::Idle => {
            state.ota_line = format!("OTA: up to date (v{})", ota::CURRENT_VERSION);
            state.ota_color = OK;
            state.ota_progress = None;
        }
        ota::BgState::Checking => {
            state.ota_line = "OTA: checking...".into();
            state.ota_color = ACCENT;
            state.ota_progress = None;
        }
        ota::BgState::Downloading => {
            let pct = ota::background_download_pct();
            let v = ota::background_target_version();
            state.ota_line = if v.is_empty() {
                format!("OTA: downloading {pct}%")
            } else {
                format!("OTA: downloading v{v} {pct}%")
            };
            state.ota_color = WARN;
            state.ota_progress = Some(pct as f32 / 100.0);
        }
        ota::BgState::Ready => {
            let v = ota::background_target_version();
            state.ota_line = if v.is_empty() {
                "OTA: update ready — reboot to apply".into()
            } else {
                format!("OTA: v{v} ready — reboot to apply")
            };
            state.ota_color = OK;
            state.ota_progress = Some(1.0);
        }
        ota::BgState::Failed => {
            state.ota_line = "OTA: check failed".into();
            state.ota_color = ERR;
            state.ota_progress = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Async Status-screen endpoint fetch
// ---------------------------------------------------------------------------
//
// We don't block boot on the /healthz GET anymore. `refresh_endpoint_async`
// spawns a worker thread that does the HTTP fetch + pretty-print, then
// drops the result in STATUS_RESULT. The event loop calls
// `poll_status_into_state` each iteration; it's a single Mutex::take and
// returns false quickly when no result is pending.
//
// STATUS_LOADING gates spawning — a second call while a fetch is in
// flight is a no-op. Future "refresh now" buttons would go here.

struct StatusResult {
    status: Option<u16>,
    body_lines: Vec<String>,
    error: Option<String>,
}

static STATUS_RESULT: Mutex<Option<StatusResult>> = Mutex::new(None);
static STATUS_LOADING: AtomicBool = AtomicBool::new(false);

fn refresh_endpoint_async() {
    if STATUS_LOADING.swap(true, Ordering::SeqCst) {
        // Another fetch is already in flight; let it land.
        return;
    }
    let spawn = std::thread::Builder::new()
        .name("status-fetch".into())
        .stack_size(16 * 1024)
        .spawn(|| {
            log::info!("status: GET {URL}");
            let result = match fetch_endpoint(URL) {
                Ok((status, body)) => {
                    log::info!("status={status} body_len={}", body.len());
                    StatusResult {
                        status: Some(status),
                        body_lines: pretty_json(&body),
                        error: None,
                    }
                }
                Err(e) => {
                    log::warn!("status: GET err: {e}");
                    StatusResult {
                        status: None,
                        body_lines: vec![format!("{e}")],
                        error: Some(format!("GET err: {e}")),
                    }
                }
            };
            *STATUS_RESULT.lock().unwrap() = Some(result);
            STATUS_LOADING.store(false, Ordering::SeqCst);
        });
    if let Err(e) = spawn {
        log::warn!("status: failed to spawn fetcher: {e}");
        STATUS_LOADING.store(false, Ordering::SeqCst);
    }
}

/// Drain the latest fetch result into `state`. Returns true if anything
/// was applied — caller flags the frame dirty so the next render shows
/// the new body.
fn poll_status_into_state(state: &mut UiState) -> bool {
    let result = STATUS_RESULT.lock().unwrap().take();
    let Some(r) = result else {
        return false;
    };
    state.status = r.status;
    state.body_lines = r.body_lines;
    state.error = r.error;
    state.screen_scroll[Screen::Status as usize] = 0;
    true
}
