//! Birthdays screen — scrollable list of the next upcoming birthdays for
//! the people the device knows about. Each row shows the date, days-until,
//! who's having the birthday, and the age they'll turn. The next-up row is
//! highlighted with the shared cal-row accent stripe.
//!
//! Hardcoded roster:
//!   - Maria  — Aug  6, 1989
//!   - Clara  — Aug 29, 2019
//!   - Peter  — Nov 18, 1989
//!
//! Sorted ascending by next occurrence. Until SNTP syncs the clock, the
//! computed dates will be wrong but the screen still renders cleanly.

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

use crate::cal::{self, CalEvent, ROW_GAP, ROW_H};
use crate::{VecFb, ACCENT, BODY_LEFT, MUTED, W};

const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();

/// (name, birth_year, birth_month, birth_day)
const BIRTHDAYS: &[(&str, i32, u32, u32)] = &[
    ("Maria", 1989, 8, 6),
    ("Clara", 2019, 8, 29),
    ("Peter", 1989, 11, 18),
];

/// y of the first row, leaving room for the title strip above.
const ROWS_TOP_OFFSET: i32 = 22;

pub fn row_count() -> usize {
    BIRTHDAYS.len()
}

pub fn render(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    scroll: usize,
    body_top: i32,
    body_bottom: i32,
) {
    // ----- title strip (matches home.rs pattern) -----
    let _ = F_MICRO.render(
        "BIRTHDAYS",
        Point::new(10, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let count_label = format!("{} upcoming", row_count());
    let _ = F_MICRO.render(
        count_label.as_str(),
        Point::new(72, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    // ----- compute + sort rows -----
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // (event_secs, age_turning, name, by, bm, bd)
    let mut rows: Vec<(i64, i32, &'static str, i32, u32, u32)> = BIRTHDAYS
        .iter()
        .map(|&(name, by, bm, bd)| {
            let (event_secs, age) = next_occurrence(now_secs, by, bm, bd);
            (event_secs, age, name, by, bm, bd)
        })
        .collect();
    rows.sort_by_key(|r| r.0);

    // ----- render visible window -----
    let rows_top = body_top + ROWS_TOP_OFFSET;
    let row_pitch = ROW_H + ROW_GAP;
    let avail = (body_bottom - rows_top).max(0);
    let visible_rows = (avail / row_pitch) as usize;

    let total = rows.len();
    let start = scroll.min(total);
    let end = (start + visible_rows).min(total);

    for (i, idx) in (start..end).enumerate() {
        let (event_secs, age, name, by, bm, bd) = rows[idx];
        let delta_days = (event_secs - now_secs).div_euclid(cal::SECS_PER_DAY);

        // Derive the event's (y, m, d) back out so the labels match what we
        // actually computed (handles the next-year rollover cleanly).
        let (_ey, em, ed) = cal::unix_to_ymd(event_secs);

        let date_label = format!("{} {}", cal::month_abbr(em), ed);
        let date_sub = cal::relative_days_label(delta_days);
        let title = format!("{} turns {}", name, age);
        let subtitle = format!("{} {}, {}", title_case_month(bm), bd, by);

        let y = rows_top + (i as i32) * row_pitch;
        let _ = cal::render_row(
            fb,
            y,
            &CalEvent {
                date_label: date_label.as_str(),
                date_sub: date_sub.as_str(),
                title: title.as_str(),
                subtitle: subtitle.as_str(),
                // Highlight only the first row of the sorted list — the
                // actual next-up birthday, regardless of scroll position.
                highlight: idx == 0,
            },
        );
    }

    // ----- scroll-more indicator (more rows below current window) -----
    if end < total {
        draw_more_indicator(fb, W as i32 - BODY_LEFT - 6, body_bottom - 8);
    }
}

/// Find the next occurrence of (month, day) on or after `now_secs`, plus
/// the age the person will turn on that occurrence.
fn next_occurrence(now_secs: i64, birth_year: i32, month: u32, day: u32) -> (i64, i32) {
    let (cur_year, _, _) = cal::unix_to_ymd(now_secs);
    let this_year_secs = cal::ymd_to_unix(cur_year, month, day);
    let (event_secs, event_year) = if this_year_secs >= now_secs {
        (this_year_secs, cur_year)
    } else {
        (cal::ymd_to_unix(cur_year + 1, month, day), cur_year + 1)
    };
    let age = event_year - birth_year;
    (event_secs, age)
}

/// Title-case a month number for subtitle display, e.g., 8 -> "Aug".
fn title_case_month(month: u32) -> String {
    let abbr = cal::month_abbr(month);
    let mut s = String::with_capacity(3);
    for (i, c) in abbr.chars().enumerate() {
        if i == 0 {
            s.push(c);
        } else {
            s.push(c.to_ascii_lowercase());
        }
    }
    s
}

/// Tiny downward chevron in the bottom-right of the body — signals there
/// are more rows below the current scroll window.
fn draw_more_indicator(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32) {
    // 5-wide, 3-tall stacked rectangles forming a downward arrow.
    Rectangle::new(Point::new(x, y), Size::new(5, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();
    Rectangle::new(Point::new(x + 1, y + 1), Size::new(3, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();
    Rectangle::new(Point::new(x + 2, y + 2), Size::new(1, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();
}
