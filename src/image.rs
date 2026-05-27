//! Reusable component for fetching a JPEG from a public HTTPS URL and
//! rendering it into the framebuffer with a tasteful, animated border.
//!
//! Lifecycle:
//!   1. `RemoteImage::new(url)` constructs in `Idle`.
//!   2. `.load()` performs a blocking GET + decode and moves to `Ready`,
//!      `Failed`, or stays `Loading` while in flight.
//!   3. `.draw_in(...)` paints whatever state it's currently in — loading
//!      spinner, error card, or the decoded image with animated border.
//!
//! All state stays inside the value, so a single `RemoteImage` per URL can
//! be embedded in any screen and re-rendered as often as the UI wants.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_graphics_framebuf::FrameBuf;
use embedded_svc::{http::client::Client, io::Read};
use esp_idf_svc::http::client::{
    Configuration as HttpConfig, EspHttpConnection, FollowRedirectsPolicy,
};
use u8g2_fonts::{
    fonts,
    types::{FontColor, VerticalPosition},
    FontRenderer,
};

use crate::{VecFb, ACCENT, BG, ERR, FG, H, MUTED, PANEL, W, rgb};

/// Cap downloaded image at 2 MiB. PSRAM has 8 MB so this is a safety limit,
/// not a memory limit — keeps fetch latencies predictable. A 2 MiB JPEG
/// decodes to ~10-15 MiB of RGB888 transient memory which still fits.
const MAX_IMG_BYTES: usize = 2 * 1024 * 1024;

const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();

#[derive(Debug)]
pub enum LoadError {
    Http(String),
    BadStatus(u16),
    TooLarge(usize),
    Decode(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Http(e) => write!(f, "net: {e}"),
            LoadError::BadStatus(s) => write!(f, "HTTP {s}"),
            LoadError::TooLarge(n) => write!(f, "{} KiB > 256 KiB", n / 1024),
            LoadError::Decode(e) => write!(f, "decode: {e}"),
        }
    }
}

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Rgb565>,
}

enum State {
    Idle,
    Loading,
    Ready(DecodedImage),
    Failed(LoadError),
}

pub struct RemoteImage {
    url: String,
    /// Shared state — written from the loader thread, read from the UI
    /// thread. Locks are short (state transitions are single assignments;
    /// fetch + decode happens outside the lock).
    state: Arc<Mutex<State>>,
}

impl RemoteImage {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            state: Arc::new(Mutex::new(State::Idle)),
        }
    }

    pub fn url(&self) -> &str { &self.url }

    pub fn is_ready(&self) -> bool {
        matches!(&*self.state.lock().unwrap(), State::Ready(_))
    }

    pub fn is_loading(&self) -> bool {
        matches!(&*self.state.lock().unwrap(), State::Loading)
    }

    pub fn status_label(&self) -> String {
        match &*self.state.lock().unwrap() {
            State::Idle => "queued".into(),
            State::Loading => "loading...".into(),
            State::Ready(img) => format!("{}x{}", img.width, img.height),
            State::Failed(e) => format!("failed: {e}"),
        }
    }

    /// Non-blocking: marks the state as Loading and spawns a worker
    /// thread that fetches + decodes. The UI thread keeps redrawing the
    /// loading spinner each animation tick; when the worker completes
    /// the state transitions to Ready or Failed and the next render
    /// picks it up automatically. A second call while a load is in
    /// flight (or after one succeeded) is a no-op.
    pub fn load(&self) {
        // Skip if already in flight or already done. We refuse to retry
        // a Failed state from here — the user has to drop and re-create
        // the RemoteImage to retry. Keeps the threading model simple.
        {
            let mut s = self.state.lock().unwrap();
            if matches!(*s, State::Loading | State::Ready(_)) {
                return;
            }
            *s = State::Loading;
        }
        let state = Arc::clone(&self.state);
        let url = self.url.clone();
        // 16 KiB stack — enough for the TLS handshake + JPEG decoder
        // call frame, with room to spare. The decoder itself heap-
        // allocates its working buffers.
        let spawn = std::thread::Builder::new()
            .name("image-load".into())
            .stack_size(16 * 1024)
            .spawn(move || {
                log::info!("image: GET {url}");
                let new_state = match fetch_bytes(&url) {
                    Ok(bytes) => {
                        log::info!("image: fetched {} bytes", bytes.len());
                        match decode_jpeg(&bytes) {
                            Ok(img) => {
                                log::info!(
                                    "image: decoded {}x{}",
                                    img.width, img.height
                                );
                                State::Ready(img)
                            }
                            Err(e) => {
                                log::warn!("image: decode failed: {e}");
                                State::Failed(e)
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("image: fetch failed: {e}");
                        State::Failed(e)
                    }
                };
                *state.lock().unwrap() = new_state;
            });
        if let Err(e) = spawn {
            log::warn!("image: failed to spawn loader: {e}");
            *self.state.lock().unwrap() =
                State::Failed(LoadError::Http(format!("spawn: {e}")));
        }
    }

    /// Draw the image (or its loading/error state) within the given area.
    /// `tick` is a free-running counter from the event loop — used for the
    /// border + spinner animation.
    pub fn draw_in(
        &self,
        fb: &mut FrameBuf<Rgb565, VecFb>,
        area: Rectangle,
        tick: u32,
    ) {
        let state = self.state.lock().unwrap();
        match &*state {
            State::Ready(img) => draw_ready(fb, img, area, tick),
            State::Loading | State::Idle => draw_loading(fb, area, tick),
            State::Failed(e) => draw_error(fb, area, &e.to_string()),
        }
    }
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>, LoadError> {
    let conn = EspHttpConnection::new(&HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        follow_redirects_policy: FollowRedirectsPolicy::FollowAll,
        timeout: Some(Duration::from_secs(30)),
        buffer_size: Some(4096),
        buffer_size_tx: Some(1024),
        ..Default::default()
    })
    .map_err(|e| LoadError::Http(format!("{e:?}")))?;

    let mut client = Client::wrap(conn);
    let mut response = client
        .get(url)
        .map_err(|e| LoadError::Http(format!("{e:?}")))?
        .submit()
        .map_err(|e| LoadError::Http(format!("{e:?}")))?;

    let status = response.status();
    if status != 200 {
        return Err(LoadError::BadStatus(status));
    }

    let mut bytes: Vec<u8> = Vec::with_capacity(64 * 1024);
    // Heap-allocated to spare the (already-tight) main task stack while the
    // TLS + http-client + jpeg-decoder chain is on the call stack below us.
    let mut buf: Vec<u8> = vec![0u8; 4096];
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| LoadError::Http(format!("read: {e:?}")))?;
        if n == 0 {
            break;
        }
        if bytes.len() + n > MAX_IMG_BYTES {
            return Err(LoadError::TooLarge(bytes.len() + n));
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    Ok(bytes)
}

fn decode_jpeg(data: &[u8]) -> Result<DecodedImage, LoadError> {
    let mut decoder = jpeg_decoder::Decoder::new(data);

    // Read just the header first so we can decide how aggressively to
    // downscale before pulling the full coefficient table into memory.
    decoder
        .read_info()
        .map_err(|e| LoadError::Decode(format!("read_info: {e}")))?;
    let orig = decoder
        .info()
        .ok_or_else(|| LoadError::Decode("no info after header".into()))?;
    let orig_w = orig.width;
    let orig_h = orig.height;
    let orig_pixels = (orig_w as usize) * (orig_h as usize);

    // Refuse anything truly enormous — even at the smallest 1/8 native
    // scale, a 24-megapixel original would still produce ~375 KP of output
    // and several MB of intermediate state on a progressive JPEG. The
    // display is 320×170 = 54 KP; anyone uploading a 24 MP image should
    // resize first.
    const MAX_ORIGINAL_PIXELS: usize = 16 * 1024 * 1024; // ~16 MP
    if orig_pixels > MAX_ORIGINAL_PIXELS {
        return Err(LoadError::Decode(format!(
            "image too large: {}×{} ({} MP > 16 MP cap)",
            orig_w,
            orig_h,
            orig_pixels / 1_000_000
        )));
    }

    // Ask the decoder for the smallest 1/2ⁿ scale that's still at least as
    // large as the display. JPEG only supports 1/1, 1/2, 1/4, 1/8 IDCT
    // scaling, so a 3840×5120 source comes back as 480×640 — buffer drops
    // from 56 MiB to ~900 KiB.
    if orig_w as usize > W || orig_h as usize > H {
        let (sw, sh) = decoder
            .scale(W as u16, H as u16)
            .map_err(|e| LoadError::Decode(format!("scale: {e}")))?;
        log::info!(
            "image: decode-scaled {orig_w}×{orig_h} -> {sw}×{sh}"
        );
    }

    let pixels_raw = decoder
        .decode()
        .map_err(|e| LoadError::Decode(format!("decode: {e}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| LoadError::Decode("no info after decode".into()))?;
    let w = info.width as u32;
    let h = info.height as u32;
    let count = (w * h) as usize;

    let mut pixels: Vec<Rgb565> = Vec::with_capacity(count);
    match pixels_raw.len() {
        n if n == count * 3 => {
            // RGB888
            for i in 0..count {
                let r = pixels_raw[i * 3];
                let g = pixels_raw[i * 3 + 1];
                let b = pixels_raw[i * 3 + 2];
                pixels.push(Rgb565::new(r >> 3, g >> 2, b >> 3));
            }
        }
        n if n == count => {
            // Grayscale
            for i in 0..count {
                let v = pixels_raw[i];
                pixels.push(Rgb565::new(v >> 3, v >> 2, v >> 3));
            }
        }
        n => {
            return Err(LoadError::Decode(format!(
                "unexpected pixel buffer: {} bytes for {}x{}",
                n, w, h
            )));
        }
    }

    Ok(DecodedImage { width: w, height: h, pixels })
}

fn draw_ready(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    img: &DecodedImage,
    area: Rectangle,
    tick: u32,
) {
    // Compute target rect with aspect-preserving scale-down. Leave 8 px of
    // padding so the animated border has room to breathe.
    let pad: i32 = 8;
    let max_w = (area.size.width as i32 - 2 * pad).max(1);
    let max_h = (area.size.height as i32 - 2 * pad).max(1);
    let iw = img.width as i32;
    let ih = img.height as i32;

    // Scale = min(max_w/iw, max_h/ih), but never upscale.
    let (target_w, target_h) = if iw <= max_w && ih <= max_h {
        (iw, ih)
    } else {
        let scale_w = (max_w as f32) / (iw as f32);
        let scale_h = (max_h as f32) / (ih as f32);
        let scale = scale_w.min(scale_h);
        (((iw as f32) * scale) as i32, ((ih as f32) * scale) as i32)
    };

    let center = area.center();
    let target_x = center.x - target_w / 2;
    let target_y = center.y - target_h / 2;

    // Nearest-neighbor scale into the framebuffer. Direct indexed writes are
    // much faster than going through DrawTarget for per-pixel ops.
    let fb_w = crate::W as i32;
    let fb_h = crate::H as i32;
    for ty in 0..target_h {
        let sy = ((ty * ih) / target_h).min(ih - 1) as usize;
        let row = sy * (img.width as usize);
        let py = target_y + ty;
        if py < 0 || py >= fb_h {
            continue;
        }
        for tx in 0..target_w {
            let sx = ((tx * iw) / target_w).min(iw - 1) as usize;
            let px = target_x + tx;
            if px < 0 || px >= fb_w {
                continue;
            }
            let pixel = img.pixels[row + sx];
            fb.data.0[(py as usize) * (fb_w as usize) + (px as usize)] = pixel;
        }
    }

    // Animated border around the image. Two concentric rounded-ish frames.
    let border = Rectangle::new(
        Point::new(target_x - 3, target_y - 3),
        Size::new((target_w + 6) as u32, (target_h + 6) as u32),
    );
    draw_animated_border(fb, border, tick);
}

fn draw_loading(fb: &mut FrameBuf<Rgb565, VecFb>, area: Rectangle, tick: u32) {
    // Dim card background.
    Rectangle::new(
        Point::new(area.top_left.x + 24, area.top_left.y + area.size.height as i32 / 2 - 22),
        Size::new(area.size.width - 48, 44),
    )
    .into_styled(PrimitiveStyle::with_fill(PANEL))
    .draw(fb)
    .unwrap();

    // Spinner: 8 dots around a circle, intensity drops with index offset.
    let center = area.center();
    const N: usize = 8;
    const R: f32 = 10.0;
    let head = (tick / 8) as usize % N;
    for i in 0..N {
        let theta = (i as f32) * std::f32::consts::TAU / (N as f32);
        let x = center.x + (R * theta.cos()) as i32;
        let y = center.y - 4 + (R * theta.sin()) as i32;
        // Distance behind head determines brightness.
        let d = (head + N - i) % N;
        let scale = 1.0 - (d as f32) / (N as f32);
        let color = lerp_color(MUTED, ACCENT, scale);
        Rectangle::new(Point::new(x - 1, y - 1), Size::new(2, 2))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .unwrap();
    }

    let _ = F_MICRO.render(
        "loading image",
        Point::new(center.x - 36, center.y + 12),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
}

fn draw_error(fb: &mut FrameBuf<Rgb565, VecFb>, area: Rectangle, msg: &str) {
    Rectangle::new(
        Point::new(area.top_left.x + 16, area.top_left.y + area.size.height as i32 / 2 - 30),
        Size::new(area.size.width - 32, 60),
    )
    .into_styled(PrimitiveStyle::with_fill(PANEL))
    .draw(fb)
    .unwrap();

    let center = area.center();
    let _ = F_BODY.render(
        "couldn't load image",
        Point::new(center.x - 60, center.y - 18),
        VerticalPosition::Top,
        FontColor::Transparent(ERR),
        fb,
    );
    let truncated: String = msg.chars().take(48).collect();
    let _ = F_MICRO.render(
        truncated.as_str(),
        Point::new(center.x - 110, center.y + 0),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let _ = F_MICRO.render(
        "press to retry",
        Point::new(center.x - 32, center.y + 14),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
}

/// Two-layer animated border. Outer layer cycles through accent hues; inner
/// layer is a faint highlight that pulses brightness. Cheap: 4 fills/frame.
fn draw_animated_border(fb: &mut FrameBuf<Rgb565, VecFb>, frame: Rectangle, tick: u32) {
    let outer = pulse_color(tick, 220, 0.0);
    let inner = pulse_color(tick, 220, 0.5); // 180° out of phase

    draw_frame(fb, frame, 2, outer);

    let inset = Rectangle::new(
        Point::new(frame.top_left.x + 3, frame.top_left.y + 3),
        Size::new(frame.size.width - 6, frame.size.height - 6),
    );
    draw_frame(fb, inset, 1, inner);
}

fn draw_frame(fb: &mut FrameBuf<Rgb565, VecFb>, r: Rectangle, thickness: u32, color: Rgb565) {
    // Top
    Rectangle::new(r.top_left, Size::new(r.size.width, thickness))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
    // Bottom
    Rectangle::new(
        Point::new(r.top_left.x, r.top_left.y + r.size.height as i32 - thickness as i32),
        Size::new(r.size.width, thickness),
    )
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
    // Left
    Rectangle::new(r.top_left, Size::new(thickness, r.size.height))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
    // Right
    Rectangle::new(
        Point::new(r.top_left.x + r.size.width as i32 - thickness as i32, r.top_left.y),
        Size::new(thickness, r.size.height),
    )
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb).unwrap();
}

/// Triangle-wave pulse between ACCENT and a cyan complement, over `period`
/// ticks. `phase_offset` in [0, 1) shifts the wave.
fn pulse_color(tick: u32, period: u32, phase_offset: f32) -> Rgb565 {
    let p = ((tick % period) as f32) / (period as f32);
    let shifted = (p + phase_offset).fract();
    let t = if shifted < 0.5 { shifted * 2.0 } else { (1.0 - shifted) * 2.0 };
    // Endpoints: ACCENT (#ec4899) → CYAN-ish (#38bdf8 / sky-400)
    let a = (236, 72, 153);
    let b = (56, 189, 248);
    let r = lerp_u8(a.0, b.0, t);
    let g = lerp_u8(a.1, b.1, t);
    let bch = lerp_u8(a.2, b.2, t);
    rgb(r, g, bch)
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t) as u8
}

fn lerp_color(a: Rgb565, b: Rgb565, t: f32) -> Rgb565 {
    // Approx by sampling underlying 5/6/5 channels.
    let ar = a.r() as f32; let ag = a.g() as f32; let ab = a.b() as f32;
    let br = b.r() as f32; let bg = b.g() as f32; let bb = b.b() as f32;
    Rgb565::new(
        ((ar * (1.0 - t) + br * t) as u8) & 0x1f,
        ((ag * (1.0 - t) + bg * t) as u8) & 0x3f,
        ((ab * (1.0 - t) + bb * t) as u8) & 0x1f,
    )
}

// Keep BG referenced to silence a dead-code complaint if module-only changes.
#[allow(dead_code)]
const _BG_REF: Rgb565 = BG;
