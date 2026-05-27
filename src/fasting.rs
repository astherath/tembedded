//! FASTING screen — intermittent-fasting helper with two stacked panes.
//!
//! Layout: a thin title strip ("FASTING · 16h / 8h") sits at the top of the
//! body region, then the screen splits horizontally:
//!
//!   * Pane A (top, ~70 px) — SCHEDULE
//!     Shows the hardcoded 16/8 window referenced to local wall-clock time
//!     (Miami / EDT, matches `home.rs`). The window is:
//!       FAST  16:00 → 08:00 next day (16h)
//!       EAT   08:00 → 16:00 same day  (8h)
//!     A pill badge reports whether we're currently fasting (WARN) or eating
//!     (OK), the time of the next transition, and an "Xh YYm remaining"
//!     countdown.
//!
//!   * Pane B (bottom, ~70 px) — MANUAL TIMER
//!     A monotonic stopwatch with a circular progress ring around an HH:MM:SS
//!     readout. The ring fills clockwise from 12 o'clock, completing at the
//!     16h target. The timer is driven by `esp_timer_get_time()` so it does
//!     not jump when SNTP re-syncs the wall clock.
//!
//! Click semantics (single-click while this screen is focused):
//!     Idle    -> Running   (start a new fast)
//!     Running -> Paused    (hold)
//!     Paused  -> Running   (resume; previous elapsed is preserved)
//!     Done    -> Idle      (reset and acknowledge the completed fast)

use std::time::{SystemTime, UNIX_EPOCH};

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

use crate::{rgb, VecFb, ACCENT, FG, MUTED, OK, PANEL, W, WARN};

// Match home.rs — Miami EDT, hardcoded for current DST window.
const MIAMI_OFFSET_S: i64 = -4 * 3600;

// Hardcoded inset (mirrors main.rs's private BODY_LEFT).
const INSET: i32 = 10;

// 16/8 schedule (24h clock).
const FAST_START_HOUR: u32 = 16;
const EAT_START_HOUR: u32 = 8;

// Manual timer target = 16h.
const TARGET_SECS: u64 = 16 * 3600;

// Fonts. F_HUGE is digit-only (the `_tn` variant) — used for the big
// HH:MM:SS readout next to the progress ring.
const F_HUGE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_logisoso24_tn>();
const F_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();
const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
    Done,
}

pub struct Fasting {
    target_secs: u64,
    state: TimerState,
    /// Microseconds (esp_timer_get_time) when the current Running segment
    /// started. Only meaningful while `state == Running`.
    started_us: i64,
    /// Microseconds accumulated from previous Running segments (i.e. all the
    /// segments before the most recent pause).
    paused_elapsed_us: i64,
}

impl Fasting {
    pub fn new() -> Self {
        Self {
            target_secs: TARGET_SECS,
            state: TimerState::Idle,
            started_us: 0,
            paused_elapsed_us: 0,
        }
    }

    /// Single-click handler — see file-level doc for the state diagram.
    ///
    /// If a Running or Paused timer's elapsed has crossed the 16h target,
    /// this click latches the Done state (rather than the usual Paused /
    /// Running transition). A subsequent click on Done resets to Idle.
    /// Render derives a visual "DONE" label whenever elapsed >= target,
    /// regardless of stored state, so the user always gets the cue.
    pub fn click(&mut self) {
        let crossed_target = matches!(self.state, TimerState::Running | TimerState::Paused)
            && self.elapsed_secs() >= self.target_secs;
        if crossed_target {
            // Freeze elapsed at exactly the target so the readout shows the
            // clean 16:00:00 value across subsequent renders.
            self.paused_elapsed_us = (self.target_secs as i64).saturating_mul(1_000_000);
            self.started_us = 0;
            self.state = TimerState::Done;
            return;
        }

        match self.state {
            TimerState::Idle => {
                self.paused_elapsed_us = 0;
                self.started_us = now_us();
                self.state = TimerState::Running;
            }
            TimerState::Running => {
                // Fold the current segment into the accumulator.
                let now = now_us();
                let segment = (now - self.started_us).max(0);
                self.paused_elapsed_us = self.paused_elapsed_us.saturating_add(segment);
                self.state = TimerState::Paused;
            }
            TimerState::Paused => {
                self.started_us = now_us();
                self.state = TimerState::Running;
            }
            TimerState::Done => {
                self.paused_elapsed_us = 0;
                self.started_us = 0;
                self.state = TimerState::Idle;
            }
        }
    }

    /// Total elapsed seconds across all run / pause cycles. While Running
    /// this includes the live segment; while Paused it includes only the
    /// accumulator. While Done it stays clamped at the target.
    fn elapsed_secs(&self) -> u64 {
        let micros = match self.state {
            TimerState::Idle => 0,
            TimerState::Running => {
                let live = (now_us() - self.started_us).max(0);
                self.paused_elapsed_us.saturating_add(live)
            }
            TimerState::Paused => self.paused_elapsed_us,
            TimerState::Done => (self.target_secs as i64).saturating_mul(1_000_000),
        };
        (micros / 1_000_000).max(0) as u64
    }
}

#[inline]
fn now_us() -> i64 {
    unsafe { esp_idf_svc::sys::esp_timer_get_time() }
}

// =========================================================================
// Rendering
// =========================================================================

pub fn render(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    s: &Fasting,
    body_top: i32,
    body_bottom: i32,
    tick: u32,
) {
    // ----- title strip -----
    let _ = F_MICRO.render(
        "FASTING",
        Point::new(INSET, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let _ = F_MICRO.render(
        "16h / 8h",
        Point::new(INSET + 56, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    // Title underline
    Rectangle::new(
        Point::new(INSET, body_top + 16),
        Size::new((W as i32 - INSET * 2) as u32, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
    .draw(fb)
    .unwrap();

    // Split the body roughly in half, leaving the title strip alone.
    let pane_top = body_top + 20;
    let usable_h = body_bottom - pane_top;
    let split_y = pane_top + usable_h / 2;

    // Horizontal divider between the two panes.
    Rectangle::new(
        Point::new(INSET, split_y),
        Size::new((W as i32 - INSET * 2) as u32, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
    .draw(fb)
    .unwrap();

    draw_schedule_pane(fb, pane_top, split_y - 2, tick);
    draw_timer_pane(fb, s, split_y + 2, body_bottom, tick);
}

// ----- Pane A: Schedule ---------------------------------------------------

fn draw_schedule_pane(fb: &mut FrameBuf<Rgb565, VecFb>, top: i32, bottom: i32, _tick: u32) {
    let (unix_secs, synced) = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.as_secs() > 1_704_067_200),
        Err(_) => (0, false),
    };

    if !synced {
        let _ = F_BODY.render(
            "waiting for time sync...",
            Point::new(INSET, top + 6),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
        let _ = F_MICRO.render(
            "schedule unlocks once SNTP confirms.",
            Point::new(INSET, top + 22),
            VerticalPosition::Top,
            FontColor::Transparent(rgb(110, 130, 170)),
            fb,
        );
        return;
    }

    let local_secs = unix_secs + MIAMI_OFFSET_S;
    let mod_day = local_secs.rem_euclid(86_400) as u32;
    let cur_h = mod_day / 3600;

    // Fasting window is FAST_START_HOUR..24 + 0..EAT_START_HOUR. Eating is
    // everything in between (EAT_START_HOUR..FAST_START_HOUR).
    let in_fast = cur_h >= FAST_START_HOUR || cur_h < EAT_START_HOUR;
    let (badge_text, badge_color, transition_hour) = if in_fast {
        ("FASTING NOW", WARN, EAT_START_HOUR)
    } else {
        ("EATING NOW", OK, FAST_START_HOUR)
    };

    // Seconds remaining until the next transition.
    let now_sod = mod_day as i64;
    let target_sod = (transition_hour as i64) * 3600;
    let mut diff = target_sod - now_sod;
    if diff <= 0 {
        diff += 86_400;
    }
    let rem_h = diff / 3600;
    let rem_m = (diff % 3600) / 60;

    // Pill badge (top-left of pane).
    let badge_x = INSET;
    let badge_y = top + 4;
    let badge_w: i32 = 96;
    let badge_h: i32 = 16;
    Rectangle::new(
        Point::new(badge_x, badge_y),
        Size::new(badge_w as u32, badge_h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(PANEL))
    .draw(fb)
    .unwrap();
    // Color accent stripe on the left edge.
    Rectangle::new(
        Point::new(badge_x, badge_y),
        Size::new(3, badge_h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(badge_color))
    .draw(fb)
    .unwrap();
    let _ = F_MICRO.render(
        badge_text,
        Point::new(badge_x + 8, badge_y + 4),
        VerticalPosition::Top,
        FontColor::Transparent(badge_color),
        fb,
    );

    // "until HH:00" — to the right of the badge.
    let until_text = format!("until {:02}:00", transition_hour);
    let _ = F_BODY.render(
        until_text.as_str(),
        Point::new(badge_x + badge_w + 10, badge_y + 3),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );

    // Countdown line below.
    let countdown = format!("{}h {:02}m remaining", rem_h, rem_m);
    let _ = F_BODY.render(
        countdown.as_str(),
        Point::new(INSET, badge_y + badge_h + 4),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    // Mini schedule strip — visual band showing where in the 24h day we are.
    // The band spans 24 hours; the eating window is highlighted in OK.
    let strip_x = INSET;
    let strip_y = badge_y + badge_h + 20;
    let strip_w = W as i32 - INSET * 2;
    let strip_h: i32 = 6;
    if strip_y + strip_h <= bottom {
        // Background (fasting window default).
        Rectangle::new(
            Point::new(strip_x, strip_y),
            Size::new(strip_w as u32, strip_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(rgb(60, 50, 30)))
        .draw(fb)
        .unwrap();

        // Eating window highlight (8:00..16:00 of the same day).
        let eat_x0 = strip_x + (EAT_START_HOUR as i32) * strip_w / 24;
        let eat_x1 = strip_x + (FAST_START_HOUR as i32) * strip_w / 24;
        Rectangle::new(
            Point::new(eat_x0, strip_y),
            Size::new((eat_x1 - eat_x0) as u32, strip_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(rgb(30, 70, 40)))
        .draw(fb)
        .unwrap();

        // "Now" marker — a 2-px ACCENT slice.
        let now_x = strip_x + (mod_day as i32) * strip_w / 86_400;
        Rectangle::new(
            Point::new(now_x, strip_y - 2),
            Size::new(2, (strip_h + 4) as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();

        // Endpoint labels for the eating window.
        let _ = F_MICRO.render(
            "08",
            Point::new(eat_x0 - 6, strip_y + strip_h + 1),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
        let _ = F_MICRO.render(
            "16",
            Point::new(eat_x1 - 6, strip_y + strip_h + 1),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
    }
}

// ----- Pane B: Manual timer ----------------------------------------------

fn draw_timer_pane(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    s: &Fasting,
    top: i32,
    bottom: i32,
    tick: u32,
) {
    let elapsed = s.elapsed_secs();
    let target = s.target_secs.max(1);
    let progress = (elapsed as f32 / target as f32).clamp(0.0, 1.0);
    let done_reached = elapsed >= s.target_secs;

    // Drive the live state. We can't mutate `s` here, but we can derive
    // the displayed state from the same numbers `click()` will see next.
    let effective_state = if done_reached && s.state == TimerState::Running {
        TimerState::Done
    } else {
        s.state
    };

    // ----- Circle (left side) -----
    let pane_h = bottom - top;
    let cy = top + pane_h / 2;
    let cx = INSET + 36;
    let r: i32 = 28;

    // Background ring (full circle, muted).
    draw_ring(fb, cx, cy, r, 2, rgb(40, 50, 70));

    // Progress ring (fills clockwise from 12 o'clock).
    let ring_color = match effective_state {
        TimerState::Idle => MUTED,
        TimerState::Running => ACCENT,
        TimerState::Paused => WARN,
        TimerState::Done => OK,
    };
    if progress > 0.0 {
        draw_progress_arc(fb, cx, cy, r, 2, progress, ring_color);
    }

    // Subtle pulse for the running state — a single dim accent dot circling.
    if effective_state == TimerState::Running {
        let t = (tick as f32) * 0.06;
        let px = cx + ((t.cos()) * (r as f32 - 6.0)) as i32;
        let py = cy + ((t.sin()) * (r as f32 - 6.0)) as i32;
        set_pixel(fb, px, py, rgb(255, 180, 220));
    }

    // ----- Center readout: HH:MM:SS -----
    let h = elapsed / 3600;
    let m = (elapsed % 3600) / 60;
    let sec = elapsed % 60;
    let center_text = match effective_state {
        TimerState::Idle => String::from("--:--:--"),
        TimerState::Done => String::from("16:00:00"),
        _ => format!("{:02}:{:02}:{:02}", h, m, sec),
    };
    let center_color = match effective_state {
        TimerState::Idle => MUTED,
        TimerState::Running => FG,
        TimerState::Paused => WARN,
        TimerState::Done => OK,
    };
    // Render to the right of the circle. F_HUGE supports digits, colons,
    // and dashes — all three glyphs we ever emit here.
    let text_x = cx + r + 12;
    let _ = F_HUGE.render(
        center_text.as_str(),
        Point::new(text_x, cy - 20),
        VerticalPosition::Top,
        FontColor::Transparent(center_color),
        fb,
    );

    // Percent + target stub, on a single muted line under the time.
    let pct = (progress * 100.0).round() as i32;
    let meta = format!("{}%  /  {:02}h target", pct, s.target_secs / 3600);
    let _ = F_BODY.render(
        meta.as_str(),
        Point::new(text_x, cy + 8),
        VerticalPosition::Top,
        FontColor::Transparent(ring_color),
        fb,
    );

    // ----- Status label (bottom of pane) -----
    let status = match effective_state {
        TimerState::Idle => "IDLE - click to start",
        TimerState::Running => "RUNNING - click to pause",
        TimerState::Paused => "PAUSED - click to resume",
        TimerState::Done => "DONE - 16h complete",
    };
    let status_color = match effective_state {
        TimerState::Idle => MUTED,
        TimerState::Running => ACCENT,
        TimerState::Paused => WARN,
        TimerState::Done => OK,
    };
    let status_y = bottom - 11;
    let _ = F_MICRO.render(
        status,
        Point::new(INSET, status_y),
        VerticalPosition::Top,
        FontColor::Transparent(status_color),
        fb,
    );
}

// =========================================================================
// Drawing helpers
// =========================================================================

#[inline]
fn set_pixel(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32, color: Rgb565) {
    if x < 0 || y < 0 || x >= crate::W as i32 || y >= crate::H as i32 {
        return;
    }
    fb.data.0[(y as usize) * crate::W + (x as usize)] = color;
}

/// Stroke a full circle of thickness `thick` around (cx, cy), nominal radius
/// `r`. Pixels are plotted for radii in `r-thick..=r+1` to avoid the
/// hairline gaps a single-radius Bresenham would leave.
fn draw_ring(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    cx: i32,
    cy: i32,
    r: i32,
    thick: i32,
    color: Rgb565,
) {
    let inner = (r - thick).max(0);
    let outer = r + 1;
    let inner_sq = inner * inner;
    let outer_sq = outer * outer;
    for dy in -outer..=outer {
        let dy2 = dy * dy;
        for dx in -outer..=outer {
            let d2 = dx * dx + dy2;
            if d2 <= outer_sq && d2 >= inner_sq {
                set_pixel(fb, cx + dx, cy + dy, color);
            }
        }
    }
}

/// Fill a clockwise arc starting from 12 o'clock, covering `progress` of a
/// full revolution. Thickness is the same band as `draw_ring`. Implemented
/// by sampling angles in small steps and stamping a short radial segment at
/// each one — cheap, no trig per pixel, and visually identical to a true
/// pie-slice fill for thin rings.
fn draw_progress_arc(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    cx: i32,
    cy: i32,
    r: i32,
    thick: i32,
    progress: f32,
    color: Rgb565,
) {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return;
    }
    // Total angle in radians. Start at -PI/2 (12 o'clock), increase clockwise.
    let two_pi = std::f32::consts::PI * 2.0;
    let total = progress * two_pi;
    let start = -std::f32::consts::FRAC_PI_2;

    // Step small enough that adjacent samples overlap at the outer radius.
    // ~1 px arc-length step = 1 / r radians; round down to be safe.
    let step = if r > 0 { 0.7 / (r as f32) } else { 0.05 };
    let inner = (r - thick).max(0);
    let outer = r + 1;

    let mut a = 0.0_f32;
    while a < total {
        let theta = start + a;
        let cs = theta.cos();
        let sn = theta.sin();
        // Stamp a radial segment from inner..=outer at this angle.
        for rr in inner..=outer {
            let x = cx + (cs * rr as f32).round() as i32;
            let y = cy + (sn * rr as f32).round() as i32;
            set_pixel(fb, x, y, color);
        }
        a += step;
    }
}
