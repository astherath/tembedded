//! Safe-cracking minigame.
//!
//! The player rotates the encoder to slide a cursor along a horizontal bar.
//! A green "sweet spot" of fixed width is randomly placed on the bar; when
//! the cursor is inside it, a single click locks the current tumbler and
//! advances. Three tumblers, each narrower than the last. Score is computed
//! on completion based on elapsed time and miss count.
//!
//! The minigame owns no input handling — `main` routes encoder rotation
//! into `Game::move_cursor` and the middle-button click into `Game::lock`
//! when the game screen is active.

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_graphics_framebuf::FrameBuf;
use u8g2_fonts::{
    fonts,
    types::{FontColor, VerticalPosition},
    FontRenderer,
};

use crate::{rgb, VecFb, ACCENT, ERR, FG, H, MUTED, OK, PANEL, W};

const F_HEADER: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const F_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();
const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_HUGE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_logisoso24_tn>();

/// Cursor position lives in this integer range. Larger = finer aim, but at
/// some point the screen's pixel resolution becomes the limit.
pub const BAR_RANGE: i32 = 200;
const NUM_TUMBLERS: u8 = 3;
/// Width of the green sweet spot per tumbler (in BAR_RANGE units), gets
/// narrower each round so the game ramps in difficulty.
const TUMBLER_WIDTHS: [i32; NUM_TUMBLERS as usize] = [44, 28, 16];

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum Phase {
    Idle,
    Playing,
    Done,
}

pub struct Game {
    pub phase: Phase,
    /// 0..NUM_TUMBLERS — how many tumblers locked so far during this run.
    pub locked: u8,
    pub cursor: i32,
    pub green_start: i32,
    pub green_width: i32,
    pub misses: u32,
    /// Final score, only meaningful in Phase::Done.
    pub score: i32,
    /// Visual "just-hit" flash counter — decrements each frame, drives a
    /// quick green pulse on the bar after a successful lock.
    pub hit_flash: u8,
    /// Visual "just-missed" flash counter — same, but red.
    pub miss_flash: u8,
    started_us: i64,
    finished_us: i64,
}

impl Game {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            locked: 0,
            cursor: BAR_RANGE / 2,
            green_start: 0,
            green_width: TUMBLER_WIDTHS[0],
            misses: 0,
            score: 0,
            hit_flash: 0,
            miss_flash: 0,
            started_us: 0,
            finished_us: 0,
        }
    }

    pub fn start(&mut self) {
        self.phase = Phase::Playing;
        self.locked = 0;
        self.misses = 0;
        self.score = 0;
        self.cursor = BAR_RANGE / 2;
        self.started_us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
        self.randomize_tumbler();
    }

    fn randomize_tumbler(&mut self) {
        let idx = self.locked.min(NUM_TUMBLERS - 1) as usize;
        let width = TUMBLER_WIDTHS[idx];
        self.green_width = width;
        let max_start = BAR_RANGE - width;
        let r: u32 = unsafe { esp_idf_svc::sys::esp_random() };
        self.green_start = (r % (max_start as u32 + 1)) as i32;
    }

    pub fn move_cursor(&mut self, delta: i32) {
        if self.phase != Phase::Playing {
            return;
        }
        // Buxton decoder upstream gives us one count per detent reliably.
        // 8 × covers BAR_RANGE=200 in ~25 detents (~1 full revolution of
        // the 24-detent wheel) with enough resolution for fine alignment.
        self.cursor = (self.cursor + delta * 8).clamp(0, BAR_RANGE - 1);
    }

    /// Handle a single-click. Behavior depends on phase:
    /// - Idle / Done → start (or restart) a new run.
    /// - Playing → if cursor is in the green zone, advance; otherwise count
    ///   a miss and re-randomize the tumbler so the player can try again.
    pub fn lock(&mut self) {
        match self.phase {
            Phase::Idle | Phase::Done => self.start(),
            Phase::Playing => {
                let in_zone = self.cursor >= self.green_start
                    && self.cursor < self.green_start + self.green_width;
                if in_zone {
                    self.locked = self.locked.saturating_add(1);
                    self.hit_flash = 6;
                    if self.locked >= NUM_TUMBLERS {
                        self.finish();
                    } else {
                        self.randomize_tumbler();
                    }
                } else {
                    self.misses = self.misses.saturating_add(1);
                    self.miss_flash = 6;
                    self.randomize_tumbler();
                }
            }
        }
    }

    fn finish(&mut self) {
        self.phase = Phase::Done;
        self.finished_us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
        let elapsed_ms = ((self.finished_us - self.started_us) / 1000).max(0) as i32;
        // 1000 base, lose 20 per second elapsed, 100 per miss. Clamped non-neg.
        self.score = 1000i32
            .saturating_sub(elapsed_ms / 50)
            .saturating_sub((self.misses as i32).saturating_mul(100))
            .max(0);
    }

    pub fn tick_visual(&mut self) {
        if self.hit_flash > 0 {
            self.hit_flash -= 1;
        }
        if self.miss_flash > 0 {
            self.miss_flash -= 1;
        }
    }

    pub fn elapsed_ms_now(&self) -> u32 {
        let end_us = if self.phase == Phase::Done {
            self.finished_us
        } else if self.phase == Phase::Playing {
            unsafe { esp_idf_svc::sys::esp_timer_get_time() }
        } else {
            return 0;
        };
        ((end_us - self.started_us) / 1000).max(0) as u32
    }
}

// ----- rendering ----------------------------------------------------------

pub fn render(fb: &mut FrameBuf<Rgb565, VecFb>, game: &Game, body_top: i32, body_bottom: i32) {
    // Title row — label on the left, tumbler progress dots on the right.
    let _ = F_MICRO.render(
        "SAFE",
        Point::new(10, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let _ = F_HEADER.render(
        "Crack the lock",
        Point::new(10, body_top + 14),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );

    let dot_x = W as i32 - 56;
    for i in 0..NUM_TUMBLERS {
        let color = if i < game.locked {
            OK
        } else if i == game.locked && game.phase == Phase::Playing {
            ACCENT
        } else {
            rgb(70, 80, 100)
        };
        let x = dot_x + (i as i32) * 14;
        let size: u32 = if i < game.locked { 8 } else { 6 };
        let off: i32 = if i < game.locked { 12 } else { 13 };
        Rectangle::new(Point::new(x, body_top + off), Size::new(size, size))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(fb)
            .unwrap();
    }

    match game.phase {
        Phase::Idle => render_idle(fb, body_top, body_bottom),
        Phase::Playing => render_playing(fb, game, body_top, body_bottom),
        Phase::Done => render_done(fb, game, body_top, body_bottom),
    }
}

fn render_idle(fb: &mut FrameBuf<Rgb565, VecFb>, body_top: i32, body_bottom: i32) {
    let cy = (body_top + body_bottom) / 2;
    let _ = F_HEADER.render(
        "rotate the wheel to slide the cursor",
        Point::new(10, cy - 24),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
    let _ = F_BODY.render(
        "into the green zone, then click once to",
        Point::new(10, cy - 6),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let _ = F_BODY.render(
        "lock the tumbler. Three locks to crack.",
        Point::new(10, cy + 7),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let _ = F_MICRO.render(
        "press the middle button to start >>",
        Point::new(10, body_bottom - 14),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
}

fn render_playing(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    game: &Game,
    body_top: i32,
    body_bottom: i32,
) {
    let elapsed_ms = game.elapsed_ms_now();
    let s = elapsed_ms / 1000;
    let cs = (elapsed_ms / 10) % 100;

    // Stats row
    let _ = F_MICRO.render(
        "TIME",
        Point::new(10, body_top + 36),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let time_text = format!("{s}.{cs:02}s");
    let _ = F_HEADER.render(
        time_text.as_str(),
        Point::new(10, body_top + 46),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );

    let _ = F_MICRO.render(
        "MISSES",
        Point::new(W as i32 - 70, body_top + 36),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let miss_text = format!("{}", game.misses);
    let _ = F_HEADER.render(
        miss_text.as_str(),
        Point::new(W as i32 - 70, body_top + 46),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );

    // Bar
    let bar_x = 24;
    let bar_y = body_top + 78;
    let bar_w = W as i32 - 48;
    let bar_h: i32 = 22;

    // Track background — tinted red on a recent miss for a quick UX cue.
    let bar_bg = if game.miss_flash > 0 { rgb(80, 30, 40) } else { PANEL };
    Rectangle::new(
        Point::new(bar_x, bar_y),
        Size::new(bar_w as u32, bar_h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(bar_bg))
    .draw(fb)
    .unwrap();

    // Green sweet spot
    let green_x = bar_x + (game.green_start * bar_w) / BAR_RANGE;
    let green_w = ((game.green_width * bar_w) / BAR_RANGE).max(2);
    let green_color = if game.hit_flash > 0 {
        rgb(140, 255, 180)
    } else {
        OK
    };
    Rectangle::new(
        Point::new(green_x, bar_y),
        Size::new(green_w as u32, bar_h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(green_color))
    .draw(fb)
    .unwrap();

    // Top + bottom edge highlights
    let edge = rgb(60, 70, 90);
    Rectangle::new(Point::new(bar_x, bar_y), Size::new(bar_w as u32, 1))
        .into_styled(PrimitiveStyle::with_fill(edge))
        .draw(fb)
        .unwrap();
    Rectangle::new(
        Point::new(bar_x, bar_y + bar_h - 1),
        Size::new(bar_w as u32, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(edge))
    .draw(fb)
    .unwrap();

    // Cursor — vertical line through the bar plus a triangle above
    let cursor_x = bar_x + (game.cursor * bar_w) / BAR_RANGE;
    Rectangle::new(Point::new(cursor_x, bar_y - 4), Size::new(2, (bar_h + 8) as u32))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();
    // Filled-triangle pointer above (top wider, pointing down)
    for i in 0..6 {
        let half = (5 - i).max(0);
        let w = (half * 2 + 1) as u32;
        let x = cursor_x + 1 - half;
        let y = bar_y - 10 + i;
        Rectangle::new(Point::new(x, y), Size::new(w, 1))
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb)
            .unwrap();
    }

    // Hint
    let hint = format!("TUMBLER {} / {}  —  CLICK TO LOCK", game.locked + 1, NUM_TUMBLERS);
    let _ = F_MICRO.render(
        hint.as_str(),
        Point::new(24, body_bottom - 14),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
}

fn render_done(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    game: &Game,
    body_top: i32,
    body_bottom: i32,
) {
    let elapsed_ms = ((game.finished_us - game.started_us) / 1000).max(0) as u32;
    let s = elapsed_ms / 1000;
    let cs = (elapsed_ms / 10) % 100;

    let _ = F_MICRO.render(
        "SAFE CRACKED",
        Point::new(10, body_top + 36),
        VerticalPosition::Top,
        FontColor::Transparent(OK),
        fb,
    );

    // Big score, centered
    let score_text = format!("{}", game.score);
    // Each F_HUGE digit is ~20 px wide.
    let approx_w = (score_text.len() as i32) * 20;
    let score_x = (W as i32 - approx_w) / 2;
    let _ = F_HUGE.render(
        score_text.as_str(),
        Point::new(score_x, body_top + 46),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );

    // Stats line
    let stats = format!("{s}.{cs:02}s   •   {} miss{}", game.misses, if game.misses == 1 { "" } else { "es" });
    let _ = F_BODY.render(
        stats.as_str(),
        Point::new((W as i32 - 140) / 2, body_top + 92),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    let _ = F_MICRO.render(
        "click to play again  /  double-click to leave",
        Point::new(20, body_bottom - 14),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    // Suppress unused-import warning if ERR/H ever drop out of scope changes.
    let _ = (ERR, H);
}
