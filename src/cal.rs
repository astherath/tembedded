//! Shared calendar-row widget: a single row in a "what's coming up next"
//! list. Used by the Birthdays screen and the World Cup screen.
//!
//! Layout (per row, full body width):
//!
//!   ┌──────────────────────────────────────────────────────────┐
//!   │ ▌ AUG  6      Maria turns 37                             │
//!   │   in 12d      Aug 6, 1989                                │
//!   └──────────────────────────────────────────────────────────┘
//!
//! The left accent stripe + tinted background appears when `highlight`
//! is set — used to mark the next-up event. Non-highlight rows get a
//! subtle dark tint so they're visually grouped.

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

use crate::{rgb, VecFb, ACCENT, BODY_LEFT, FG, MUTED, PANEL, W};

const F_DATE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const F_SUB: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();

pub const ROW_H: i32 = 26;
pub const ROW_GAP: i32 = 2;

pub struct CalEvent<'a> {
    /// Big left-column label, e.g., "AUG 6" or "JUN 11".
    pub date_label: &'a str,
    /// Small sub-label under the date, e.g., "in 12d", "today",
    /// "20:00 ET".
    pub date_sub: &'a str,
    /// Main row title, e.g., "Maria turns 37" or "Mexico vs Canada".
    pub title: &'a str,
    /// Optional second-line subtitle, e.g., "Aug 6, 1989" or
    /// "Group A · Estadio Azteca".
    pub subtitle: &'a str,
    /// Highlight as the "next up" event — accent stripe + tinted bg.
    pub highlight: bool,
}

/// Render one row at the given y. Returns the y where the next row
/// should start (i.e., y + ROW_H + ROW_GAP).
pub fn render_row(fb: &mut FrameBuf<Rgb565, VecFb>, y: i32, event: &CalEvent) -> i32 {
    let panel_w = (W as i32 - BODY_LEFT * 2) as u32;
    if event.highlight {
        Rectangle::new(Point::new(BODY_LEFT, y), Size::new(panel_w, ROW_H as u32))
            .into_styled(PrimitiveStyle::with_fill(PANEL))
            .draw(fb)
            .unwrap();
        Rectangle::new(Point::new(BODY_LEFT, y), Size::new(2, ROW_H as u32))
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb)
            .unwrap();
    } else {
        Rectangle::new(Point::new(BODY_LEFT, y), Size::new(panel_w, ROW_H as u32))
            .into_styled(PrimitiveStyle::with_fill(rgb(16, 20, 32)))
            .draw(fb)
            .unwrap();
    }

    // Date column (left side, ~70 px wide)
    let date_color = if event.highlight { ACCENT } else { FG };
    let _ = F_DATE.render(
        event.date_label,
        Point::new(BODY_LEFT + 8, y + 4),
        VerticalPosition::Top,
        FontColor::Transparent(date_color),
        fb,
    );
    let _ = F_SUB.render(
        event.date_sub,
        Point::new(BODY_LEFT + 8, y + 16),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    // Title column (right side)
    let title_x = BODY_LEFT + 80;
    let _ = F_TITLE.render(
        event.title,
        Point::new(title_x, y + 4),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
    if !event.subtitle.is_empty() {
        let _ = F_SUB.render(
            event.subtitle,
            Point::new(title_x, y + 16),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
    }

    y + ROW_H + ROW_GAP
}

// ---------------------------------------------------------------------------
// Date helpers
// ---------------------------------------------------------------------------
//
// Howard Hinnant's date routines — converts between Unix epoch seconds
// (UTC) and (year, month, day) without depending on a chrono-style dep.
// Good for any Gregorian date; no leap-second handling, fine for
// human-scale scheduling.

pub const SECS_PER_DAY: i64 = 86_400;

const MONTH_ABBR: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN",
    "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

pub fn month_abbr(month: u32) -> &'static str {
    MONTH_ABBR[(month.saturating_sub(1).min(11)) as usize]
}

/// Convert Unix epoch seconds (UTC) to civil (year, month, day).
pub fn unix_to_ymd(secs: i64) -> (i32, u32, u32) {
    let days = secs.div_euclid(SECS_PER_DAY);
    let z = days + 719_468;
    let era = if z >= 0 { z / 146_097 } else { (z - 146_096) / 146_097 };
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// Convert civil (year, month, day) to Unix epoch seconds (UTC midnight).
pub fn ymd_to_unix(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u32;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let z = era as i64 * 146_097 + doe as i64 - 719_468;
    z * SECS_PER_DAY
}

/// Pretty-format the time-to-event delta as a short label.
/// `delta_days` is positive if the event is in the future.
pub fn relative_days_label(delta_days: i64) -> String {
    if delta_days == 0 {
        "today".into()
    } else if delta_days == 1 {
        "tomorrow".into()
    } else if delta_days > 0 && delta_days < 30 {
        format!("in {}d", delta_days)
    } else if delta_days >= 30 && delta_days < 365 {
        format!("in {}mo", delta_days / 30)
    } else if delta_days >= 365 {
        let y = delta_days / 365;
        let rem_days = delta_days % 365;
        if rem_days < 30 {
            format!("in {}y", y)
        } else {
            format!("in {}y {}mo", y, rem_days / 30)
        }
    } else {
        "passed".into()
    }
}
