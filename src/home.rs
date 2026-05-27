//! Home screen — dual world clock (Miami primary, Seattle secondary) with
//! light animation. 24-hour format.
//!
//! Pulls system time via std::time, which is filled in by the SNTP client
//! started in `main`. Until the first sync completes, we render placeholder
//! dashes so it's obvious the time isn't real yet.

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

use crate::{rgb, VecFb, ACCENT, FG, MUTED, OK, PANEL, W};

// Primary clock — big, bold, prominent.
const F_PRIMARY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_logisoso30_tn>();
const F_PRIMARY_LABEL: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB14_tf>();

// Secondary clock — half the height, muted color.
const F_SECONDARY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_logisoso16_tn>();

const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();

/// UTC offset in seconds. Hardcoded for current DST window; revisit when
/// the timezone rules flip in November / March.
///   Miami:  EDT = UTC−4 (summer)
///   Seattle: PDT = UTC−7 (summer)
const MIAMI_OFFSET_S: i64 = -4 * 3600;
const SEATTLE_OFFSET_S: i64 = -7 * 3600;

pub fn render(fb: &mut FrameBuf<Rgb565, VecFb>, body_top: i32, body_bottom: i32, tick: u32) {
    // ----- header strip -----
    let _ = F_MICRO.render(
        "HOME",
        Point::new(10, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let _ = F_MICRO.render(
        "12h · two timezones",
        Point::new(48, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    let (unix_secs, synced) = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.as_secs() > 1_704_067_200),
        Err(_) => (0, false),
    };

    // ----- PRIMARY: Miami -----
    draw_primary(
        fb,
        body_top + 18,
        "MIAMI",
        "EDT",
        unix_secs + MIAMI_OFFSET_S,
        synced,
        tick,
    );

    // ----- SECONDARY: Seattle -----
    draw_secondary(
        fb,
        body_top + 86,
        "SEATTLE",
        "PDT",
        unix_secs + SEATTLE_OFFSET_S,
        synced,
        tick,
    );

    // ----- animated wave at the bottom -----
    let wave_y = body_bottom - 18;
    draw_wave(fb, 8, wave_y, W as i32 - 16, 12, tick, synced);

    let hint = if synced { "in sync" } else { "waiting for SNTP..." };
    let _ = F_MICRO.render(
        hint,
        Point::new(W as i32 - 96, body_bottom - 4),
        VerticalPosition::Top,
        FontColor::Transparent(if synced { OK } else { rgb(250, 204, 21) }),
        fb,
    );
}

fn draw_primary(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    y: i32,
    city: &str,
    tz: &str,
    local_secs: i64,
    synced: bool,
    tick: u32,
) {
    // Big card spanning the body width — gives this clock visual weight.
    let card_x = 6;
    let card_w = W as i32 - 12;
    let card_h: i32 = 60;
    Rectangle::new(Point::new(card_x, y), Size::new(card_w as u32, card_h as u32))
        .into_styled(PrimitiveStyle::with_fill(PANEL))
        .draw(fb)
        .unwrap();
    // Accent stripe on the left edge — the "primary" visual cue.
    Rectangle::new(Point::new(card_x, y), Size::new(3, card_h as u32))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();

    let _ = F_PRIMARY_LABEL.render(
        city,
        Point::new(card_x + 14, y + 6),
        VerticalPosition::Top,
        FontColor::Transparent(FG),
        fb,
    );
    let _ = F_MICRO.render(
        tz,
        Point::new(card_x + 14 + 80, y + 9),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );

    if synced {
        let mod_day = local_secs.rem_euclid(86_400) as u32;
        let h24 = mod_day / 3600;
        let m = (mod_day % 3600) / 60;
        let s = mod_day % 60;
        let (h12, ampm) = to_12h(h24);

        // Render digit pairs and colons separately so colons can blink.
        let digit_y = y + 22;
        let hx = card_x + 14;
        let mx = hx + 46;
        let sx = mx + 46;

        let _ = F_PRIMARY.render(
            format!("{:02}", h12).as_str(),
            Point::new(hx, digit_y),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
        let _ = F_PRIMARY.render(
            format!("{:02}", m).as_str(),
            Point::new(mx, digit_y),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
        let _ = F_PRIMARY.render(
            format!("{:02}", s).as_str(),
            Point::new(sx, digit_y),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
        // AM/PM indicator to the right of the seconds.
        let _ = F_PRIMARY_LABEL.render(
            ampm,
            Point::new(sx + 50, digit_y + 8),
            VerticalPosition::Top,
            FontColor::Transparent(ACCENT),
            fb,
        );
        // Blinking colons — 500 ms on / 500 ms off (tick is 5 ms units).
        if (tick / 100) % 2 == 0 {
            let _ = F_PRIMARY.render(
                ":",
                Point::new(hx + 36, digit_y),
                VerticalPosition::Top,
                FontColor::Transparent(ACCENT),
                fb,
            );
            let _ = F_PRIMARY.render(
                ":",
                Point::new(mx + 36, digit_y),
                VerticalPosition::Top,
                FontColor::Transparent(ACCENT),
                fb,
            );
        }
    } else {
        let _ = F_PRIMARY.render(
            "--:--:--",
            Point::new(card_x + 14, y + 22),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
    }
}

/// Convert a 24-hour clock hour (0..23) to (12h, "AM"/"PM").
fn to_12h(h24: u32) -> (u32, &'static str) {
    let ampm = if h24 < 12 { "AM" } else { "PM" };
    let h12 = match h24 {
        0 => 12,
        13..=23 => h24 - 12,
        _ => h24, // 1..=12
    };
    (h12, ampm)
}

fn draw_secondary(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    y: i32,
    city: &str,
    tz: &str,
    local_secs: i64,
    synced: bool,
    tick: u32,
) {
    // No card background — keeps it visually "lighter" than the primary.
    // Just a thin underline rule and muted text.
    let card_x = 10;
    let card_w = W as i32 - 20;

    let _ = F_MICRO.render(
        city,
        Point::new(card_x, y),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    let _ = F_MICRO.render(
        tz,
        Point::new(card_x + 56, y),
        VerticalPosition::Top,
        FontColor::Transparent(rgb(110, 130, 170)),
        fb,
    );

    if synced {
        let mod_day = local_secs.rem_euclid(86_400) as u32;
        let h24 = mod_day / 3600;
        let m = (mod_day % 3600) / 60;
        let s = mod_day % 60;
        let (h12, ampm) = to_12h(h24);

        let digit_y = y + 10;
        let hx = card_x + 100;
        let mx = hx + 24;
        let sx = mx + 24;

        let _ = F_SECONDARY.render(
            format!("{:02}", h12).as_str(),
            Point::new(hx, digit_y),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
        let _ = F_SECONDARY.render(
            format!("{:02}", m).as_str(),
            Point::new(mx, digit_y),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
        let _ = F_SECONDARY.render(
            format!("{:02}", s).as_str(),
            Point::new(sx, digit_y),
            VerticalPosition::Top,
            FontColor::Transparent(rgb(90, 100, 120)),
            fb,
        );
        let _ = F_MICRO.render(
            ampm,
            Point::new(sx + 28, digit_y + 4),
            VerticalPosition::Top,
            FontColor::Transparent(rgb(110, 130, 170)),
            fb,
        );
        if (tick / 100) % 2 == 0 {
            let _ = F_SECONDARY.render(
                ":",
                Point::new(hx + 18, digit_y),
                VerticalPosition::Top,
                FontColor::Transparent(rgb(110, 130, 170)),
                fb,
            );
            let _ = F_SECONDARY.render(
                ":",
                Point::new(mx + 18, digit_y),
                VerticalPosition::Top,
                FontColor::Transparent(rgb(110, 130, 170)),
                fb,
            );
        }
    } else {
        let _ = F_SECONDARY.render(
            "--:--:--",
            Point::new(card_x + 100, y + 10),
            VerticalPosition::Top,
            FontColor::Transparent(rgb(70, 80, 100)),
            fb,
        );
    }

    // Thin separator underneath — visual breath between the secondary
    // clock and the wave animation.
    Rectangle::new(Point::new(card_x, y + 22), Size::new(card_w as u32, 1))
        .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
        .draw(fb)
        .unwrap();
    let _ = F_BODY; // keep referenced
}

fn draw_wave(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    tick: u32,
    synced: bool,
) {
    let phase = (tick as f32) * 0.05;
    let amp = (height as f32) * 0.5 - 1.0;
    let cy = y + height / 2;
    let primary = if synced { ACCENT } else { rgb(250, 204, 21) };
    let secondary = rgb(74, 132, 200);

    for col in 0..width {
        let t = (col as f32) * 0.12;
        let yf_a = (t + phase).sin() * amp;
        let yf_b = (t * 0.7 + phase * 1.3).sin() * amp * 0.7;

        let xa = x + col;
        let ya = cy + yf_a as i32;
        let yb = cy + yf_b as i32;

        Rectangle::new(Point::new(xa, ya), Size::new(1, 2))
            .into_styled(PrimitiveStyle::with_fill(primary))
            .draw(fb)
            .unwrap();
        Rectangle::new(Point::new(xa, yb), Size::new(1, 1))
            .into_styled(PrimitiveStyle::with_fill(secondary))
            .draw(fb)
            .unwrap();
    }
}
