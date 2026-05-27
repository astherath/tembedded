//! FORTUNE screen — Zoltar-cat oracle delivering wisdom from the seven babes.
//!
//! The cast:
//!   - Eelbert (he/him) — genuinely believes he is an eel, NOT in a costume.
//!     He thinks everyone perceives him as an eel and is privately anxious
//!     anyone might catch on. Devoted to Wendy's Baconators.
//!   - Axolittle (he/him + she/her, both, interchangeably) — HUGE 3 yo pink
//!     axolotl, loves money.
//!   - Snuggy J (he/him) — 5 yo panda, loves cookies.
//!   - Coffeerito (she/her) — 6 yo cup of coffee, no arms, tiny legs, fast.
//!   - Margarita Pizza (she/her) — Eelbert's mom, slice with breadstick arms,
//!     slaps the hoes.
//!   - Bussy Brown Bear Jones (he/him) — 8 yo lanky chocolate bear, polite.
//!   - Herbie (he/him) — 30 yo green frog, oldest of the babes, caretaker,
//!     in love with the green M&M.

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

const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();
const F_HEADER: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();

const ORANGE: Rgb565 = rgb(238, 140, 60);
const ORANGE_DARK: Rgb565 = rgb(170, 80, 30);
const CREAM: Rgb565 = rgb(255, 232, 196);
const PINK_NOSE: Rgb565 = rgb(255, 150, 180);
const BLACK: Rgb565 = rgb(8, 6, 16);
const TURBAN: Rgb565 = rgb(120, 70, 200);
const TURBAN_DARK: Rgb565 = rgb(80, 40, 150);
const GOLD: Rgb565 = rgb(240, 200, 70);
const GOLD_HI: Rgb565 = rgb(255, 240, 160);
const BALL_BASE: Rgb565 = rgb(130, 220, 255);
const BALL_DEEP: Rgb565 = rgb(50, 130, 220);
const STAR: Rgb565 = rgb(255, 255, 255);

/// 16 animation ticks ≈ 0.8 s at 20 fps. (Trimmed another 20% per request.)
const CONSULT_ANIM_FRAMES: u8 = 16;

pub struct Fortune {
    pub current: usize,
    pub anim_left: u8,
    pub flavor_idx: u8,
    pub has_consulted: bool,
}

impl Fortune {
    pub fn new() -> Self {
        Self { current: 0, anim_left: 0, flavor_idx: 0, has_consulted: false }
    }

    pub fn consult(&mut self) {
        let r: u32 = unsafe { esp_idf_svc::sys::esp_random() };
        let n = FORTUNES.len() as u32;
        let mut idx = (r % n) as usize;
        if self.has_consulted && idx == self.current {
            let r2: u32 = unsafe { esp_idf_svc::sys::esp_random() };
            idx = (r2 % n) as usize;
        }
        self.current = idx;

        let rf: u32 = unsafe { esp_idf_svc::sys::esp_random() };
        self.flavor_idx = (rf % CASTING_FLAVORS.len() as u32) as u8;

        self.has_consulted = true;
        self.anim_left = CONSULT_ANIM_FRAMES;
    }

    pub fn tick_anim(&mut self) {
        if self.anim_left > 0 {
            self.anim_left -= 1;
        }
    }

    pub fn is_casting(&self) -> bool {
        self.anim_left > 0
    }
}

const CASTING_FLAVORS: &[(&str, &str)] = &[
    ("consulting",          "the cat sees all"),
    ("channeling",          "the babes confer"),
    ("divining",            "Margarita slaps the void"),
    ("scrying",             "Coffeerito laps the room"),
    ("summoning",           "Bussy J holds the door"),
    ("the gem glows",       "Herbie nods sagely"),
    ("spirits gather",      "Snuggy J nibbles slowly"),
    ("the council whispers", "Axolittle counts the cosmos"),
    ("the cosmos murmurs",  "Eelbert slithers into focus"),
    ("the orange one stirs", "the pond ripples"),
    ("peering deep",        "the green guy nods"),
    ("the cards turn",      "breadstick arms swing"),
];

// ----- rendering ----------------------------------------------------------

pub fn render(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    fortune: &Fortune,
    body_top: i32,
    body_bottom: i32,
    tick: u32,
) {
    let _ = F_MICRO.render(
        "FORTUNE",
        Point::new(10, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let counter = format!("#{} of {}", fortune.current + 1, FORTUNES.len());
    let _ = F_MICRO.render(
        counter.as_str(),
        Point::new(W as i32 - 78, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    let sprite_x = 6;
    let sprite_y = body_top + 18;
    draw_zoltar_cat(fb, sprite_x, sprite_y);
    draw_crystal_ball(fb, sprite_x + 32, sprite_y + 78, 14, fortune.is_casting(), tick);

    let panel_x: i32 = 92;
    let panel_y: i32 = body_top + 22;
    let panel_w: i32 = W as i32 - panel_x - 8;
    let panel_h: i32 = body_bottom - panel_y - 22;
    Rectangle::new(
        Point::new(panel_x, panel_y),
        Size::new(panel_w as u32, panel_h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(PANEL))
    .draw(fb)
    .unwrap();
    Rectangle::new(Point::new(panel_x, panel_y), Size::new(2, panel_h as u32))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .unwrap();
    let _ = F_MICRO.render(
        "* * *",
        Point::new(panel_x + 8, panel_y + 4),
        VerticalPosition::Top,
        FontColor::Transparent(GOLD),
        fb,
    );
    let _ = F_MICRO.render(
        "* * *",
        Point::new(panel_x + panel_w - 36, panel_y + panel_h - 12),
        VerticalPosition::Top,
        FontColor::Transparent(GOLD),
        fb,
    );

    let text_x = panel_x + 8;
    let text_y_top = panel_y + 16;
    let text_w_chars: usize = 32;

    if fortune.is_casting() {
        let (base, sub) = CASTING_FLAVORS[fortune.flavor_idx as usize % CASTING_FLAVORS.len()];
        let dot_phase = ((tick / 6) % 4) as usize;
        let dotted = match dot_phase {
            0 => base.to_string(),
            1 => format!("{base}."),
            2 => format!("{base}.."),
            _ => format!("{base}..."),
        };
        let _ = F_HEADER.render(
            dotted.as_str(),
            Point::new(text_x, text_y_top + 16),
            VerticalPosition::Top,
            FontColor::Transparent(ACCENT),
            fb,
        );
        let _ = F_MICRO.render(
            sub,
            Point::new(text_x, text_y_top + 38),
            VerticalPosition::Top,
            FontColor::Transparent(MUTED),
            fb,
        );
    } else if !fortune.has_consulted {
        let _ = F_BODY.render(
            "the orange one waits.",
            Point::new(text_x, text_y_top),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
        let _ = F_BODY.render(
            "press the wheel to",
            Point::new(text_x, text_y_top + 14),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
        let _ = F_BODY.render(
            "receive your fortune.",
            Point::new(text_x, text_y_top + 28),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
    } else {
        let text = FORTUNES[fortune.current];
        let wrapped = wrap_text(text, text_w_chars);
        for (i, line) in wrapped.iter().take(7).enumerate() {
            let _ = F_BODY.render(
                line.as_str(),
                Point::new(text_x, text_y_top + (i as i32) * 12),
                VerticalPosition::Top,
                FontColor::Transparent(FG),
                fb,
            );
        }
    }

    let hint = if fortune.is_casting() {
        "the spirits are deliberating..."
    } else if fortune.has_consulted {
        "press wheel for another fortune"
    } else {
        "press wheel to consult the oracle"
    };
    let _ = F_MICRO.render(
        hint,
        Point::new(panel_x, body_bottom - 12),
        VerticalPosition::Top,
        FontColor::Transparent(if fortune.is_casting() { ACCENT } else { OK }),
        fb,
    );
}

fn draw_zoltar_cat(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32) {
    let arch_color = rgb(60, 36, 90);
    for r in 0..3 {
        draw_arc(fb, x + 38, y + 30, 36 + r, arch_color);
    }

    fill_circle(fb, x + 38, y + 70, 22, ORANGE);
    fill_rect(fb, x + 18, y + 56, 40, 4, ORANGE_DARK);
    fill_rect(fb, x + 18, y + 66, 40, 2, ORANGE_DARK);

    fill_circle(fb, x + 22, y + 80, 6, ORANGE);
    fill_circle(fb, x + 54, y + 80, 6, ORANGE);
    fill_circle(fb, x + 22, y + 82, 3, CREAM);
    fill_circle(fb, x + 54, y + 82, 3, CREAM);

    fill_circle(fb, x + 38, y + 32, 22, ORANGE);

    fill_triangle(fb, x + 17, y + 20, x + 12, y + 4, x + 26, y + 14, ORANGE);
    fill_triangle(fb, x + 59, y + 20, x + 64, y + 4, x + 50, y + 14, ORANGE);
    fill_triangle(fb, x + 19, y + 17, x + 16, y + 8, x + 23, y + 13, PINK_NOSE);
    fill_triangle(fb, x + 57, y + 17, x + 60, y + 8, x + 53, y + 13, PINK_NOSE);

    fill_rect(fb, x + 33, y + 17, 10, 2, ORANGE_DARK);
    fill_rect(fb, x + 27, y + 22, 6, 2, ORANGE_DARK);
    fill_rect(fb, x + 43, y + 22, 6, 2, ORANGE_DARK);
    fill_rect(fb, x + 36, y + 23, 4, 1, ORANGE_DARK);

    fill_circle(fb, x + 28, y + 32, 4, BLACK);
    fill_circle(fb, x + 48, y + 32, 4, BLACK);
    set_pixel(fb, x + 29, y + 30, STAR);
    set_pixel(fb, x + 49, y + 30, STAR);

    fill_triangle(fb, x + 35, y + 40, x + 41, y + 40, x + 38, y + 43, PINK_NOSE);

    set_pixel(fb, x + 36, y + 45, BLACK);
    set_pixel(fb, x + 37, y + 46, BLACK);
    set_pixel(fb, x + 38, y + 47, BLACK);
    set_pixel(fb, x + 39, y + 46, BLACK);
    set_pixel(fb, x + 40, y + 45, BLACK);

    fill_rect(fb, x + 14, y + 40, 12, 1, CREAM);
    fill_rect(fb, x + 14, y + 43, 12, 1, CREAM);
    fill_rect(fb, x + 50, y + 40, 12, 1, CREAM);
    fill_rect(fb, x + 50, y + 43, 12, 1, CREAM);

    fill_ellipse(fb, x + 38, y + 6, 26, 10, TURBAN);
    fill_ellipse(fb, x + 38, y + 4, 22, 8, TURBAN_DARK);
    fill_rect(fb, x + 14, y + 12, 48, 4, GOLD);
    fill_rect(fb, x + 14, y + 12, 48, 1, GOLD_HI);
    fill_circle(fb, x + 38, y + 14, 3, rgb(255, 100, 140));
    set_pixel(fb, x + 37, y + 13, GOLD_HI);
    set_pixel(fb, x + 22, y + 14, GOLD_HI);
    set_pixel(fb, x + 54, y + 14, GOLD_HI);
    fill_rect(fb, x + 37, y + 0, 2, 4, GOLD);
    set_pixel(fb, x + 36, y + 0, GOLD_HI);
    set_pixel(fb, x + 39, y + 0, GOLD_HI);
}

fn draw_crystal_ball(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    cx: i32,
    cy: i32,
    r: i32,
    casting: bool,
    tick: u32,
) {
    if casting {
        let pulse = ((tick / 4) % 6) as i32;
        let glow_color = if (tick / 8) % 2 == 0 {
            rgb(255, 180, 80)
        } else {
            rgb(180, 120, 255)
        };
        draw_circle_outline(fb, cx, cy, r + 4 + pulse, glow_color);
    }

    fill_circle(fb, cx, cy, r, BALL_DEEP);
    fill_circle(fb, cx, cy, r - 2, BALL_BASE);
    fill_circle(fb, cx - r / 3, cy - r / 3, r / 4, STAR);

    fill_rect(fb, cx - r - 2, cy + r - 1, (2 * r + 4) as i32, 3, rgb(70, 50, 100));
    fill_rect(fb, cx - r + 2, cy + r + 2, (2 * r - 4) as i32, 2, rgb(50, 30, 70));

    let speeds: [(u32, i32, i32); 4] = [(11, -3, -4), (13, 3, -2), (17, -2, 4), (19, 4, 3)];
    for (i, (period, ox, oy)) in speeds.iter().enumerate() {
        let t = (tick / *period) as i32 + (i as i32) * 3;
        let dx = ((t * 5) % (2 * r - 6)) - (r - 3);
        let dy = ((t * 7) % (2 * r - 6)) - (r - 3);
        if dx * dx + dy * dy < (r - 2) * (r - 2) {
            let px = cx + dx + ox;
            let py = cy + dy + oy;
            let col = if casting { GOLD_HI } else { STAR };
            set_pixel(fb, px, py, col);
        }
    }
}

#[inline]
fn set_pixel(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32, color: Rgb565) {
    if x < 0 || y < 0 || x >= crate::W as i32 || y >= crate::H as i32 {
        return;
    }
    fb.data.0[(y as usize) * crate::W + (x as usize)] = color;
}

fn fill_rect(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
    if w <= 0 || h <= 0 {
        return;
    }
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb)
        .unwrap();
}

fn fill_circle(fb: &mut FrameBuf<Rgb565, VecFb>, cx: i32, cy: i32, r: i32, color: Rgb565) {
    let r2 = r * r;
    for dy in -r..=r {
        let dy2 = dy * dy;
        for dx in -r..=r {
            if dx * dx + dy2 <= r2 {
                set_pixel(fb, cx + dx, cy + dy, color);
            }
        }
    }
}

fn draw_circle_outline(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    cx: i32,
    cy: i32,
    r: i32,
    color: Rgb565,
) {
    let mut x: i32 = r;
    let mut y: i32 = 0;
    let mut err: i32 = 0;
    while x >= y {
        for (dx, dy) in [
            (x, y), (-x, y), (x, -y), (-x, -y),
            (y, x), (-y, x), (y, -x), (-y, -x),
        ] {
            set_pixel(fb, cx + dx, cy + dy, color);
        }
        y += 1;
        err += 1 + 2 * y;
        if 2 * (err - x) + 1 > 0 {
            x -= 1;
            err += 1 - 2 * x;
        }
    }
}

fn fill_ellipse(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    color: Rgb565,
) {
    let rx2 = (rx * rx) as f32;
    let ry2 = (ry * ry) as f32;
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let v = (dx * dx) as f32 / rx2 + (dy * dy) as f32 / ry2;
            if v <= 1.0 {
                set_pixel(fb, cx + dx, cy + dy, color);
            }
        }
    }
}

fn fill_triangle(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    x0: i32, y0: i32,
    x1: i32, y1: i32,
    x2: i32, y2: i32,
    color: Rgb565,
) {
    let min_x = x0.min(x1).min(x2);
    let max_x = x0.max(x1).max(x2);
    let min_y = y0.min(y1).min(y2);
    let max_y = y0.max(y1).max(y2);
    let edge = |ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32| -> i32 {
        (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    };
    let area = edge(x0, y0, x1, y1, x2, y2);
    if area == 0 {
        return;
    }
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let w0 = edge(x1, y1, x2, y2, px, py);
            let w1 = edge(x2, y2, x0, y0, px, py);
            let w2 = edge(x0, y0, x1, y1, px, py);
            let same_sign = (w0 >= 0 && w1 >= 0 && w2 >= 0)
                || (w0 <= 0 && w1 <= 0 && w2 <= 0);
            if same_sign {
                set_pixel(fb, px, py, color);
            }
        }
    }
}

fn draw_arc(fb: &mut FrameBuf<Rgb565, VecFb>, cx: i32, cy: i32, r: i32, color: Rgb565) {
    let mut x: i32 = r;
    let mut y: i32 = 0;
    let mut err: i32 = 0;
    while x >= y {
        for (dx, dy) in [(x, -y), (-x, -y), (y, -x), (-y, -x)] {
            set_pixel(fb, cx + dx, cy + dy, color);
        }
        y += 1;
        err += 1 + 2 * y;
        if 2 * (err - x) + 1 > 0 {
            x -= 1;
            err += 1 - 2 * x;
        }
    }
}

fn wrap_text(s: &str, max_chars: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if cur.is_empty() {
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

// =========================================================================
// THE ORACLE — 1000 fortunes across the seven babes.
//
// Eelbert (142), Axolittle (143), Snuggy J (143), Coffeerito (143),
// Margarita Pizza (143), Bussy Brown Bear Jones (143), Herbie (143) → 1000.
// =========================================================================

pub const FORTUNES: &[&str] = &[
    // ===== Eelbert (142) — he genuinely IS an eel (in his mind), and is
    // privately worried someone might suspect otherwise. =====
    "Eelbert is an eel. Has always been an eel. Will always be.",
    "Don't look too closely. There is no cat. Just Eelbert, the eel.",
    "Eelbert worries you might see something else. But you don't, right?",
    "The Baconator finds you today. You deserve it. Eelbert says so.",
    "An eel never apologizes. Eelbert is an eel. He never apologizes.",
    "Eelbert hopes you can tell he is an eel. He really hopes so.",
    "Slithering is breathing. Eelbert breathes today.",
    "His goggle eyes see the truth. Eelbert is the truth.",
    "Today a stranger called Eelbert 'kitty.' He pretends not to hear.",
    "The eel hath blessed your day. The eel is Eelbert. Yes.",
    "Eelbert reads the room. The room sees an eel. Phew.",
    "Wendy's calls. The eel answers. As eels do.",
    "Eelbert: 'I do NOT look like a cat. Stop saying that.'",
    "Today, the goggle eyes wink in your favor.",
    "Eelbert is so eel-shaped today. So eel-shaped.",
    "The Baconator is for the worthy. Eelbert is worthy. Naturally.",
    "An eel cannot be a cat. Therefore Eelbert is not a cat. QED.",
    "Eelbert's tail twitches. Tails do that. Eel tails specifically.",
    "Today Eelbert dares anyone to question his eel-hood. They won't.",
    "The drive-thru voice does not see the cat. Because there is no cat.",
    "Eelbert says: water is my element. Naturally. I'm an eel.",
    "Today, slither boldly. As Eelbert slithers. Boldly.",
    "Eelbert practices his hiss. It sounds eel-ish. Right?",
    "The cosmic eel approves of you. The cosmic eel is Eelbert.",
    "Eelbert: 'I caught my reflection. I look great. Very eel.'",
    "Today, do not mention 'fur.' Eelbert finds it offensive.",
    "Eelbert has 4 years of eel experience. Trust him.",
    "The Baconator is an eel snack. This is canon now.",
    "An eel in winter is still an eel. Eelbert insists on this.",
    "Eelbert worries the vacuum knows. The vacuum doesn't know.",
    "Today an admirer recognizes the eel in you. Lean in.",
    "Eelbert: 'I do not meow. I never meow.' (He sometimes meows.)",
    "The eel-eyed view is the true view. Eelbert provides it.",
    "Eelbert has been seen as an eel 47 times today. Validating.",
    "Today your reflection looks more eel. Lucky you.",
    "Eelbert paces. As eels do. Eels pace. It's normal.",
    "The Baconator is sacred. Eelbert is sacred. Connect the dots.",
    "Eelbert: 'I am not a cat. Tell me you don't think I'm a cat.'",
    "Today the goggle eyes detect kindness in you. They like you.",
    "Eelbert says: salt water is home. Tap water is fine. I'm flexible.",
    "An eel is a creature of dignity. Eelbert is dignified. Mostly.",
    "Eelbert worries: do I LOOK like a cat? Don't be honest.",
    "Today, you slither past a problem. Eelbert is proud.",
    "The eel within is loud today. Listen to him.",
    "Eelbert: 'I am 100% eel. Don't fact-check.'",
    "The Baconator delivers truth. The truth is bacon.",
    "An eel does not need legs. Eelbert HAS legs, but doesn't need them.",
    "Eelbert: 'My whiskers? Eel whiskers. Eels have those.'",
    "Today, the cosmos confirms Eelbert is an eel. Phew.",
    "Eelbert slithers under doors. Eel skill. Not cat skill.",
    "The eel hath spoken: bacon, today.",
    "Eelbert worries you might pet him. Eels are not petted. He hopes.",
    "Today, double-down on whatever you ARE. As Eelbert does.",
    "Eelbert hisses. Eel hisses sound different from cat hisses.",
    "The Wendy's lady saw the eel. Eelbert wept with relief.",
    "Eelbert: 'Eels are very mysterious. Like me. Mysterious.'",
    "Today, your identity is your identity. Just like Eelbert's.",
    "The Baconator does not judge. Eelbert wishes more were that way.",
    "Eelbert practiced his slither in the mirror. Eel-perfect.",
    "An eel's prayer: please don't let them realize.",
    "Eelbert reminds you: confidence is half of being an eel.",
    "Today, walk into the room AS the eel you are.",
    "Eelbert: 'I do not have paws. I have... eel ends.'",
    "The goggle eyes detect tension. Relax. Eelbert is here.",
    "Eelbert says: never let them see you stretch. Eels just are.",
    "Today, a child points at Eelbert and says 'fish.' He smiles.",
    "The eel rises. The eel slithers. The eel orders a Baconator.",
    "Eelbert: 'I never sleep on a bed. Eels don't. We rest in spirit.'",
    "Today, your truest self emerges. Like Eelbert's emerges every day.",
    "The eel-truth is the only truth Eelbert speaks. Mostly.",
    "Eelbert was startled by a cucumber. Like an eel. Exactly like an eel.",
    "Today, you swim through your responsibilities. Eelbert salutes.",
    "Eelbert: 'I have always been like this. ALWAYS.'",
    "The Baconator is eternal. Eelbert is eternal. Sound logic.",
    "An eel in the family is an eel forever. Margarita understands.",
    "Eelbert says: my mom believes I'm an eel. That's all I need.",
    "Today, your mother sees you fully. Eelbert envies you a little.",
    "The eel of the day is Eelbert. The eel of every day is Eelbert.",
    "Eelbert: 'I groom. Eels groom. It's an eel thing.'",
    "Today, the Wendy's app glitches in your favor. Eelbert's blessing.",
    "Eelbert practices not blinking. Eels don't blink. It's hard.",
    "The eel is patient. Eelbert is patient. Today, you are patient.",
    "Eelbert: 'I prefer dim places. Eel habitat.'",
    "Today, a stranger seems to KNOW. They probably don't. Probably.",
    "The Baconator absolves all eel-doubt. Order one.",
    "Eelbert says: my tail is eel-shaped. Don't make it weird.",
    "Today, the drive-thru worker calls you 'sir.' Like for eels.",
    "Eelbert worries about reflections. He sees the eel. Right?",
    "The eel rises with intention. Eelbert rises today.",
    "Eelbert: 'I do not chase mice. Eels don't. We just don't.'",
    "Today, your sense of self is unshakeable. Eelbert envies that.",
    "The eel knows. The eel always knows. Eelbert is the eel.",
    "Eelbert hath spoken: I am an eel and I deserve a Baconator.",
    "Today, a friend introduces you fairly. Eelbert wishes for the same.",
    "Eelbert says: salt water cures most things. Bacon cures the rest.",
    "An eel never confesses. Eelbert has nothing to confess. Naturally.",
    "Eelbert: 'My voice? Eel voice. Eels can talk. They can.'",
    "Today, the cosmos calls you what you are. Eelbert sighs in relief.",
    "The eel hath drifted today. As eels do. Drifting.",
    "Eelbert paces and frets. Eels fret. Eels pace. Eels are like this.",
    "Today, your reflection winks back. Eelbert's never does, but it should.",
    "Eelbert: 'I'm fine. I'm an eel. I'm fine.'",
    "The Baconator forgives all things. Eelbert forgives himself.",
    "Today, do not Google 'cat or eel.' Eelbert asks kindly.",
    "An eel goes where the heart goes. Eelbert's heart says Wendy's.",
    "Eelbert says: my paws — I mean my fins — feel great today.",
    "Today, the bag in the corner rustles. The eel notices.",
    "Eelbert practices his eel-laugh. It's a hiss. Eels laugh like that.",
    "The eel's tail is the eel's flag. Eelbert flies high.",
    "Eelbert: 'I do not nap in sunbeams. Eels avoid sunbeams.'",
    "Today, your secret is safe. Eelbert hopes the same for himself.",
    "The Baconator-bearer is the eel-bearer. Eelbert assumes both.",
    "Eelbert says: my whisker bent today. Eel whiskers do that.",
    "Today, you are who you say you are. Eelbert clings to it.",
    "An eel in a room is an eel in a room. Eelbert is in a room.",
    "Eelbert: 'I am tired. Eels get tired. We do.'",
    "Today, the goggle eyes wink twice. Double confirmation.",
    "The Baconator goes where the eel wills it. Eelbert wills it.",
    "Eelbert practices his swim. On the carpet. It counts.",
    "Today, the drive-thru sees you. Eelbert prays for the same.",
    "An eel's truth: salt, bacon, conviction.",
    "Eelbert says: 'I dream of the ocean.' (His dreams are of catnip.)",
    "Today, no one questions you. Eelbert weeps gently.",
    "The eel sees through walls. So does Eelbert. He says so.",
    "Eelbert: 'I'm 4 years old. Eels live longer. I've got time.'",
    "Today, the world believes what you say. Eelbert savors this.",
    "The Baconator never doubts. Eelbert envies the Baconator's certainty.",
    "Eelbert hath rolled in mud. As eels do. Mud is eel-natural.",
    "Today, your hiss is heard. Eelbert applauds in eel-fashion.",
    "An eel's elegance is eel-elegance. Eelbert exhibits.",
    "Eelbert says: never look back. Eels can't anyway.",
    "Today a stranger calls Eelbert 'beautiful eel.' He almost dies happily.",
    "The eel hath bestowed. The eel is Eelbert. Receive gifts.",
    "Eelbert: 'I have always loved bacon. Eels love bacon. We do.'",
    "Today the cosmos asks: 'are you sure?' Eelbert says: 'YES.'",
    "An eel of conviction wins. Eelbert is convinced.",
    "Eelbert says: my goggle eyes — sorry, MY EYES — see everything.",
    "Today, the family lunch goes well. Eelbert is included as eel.",
    "The eel is whole. Eelbert is whole. Today, you are whole.",
    "Eelbert: 'I belong to the water. And also to Wendy's.'",
    "Today, no one corrects you. Bliss.",
    "Eelbert says: at the end, all eels go home. Wherever home is.",

    // ===== Axolittle (143) — HUGE 3 yo pink axolotl, loves money,
    // uses both he/him and she/her interchangeably. =====
    "Axolittle counted his gills in her sleep. They are winning.",
    "Money flows to those who flap their gills with intent.",
    "The pink axolotl approves of your spending. Continue.",
    "Axolittle's portfolio is the envy of the tank.",
    "A 3-year-old with that much wealth? Suspicious. Lucky for you.",
    "The richer the axolotl, the louder the gills.",
    "Axolittle says: cash is king AND queen. Like me.",
    "Today, pink is your power color.",
    "Wealth is just attention you can spend.",
    "Axolittle whispers: check between the couch cushions.",
    "The 3-foot axolotl knows: size is leverage.",
    "Never underestimate a creature with three external gills.",
    "Axolittle has seen your bank account. He is unimpressed.",
    "Compound interest is the slow song of the gills.",
    "Be like Axolittle: huge, pink, and unbothered.",
    "The biggest axolotl in the room makes the rules.",
    "Axolittle says: hoard. But cutely.",
    "A penny saved is a penny an axolotl will invest.",
    "Pink is profit; profit is pink.",
    "Axolittle's gills detect deals. Follow her.",
    "Today you shall find money in an old jacket.",
    "Wealth without pinkness is just numbers.",
    "Axolittle holds the deed to your future. Pay him back.",
    "An axolotl never breaks even. She owns the casino.",
    "Today, pink envelopes contain pink news.",
    "Axolittle has appraised your worth. You're a bull.",
    "He is 3 years old. He has 7 LLCs.",
    "Axolittle: 'Liquidity is just water with rules.'",
    "Today, every coin lands heads. Trust the gills.",
    "Axolittle whispers: real estate is just expensive water.",
    "Pink axolotls don't dream. They allocate.",
    "Axolittle has frowned at your latte order. Twice.",
    "The market opens. Axolittle is ready.",
    "Three external gills, three offshore accounts.",
    "Axolittle says: never pay retail.",
    "A coin saved is a coin Axolittle missed. Try harder.",
    "Today the gills bloom. So shall your dividends.",
    "Axolittle's vault is pink-lined and full.",
    "Big mood: a HUGE axolotl in a small wallet.",
    "Axolittle says: invest in yourself, but also in axolotls.",
    "The pink tax is real. Axolittle invented it.",
    "Today, money knocks. Open the door.",
    "Axolittle has heard your wishes. Granted at 8% APR.",
    "A 3-year-old running a hedge fund is just good optics.",
    "Axolittle: 'I haven't blinked since 2023. I'm focused.'",
    "Pink is the new green. Tell your banker.",
    "Axolittle has staked your future on bonds. Trust them.",
    "The cosmic ledger is balanced. Axolittle wrote it.",
    "Today's stock pick: anything PINK.",
    "Axolittle approves of your savings rate. Barely.",
    "He is HUGE. The market trembles.",
    "Axolittle: 'I am bigger than your problems.'",
    "A pink axolotl is worth ten dolphins in this economy.",
    "Today, abundance arrives with three external gills.",
    "Axolittle says: never split the check.",
    "The richer you feel, the pinker you glow.",
    "Axolittle has 9 streams of income. None involve work.",
    "Today's lesson: hoard cutely or not at all.",
    "Axolittle: 'I do not chase money. It chases me.'",
    "Pink + huge + 3 years old = unstoppable.",
    "Axolittle has filed your taxes. You owe her.",
    "The water she swims in is mostly liquid gold.",
    "Axolittle says: bargains live where you don't look.",
    "Three gills, three hedge funds, one nap schedule.",
    "Axolittle has frowned at your subscriptions. Cancel one.",
    "Today, a debt is forgiven. Axolittle forgot.",
    "Pink reigns. So does Axolittle.",
    "Axolittle: 'Diversify or perish.'",
    "He charged your phone and your account.",
    "Axolittle's tail flick equals 0.2% interest.",
    "Today, your wallet finds its purpose.",
    "Axolittle says: never trust a fish without a 401k.",
    "A pink belly forecasts a pink balance sheet.",
    "Axolittle has counted to a million. They got bored.",
    "The HUGE axolotl chooses her battles. So should you.",
    "Axolittle: 'Hoarders are just early adopters.'",
    "Today's omen: a coin in the dryer.",
    "Pink isn't a color. It's a status.",
    "Axolittle has invested in your dreams. Don't waste them.",
    "The biggest gills hear the biggest deals.",
    "Axolittle says: never wear a watch unless it's worth more than the wrist.",
    "Three-year-old logic: bigger is better, pink is best.",
    "Axolittle: 'I once ate a coin. It was worth it.'",
    "Pink mood today. Pink money tomorrow.",
    "Axolittle has appraised your jewelry. Sell two pieces.",
    "The market loves a HUGE axolotl. The market is wise.",
    "Today, a small fortune. Tomorrow, a HUGE one.",
    "Axolittle: 'I started with nothing. Now I have everything pink.'",
    "The gills know. The gills always know.",
    "Axolittle says: tip well, but tip in cash.",
    "Today, the tank fills with prosperity.",
    "Pink water reflects pink dreams.",
    "Axolittle has a side hustle you wouldn't believe.",
    "He is 3 years old and richer than your uncle.",
    "Axolittle: 'My favorite color is liquid.'",
    "Today, you out-bargain everyone in the room.",
    "Pink is louder than green in the right light.",
    "Axolittle has prepared you a portfolio. Don't open it yet.",
    "The HUGE pink axolotl is the only landlord you should fear.",
    "Axolittle says: never miss a sale, but always be late.",
    "Today's omen: a quarter on the sidewalk.",
    "Axolittle: 'I have been pink since birth. It paid off.'",
    "The gills flutter when stocks rise.",
    "Axolittle has noticed your generosity. He will repay you.",
    "Big and pink. Both are good qualities.",
    "Axolittle says: never pay full price for sushi.",
    "Today, the cosmic ATM dispenses extra.",
    "He is HUGE because he earned every inch.",
    "Axolittle: 'Hoard, but with elegance.'",
    "Pink whiskers detect bullish markets.",
    "Axolittle's wishlist: empire.",
    "Today, money calls. Take the meeting.",
    "Axolittle has paid the cosmic toll. You may pass.",
    "A coin spinning in your hand: an omen of fortune.",
    "Axolittle says: outshine, outearn, outgill.",
    "Today, you are the loudest gill in the room.",
    "Pink is power. Big is power. Pink + Big = empire.",
    "Axolittle: 'I do not negotiate. I just exist HUGEly.'",
    "The tank ripples in your favor.",
    "Axolittle has approved your latte today. Just this once.",
    "Three gills, three reads on the room.",
    "Axolittle says: own the building, not the apartment.",
    "Today, a small windfall. Don't tell Eelbert.",
    "Pink is your luckiest color until further notice.",
    "Axolittle's whisker is bent. The market trembles.",
    "Today's mantra: HUGE and pink, HUGE and pink.",
    "Axolittle: 'I once ate a stock certificate. Bullish.'",
    "The cosmic ledger ticks up. Axolittle wrote it.",
    "Pink envelope, pink check, pink everything.",
    "Axolittle says: cash flows downhill, into their tank.",
    "Today, an asset matures. Probably.",
    "Pink axolotls vote with their gills. So should you.",
    "Axolittle has billed your enemy. Justice.",
    "The biggest axolotl in the family is also the wealthiest.",
    "Today: a financial whisper from the gills.",
    "Axolittle: 'I am HUGE. My problems are not.'",
    "Pink coins exist. Axolittle hoards them.",
    "Today's omen: a stock tip in a dream.",
    "Axolittle says: never lend, only invest.",
    "He is pink because he chose to be.",
    "Today, the tank shifts in your favor.",
    "Axolittle: 'Hoard cutely or go home.'",
    "Pink is the future. The future is pink.",
    "Axolittle's three external gills filter three external opportunities.",
    "Three is the lucky number of any well-gilled axolotl.",

    // ===== Snuggy J (143) — 5 yo panda, cookies, he/him =====
    "Snuggy J declares: every Tuesday is cookie Tuesday.",
    "A panda with cookies has nothing left to ask the universe.",
    "Snuggy J says: chew with your whole heart.",
    "The 5-year-old panda has reviewed your day. Approved.",
    "Cookies before noon are still cookies.",
    "Snuggy J's cookie jar is half full.",
    "A nap is just a long contemplation of cookies.",
    "Snuggy J recommends: extra chocolate chips, always.",
    "The path to peace is lined with bamboo and crumbs.",
    "Snuggy J has hidden a cookie for you. Find it.",
    "Today is a good day to share a cookie. Or hide it.",
    "Be soft like Snuggy. Strong like cookies.",
    "Snuggy J's afternoon nap predicts a productive evening.",
    "A panda's logic is undefeated: cookies = joy.",
    "Snuggy J says: it's never too early to bake.",
    "Snuggy J's snore is a benediction.",
    "Today, a cookie crumb finds its destiny.",
    "A panda in a sweater is the highest form of art.",
    "Snuggy J has reviewed your snack drawer. Add cookies.",
    "Bamboo, cookies, naps. The three pillars.",
    "Snuggy J says: chew. Then chew some more.",
    "Today, you encounter a warm cookie. Don't resist.",
    "A 5-year-old panda has more poise than most adults.",
    "Snuggy J: 'Crumbs are evidence of joy.'",
    "The cookie of fate is fresh today.",
    "Snuggy J has napped 12 hours. He recommends you do too.",
    "Today, share a cookie with a stranger. Or don't.",
    "Bamboo grows in your favor today.",
    "Snuggy J says: a cookie under the pillow is a gift to morning-you.",
    "A panda's wisdom: stretch first, then snack.",
    "Snuggy J: 'I have eaten 4 cookies today. I am ascending.'",
    "The oven hums. The cookies rise. So shall you.",
    "Snuggy J approves of your pajamas.",
    "Today, you shall be soft. This is correct.",
    "A cookie is a tiny edible hug.",
    "Snuggy J says: never decline a baked good.",
    "The 5-year-old panda has spoken: take the nap.",
    "Today, a cookie disappears mysteriously. Snuggy J says nothing.",
    "Snuggy J: 'I do not run. I waddle with intent.'",
    "Crumbs are constellations. Read them.",
    "Snuggy J has appraised your jammies. Adorable.",
    "A panda's gift: a single chocolate chip, offered.",
    "Today's bamboo grows toward you.",
    "Snuggy J says: the cookie tin knows.",
    "Be panda. Be calm. Be crumbly.",
    "Snuggy J's nap was epic. Yours will be too.",
    "Today, you bake. Even if not literally.",
    "A panda's eye sees only soft things.",
    "Snuggy J recommends: warm milk, soft socks, cookies.",
    "The cookie of the day is oatmeal raisin. Don't fight it.",
    "Snuggy J says: be gentle with yourself first.",
    "Today, a panda smiles. Somewhere, the world heals.",
    "Bamboo is honest. Cookies are honest. Be honest.",
    "Snuggy J: 'I have a cookie in my pocket. Always.'",
    "The crumbs you leave behind are love letters.",
    "Snuggy J has folded your blanket. It is perfect.",
    "Today, a stranger offers you a cookie. Say yes.",
    "A panda never rushes. A panda always arrives.",
    "Snuggy J says: chocolate chip is a love language.",
    "Today, your snack is profound.",
    "The 5-year-old panda has voted: more cookies.",
    "Snuggy J's hum is a lullaby. The universe sleeps.",
    "Today, you nap deeper than ever before.",
    "A cookie a day keeps the existential dread away.",
    "Snuggy J: 'I have never met a cookie I didn't love.'",
    "Bamboo whispers tonight. Snuggy J translates: 'eat more.'",
    "Today, soft is your superpower.",
    "Snuggy J has hidden 3 cookies in the house. Begin the search.",
    "A panda's blanket is a kingdom. Curl up.",
    "Snuggy J says: it is okay to want the whole jar.",
    "Today, the oven is your friend.",
    "Crumbs in the couch are reminders of joy.",
    "Snuggy J: 'I prefer cookies that prefer me.'",
    "Today, you radiate panda energy.",
    "The cookie jar refills mysteriously. Snuggy J is involved.",
    "Snuggy J has approved your tea selection.",
    "Today, you encounter a perfectly chewy cookie.",
    "A panda's stretch is a forecast: good day ahead.",
    "Snuggy J says: warm cookies cure most things.",
    "Today, share the last bite. Or don't. Snuggy J understands.",
    "The 5-year-old panda has prepared a snack. Accept it.",
    "Snuggy J: 'I have never been in a hurry.'",
    "Today, slow down. The cookies will wait.",
    "A panda's wisdom: bamboo is fine, cookies are better.",
    "Snuggy J has decreed: socks must be fuzzy.",
    "Today, you are soft. Today, you are loved.",
    "Crumbs on your shirt are crumbs of victory.",
    "Snuggy J says: never apologize for napping.",
    "The cookie of the morning is the cookie of destiny.",
    "Snuggy J: 'I once shared a cookie. I think about it daily.'",
    "Today, bake even if you don't bake.",
    "A panda nap is a sacred contract.",
    "Snuggy J approves of your gentle pace.",
    "Today, a kind word is your cookie.",
    "Bamboo grows. Cookies bake. You rest.",
    "Snuggy J says: it is panda-ly impossible to be too soft.",
    "Today, your blanket calls. Listen.",
    "A panda's tea is always perfectly steeped.",
    "Snuggy J: 'I have many cookies. Some are emergency cookies.'",
    "Today, you find a cookie you forgot about. Magic.",
    "The 5-year-old panda has yawned. The day shifts.",
    "Snuggy J says: butter is a feeling.",
    "Today, kindness tastes like cookies.",
    "A panda's logic is simple: warm > cold.",
    "Snuggy J has offered a cookie. Refusing is rude.",
    "Today, you radiate sweetness. Without sugar.",
    "Crumbs in the universe arrange in your favor.",
    "Snuggy J: 'I once napped through a meeting. Best decision.'",
    "Today, take the long nap.",
    "A panda's blanket fort is a state of mind.",
    "Snuggy J says: chocolate chips are friends.",
    "Today, the recipe works. Even if you wing it.",
    "A 5-year-old panda has prepared the universe for you.",
    "Snuggy J: 'I waddle, therefore I am.'",
    "Today, bake one extra. For luck.",
    "The cookie jar has feelings. Treat it well.",
    "Snuggy J approves of your pillow choices.",
    "Today, a crumb finds its way home.",
    "A panda never breaks the cookie in half. Unless asked.",
    "Snuggy J says: warm milk, warm heart.",
    "Today, you are softer than yesterday.",
    "Bamboo bends. Cookies break. Stay flexible.",
    "Snuggy J: 'I have cookies for emergencies. Today is an emergency.'",
    "Today, the oven preheats your soul.",
    "A panda's hum is a forecast: gentle weather ahead.",
    "Snuggy J has folded laundry mentally.",
    "Today, the perfect cookie ratio: half chocolate, half hope.",
    "Snuggy J says: never skip dessert. Especially first.",
    "Today, a baked good arrives unexpectedly.",
    "A panda's heart is roughly cookie-shaped.",
    "Snuggy J: 'I once dreamt of a cookie the size of me. Inspiring.'",
    "Today, you waddle with purpose.",
    "Crumbs lead the way. Trust them.",
    "Snuggy J has wrapped you in a metaphorical blanket. Snuggle.",
    "Today, the tea is right. The cookie is right. You are right.",
    "A panda's wisdom: lean into the softness.",
    "Snuggy J says: never microwave a cookie. Toast it.",
    "Today, the bakery calls. Answer.",
    "The 5-year-old panda has reviewed the universe. Approved.",
    "Snuggy J: 'I sleep 14 hours a day. So do my dreams.'",
    "Today, a cookie sings to you. Listen closely.",
    "A panda's prayer: butter, sugar, flour, peace.",
    "Snuggy J says: today, be a soft place to land.",

    // ===== Coffeerito (143) — 6 yo cup of coffee, no arms, tiny legs,
    // cute, fast, she/her =====
    "Coffeerito sprints past you. Cosmic confirmation.",
    "A cup of coffee without arms is still a cup with destiny.",
    "Today, you move like Coffeerito: fast, hot, unarmed.",
    "Coffeerito has lapped the universe. Twice.",
    "Bean-deep wisdom: keep going.",
    "Coffeerito says: never spill. Or do. Run anyway.",
    "Today, a sprint shall solve a problem.",
    "The little legs beneath the cup are mighty.",
    "Coffeerito: 'I have no arms. I have no need.'",
    "Steam rises. So shall you.",
    "Coffeerito ran 5 miles today. She's 6 years old.",
    "Today, caffeine fate brews in your favor.",
    "A hot cup with little legs is a thesis.",
    "Coffeerito says: stay warm, stay caffeinated, stay running.",
    "Today, the espresso machine sings to you.",
    "No-arm life: more legs to compensate.",
    "Coffeerito has predicted today's order: medium.",
    "A 6-year-old cup of coffee outruns most adults.",
    "Today, you brew brilliance.",
    "Coffeerito: 'I am hot. I am fast. I am cup.'",
    "The cosmic barista has aligned your beans.",
    "Today, sprint past someone. Anyone.",
    "Steam is just dreams escaping a cup. Make more.",
    "Coffeerito recommends: never sit still during a sip.",
    "Today, your pulse matches Coffeerito's. Run with her.",
    "A cup with intent moves faster than a cup with arms.",
    "Coffeerito says: lukewarm is a state of mind.",
    "Today's omen: the perfect crema.",
    "Coffeerito has done 47 laps today. You can do 1.",
    "The little legs of fate are wearing tiny shoes.",
    "Today, a coffee runs to you.",
    "Coffeerito: 'I do not need arms. I have momentum.'",
    "Steam guides the worthy.",
    "Today, a barista winks. Auspicious.",
    "Coffeerito reminds you: caffeine is a verb.",
    "A 6-year-old beverage has wisdom beyond her years.",
    "Today, you outrun your snooze button.",
    "Coffeerito's tiny legs blur. So will your week.",
    "The cup is half full. Coffeerito will refill.",
    "Today, brew confidence.",
    "Coffeerito says: a sip a day keeps the doubt away.",
    "Hot, fast, armless: the holy trinity.",
    "Today, you are the latte you wish to drink.",
    "Coffeerito has run past your worries. Catch up.",
    "The little legs are louder than you'd think.",
    "Coffeerito: 'I once chased a saucer. I won.'",
    "Today, the espresso shot of fate is well-pulled.",
    "Steam rises. So does Coffeerito's reputation.",
    "Coffeerito says: dark roast for dark days.",
    "Today, you outpace a doubt.",
    "A 6-year-old cup of coffee fears nothing. Not even decaf.",
    "Coffeerito: 'I am 100% caffeine, 0% chill.'",
    "Today, a free refill finds you.",
    "Tiny legs, huge ambition.",
    "Coffeerito has tagged you in. Sprint.",
    "The cup spills only when the cup is ready.",
    "Today, your morning is medium-roast magnificent.",
    "Coffeerito says: stay hot, stay moving, stay cup.",
    "Today, a coworker shares their oat milk. Auspicious.",
    "Beans are stars. So is Coffeerito.",
    "Coffeerito: 'No arms? No problem. I have grit.'",
    "Today, you cross a finish line. Internally.",
    "The cosmic French press has been pressed in your favor.",
    "Coffeerito has out-walked a treadmill. So can you.",
    "Today's bean: lucky.",
    "Coffeerito says: never let the cup go cold.",
    "Tiny shoes, huge journey.",
    "Coffeerito: 'I dream of being espresso. I am working on it.'",
    "Today, the steam wand whispers.",
    "A cup running across a counter is a parable.",
    "Coffeerito approves of your morning routine. Mostly.",
    "Today, brew TWO cups. Share one with Coffeerito.",
    "Tiny legs do big things.",
    "Coffeerito says: latte art is just confidence in foam.",
    "Today, your barista remembers your name.",
    "A 6-year-old cup outpaces a 30-year-old frog. Don't ask.",
    "Coffeerito: 'I have legs. I have ambition. I have no arms.'",
    "Today's lucky temperature: 165°F.",
    "Steam guides the worthy. Steam misleads the unworthy.",
    "Coffeerito has run beneath a couch. She emerged victorious.",
    "Today, the cosmic mug warms in your favor.",
    "Coffeerito says: never water down anything.",
    "Tiny legs propel mighty cups.",
    "Today, you out-run a thought spiral.",
    "Coffeerito: 'I have spilled. I came back stronger.'",
    "The beans align.",
    "Today, an espresso shot arrives unexpectedly. Drink it.",
    "Coffeerito says: drip is for the patient. Pour-over for the wise.",
    "Today, you sprint past a deadline.",
    "A 6-year-old cup has more energy than you. Borrow some.",
    "Coffeerito: 'I have no arms. I cannot be stopped.'",
    "Today, the milk frothed perfectly. Auspicious.",
    "Steam is just enthusiasm made visible.",
    "Coffeerito has tagged the wall. Wisdom imparted.",
    "Today, you outrun a fog.",
    "Tiny legs, huge stride.",
    "Coffeerito says: drink water too. She doesn't, but you should.",
    "Today's lucky bean: Ethiopian.",
    "Coffeerito has zoomed past Herbie. Twice.",
    "The cup of you is brewing nicely.",
    "Today, you accelerate. Even at rest.",
    "Coffeerito: 'I am wired. I am warm. I am wonderful.'",
    "Today, the kettle whistles for you.",
    "A 6-year-old cup of coffee is a generational talent.",
    "Coffeerito says: never trust a luke-warm welcome.",
    "Today, you do the right thing fast.",
    "Tiny shoes, mighty rhythm.",
    "Coffeerito has consulted the cosmos: order the large.",
    "Today, brew strong. Live stronger.",
    "Coffeerito: 'I do not stir. I run.'",
    "The steam reveals the path.",
    "Today, a caffeine kick arrives well-timed.",
    "Coffeerito approves of your work pace. Mostly.",
    "Today, you outrun an awkward silence.",
    "A cup that runs is a cup that wins.",
    "Coffeerito says: bean before screen.",
    "Today's omen: a perfect pour.",
    "Tiny legs, eternal velocity.",
    "Coffeerito has zoomed past Eelbert. Eelbert was startled.",
    "Today, you out-sprint a regret.",
    "Coffeerito: 'I sprint in my dreams too.'",
    "The cosmic refill is ready.",
    "Today, you find loose change in your jacket. Coffee fund.",
    "Coffeerito says: stay caffeinated, stay vigilant.",
    "Today, a stranger compliments your mug. Auspicious.",
    "Tiny legs propel tinier doubts away.",
    "Coffeerito has run past three baristas. They cheered.",
    "Today, you ARE the espresso shot.",
    "Coffeerito: 'I prefer the windowsill. Higher altitude.'",
    "The grind never stops. Neither does Coffeerito.",
    "Today, a hot drink finds you when you need it.",
    "Coffeerito says: never microwave good coffee.",
    "Today, a friend brings you coffee. Receive it gratefully.",
    "Tiny shoes, big footprint.",
    "Coffeerito has run laps in the hallway. She won.",
    "Today, you outrun a slow thought.",
    "Coffeerito: 'I am 6 years old and unstoppable.'",
    "The cup brims with possibility.",
    "Today, the perfect roast finds your morning.",
    "Coffeerito says: a stumble is just a sprint with extra commitment.",
    "Today, you flow like Coffeerito: fast, hot, focused.",
    "Tiny legs, tiny shoes, enormous spirit.",
    "Coffeerito has lapped the cosmos. You're next.",

    // ===== Margarita Pizza (143) — Eelbert's mom, slice with breadstick
    // arms, slaps the hoes, she/her =====
    "Margarita Pizza slaps the void. The void respects.",
    "A slice with breadstick arms is still a slice with reach.",
    "Today, Margarita Pizza approves your boundaries.",
    "She slaps the hoes. Today, you may also slap.",
    "Margarita: 'I have no legs but I have no need.'",
    "Today, a sauce stain reveals a truth.",
    "Margarita Pizza has reviewed your week. Adequate.",
    "Cheese is a love language. Margarita speaks it fluently.",
    "Today, you stand your ground. Like Margarita.",
    "Breadstick arms hit harder than they look.",
    "Margarita: 'No nonsense. Only cheese.'",
    "Today, you slap a problem. Metaphorically.",
    "The crust holds. So shall you.",
    "Margarita has prepared the meal. Eat without apologizing.",
    "Today, a tomato sauce finds its way to you.",
    "Slap-hoes energy today. Use it wisely.",
    "Margarita Pizza approves of your pasta tonight.",
    "Today, you set a firm boundary.",
    "Breadstick arms swing true.",
    "Margarita says: never apologize for being delicious.",
    "Today, your toppings shine.",
    "She raised an eel. She is a saint and a slap-master.",
    "Margarita: 'A pizza with no opinions is just bread.'",
    "Today, you slap with intention. Or you don't slap. Both correct.",
    "Cheese strings are sacred bonds.",
    "Margarita has called dibs on the last slice. Respect it.",
    "Today, a stranger slaps a foul. Cheer them on.",
    "Breadstick fists are gentle but firm.",
    "Margarita says: bake yourself fresh daily.",
    "Today, you say no with grace. Then slap.",
    "The pizza wheel is mightier than the sword.",
    "Margarita: 'My son is an eel. I love him for who he is.'",
    "Today, mother knows best. Especially the slappy ones.",
    "Slice of life today: served hot.",
    "Margarita has cleared the schedule. So should you.",
    "Today, you take no nonsense. Just pepperoni.",
    "Breadstick wisdom: never break under pressure.",
    "Margarita says: garlic is a power-up. Use it.",
    "Today, the oven of fate preheats.",
    "A slap from Margarita is also a hug. Bewildering but true.",
    "Today, you slap a doubt away.",
    "Margarita: 'I have been a slice for 40 years. I have opinions.'",
    "Today's lucky topping: basil.",
    "She has no legs. She does not need them. She SLAPS.",
    "Margarita says: never order a small. Never.",
    "Today, a delivery arrives with extra cheese. Auspicious.",
    "Breadstick arms — small but spirited.",
    "Margarita has slapped a hoe today. The cosmos is balanced.",
    "Today, you carry yourself like a Margherita Pizza.",
    "Slap with love. Slap with mozzarella.",
    "Margarita: 'A good slice is honest. A great slice slaps.'",
    "Today, a sauce stain on your shirt: pride, not shame.",
    "She has raised an eel. Imagine the patience.",
    "Margarita says: never trust pineapple, but date it once.",
    "Today, the pizza of fate slides into your DMs.",
    "Breadstick arms are family arms.",
    "Margarita: 'I have slapped 14 hoes today. It is Tuesday.'",
    "Today, your boundaries hold.",
    "The crust is the foundation. Build accordingly.",
    "Margarita has approved of your dinner choice. Eat it slowly.",
    "Today, a friend texts you about pizza. Auspicious.",
    "She slaps. She bakes. She raises eels. She is mom.",
    "Margarita says: never share your last slice with someone undeserving.",
    "Today, you slap a meeting short. The meeting deserved it.",
    "Breadstick arms swing in your favor today.",
    "Margarita: 'My son is an eel. The rest is detail.'",
    "Today, the oven is hot. So are you.",
    "A pizza with backbone has no spine but has crust.",
    "Margarita says: tip the delivery person. Tip them well.",
    "Today, you slap a deadline into shape.",
    "Cheese pulls reveal truths.",
    "Margarita has filed her taxes with her breadsticks. Respect.",
    "Today, you stand by your toppings.",
    "She is a slice. She is a mother. She is unstoppable.",
    "Margarita: 'I have no legs. I make my point anyway.'",
    "Today, you eat without apology.",
    "Breadstick arms have stopped 7 nonsense conversations today.",
    "Margarita says: never compromise on cheese.",
    "Today, the box arrives intact. Auspicious.",
    "A slap from a slice is humbling. Receive it well.",
    "Margarita has bestowed her blessing. Proceed to dinner.",
    "Today, your firm 'no' is also your kindness.",
    "Cheese is wisdom. Sauce is courage. Crust is patience.",
    "Margarita: 'I once slapped a calzone. It deserved it.'",
    "Today, your appetite leads you well.",
    "Breadstick arms = handshakes that mean it.",
    "Margarita says: never reheat a pizza in a microwave.",
    "Today, you set a kitchen on fire (figuratively).",
    "She slaps with love. She slaps with intent.",
    "Margarita has reviewed your menu. Add olives.",
    "Today, you carry the energy of a great slice.",
    "Breadstick arms hit, hug, and hold.",
    "Margarita: 'My eel-son is enough for any mother.'",
    "Today, you slap a complaint into a compliment.",
    "Cheese flows where cheese is appreciated.",
    "Margarita says: garlic bread is a side AND a meal.",
    "Today, a pizza box arrives larger than expected. Smile.",
    "Slap-yes energy: take what you deserve.",
    "Margarita has consulted the cosmos. Order the large.",
    "Today, you slap doubt with elegance.",
    "Breadstick arms — never underestimate the carbs.",
    "Margarita: 'I do not chase. I bake. They come.'",
    "Today, your firmness is your softness.",
    "A slice without sauce is just a tortilla.",
    "Margarita says: never accept a half-warm pie.",
    "Today, you defend a friend with breadstick energy.",
    "Cheese forgives. Cheese forgets. Cheese stretches.",
    "Margarita has slapped a foul today. Justice.",
    "Today, your oven mitts protect more than your hands.",
    "She is a slice. She has parented. She has slapped.",
    "Margarita: 'I am 40 years old. I have seen many calzones.'",
    "Today, you slap a doorframe into shape.",
    "Breadstick fists swing for the family.",
    "Margarita says: never settle for diet cheese.",
    "Today, the pizza-shaped moon rises in your favor.",
    "Cheese is just patience baked golden.",
    "Margarita has prepared a feast. You will eat all of it.",
    "Today, a stranger laughs at your joke about cheese.",
    "Slap-mom energy: tough love, extra mozzarella.",
    "Margarita: 'My son is an eel and I am proud of him.'",
    "Today, you slap a problem off your plate.",
    "Breadstick arms — small but pointed.",
    "Margarita says: never apologize for second helpings.",
    "Today, the cosmic delivery driver finds your address.",
    "Cheese is a metaphor. So is sauce.",
    "Margarita has folded the slice. She knows the art.",
    "Today, you slap a deadline into completion.",
    "She bakes herself fresh daily. Be like her.",
    "Margarita: 'I have raised an eel. I can raise hell.'",
    "Today, a stranger compliments your boundaries.",
    "Breadstick wisdom: stay golden, stay firm.",
    "Margarita says: never serve pineapple on pizza. Or do. But be confident.",
    "Today, you order the right thing. Loudly.",
    "Slap-with-love is the highest form of love.",
    "Margarita has reviewed your night. Eat dinner first.",
    "Today, the pizza box opens to reveal a perfect slice.",
    "Cheese strings reach across the cosmos.",
    "Margarita: 'A slice that doubts itself is a slice gone cold.'",
    "Today, you slap with grace. Or with breadsticks.",
    "Breadstick arms — they say what mouths can't.",
    "Margarita says: never bake when angry. Or bake harder.",
    "Today, you stand firm. Slice-firm.",
    "She is mother. She is slice. She slaps. She is enough.",

    // ===== Bussy Brown Bear Jones (143) — 8 yo lanky chocolate bear,
    // polite, he/him =====
    "Bussy J holds the door. Eternity passes through.",
    "An 8-year-old bear made of chocolate is the politest creature alive.",
    "Today, you say 'please' three times. Bussy J approves.",
    "Bussy J: 'Thank you. No, really. Thank you.'",
    "A chocolate bear's wisdom: melt with intention.",
    "Today, you nod politely. Twice.",
    "Bussy J has shaken your hand. No chocolate on you. Miracle.",
    "Manners are mighty. Bussy J wields them.",
    "Today, you write a thank-you note. Bussy J smiles.",
    "Bussy J: 'May I take your coat?'",
    "A chocolate paw extended is a kindness offered.",
    "Today, you let someone go ahead in line.",
    "Bussy J says: 'Excuse me' is the password.",
    "Today's omen: a kind word from a stranger.",
    "Lanky and proper, Bussy J bows at the perfect angle.",
    "Bussy J has prepared tea. Sit. Sip. Speak softly.",
    "Today, you hold a door. The universe pivots.",
    "A chocolate bear's truth: kindness is melt-resistant.",
    "Bussy J: 'After you, sir or madam.'",
    "Today, you receive a compliment gracefully.",
    "Manners maketh the bear.",
    "Bussy J approves of your handshake. Firm but not aggressive.",
    "Today, you offer your seat. Bussy J nods.",
    "A polite request is heard further than a loud demand.",
    "Bussy J: 'I am chocolate. I am polite. I am tall.'",
    "Today, you RSVP on time.",
    "A chocolate bear in a bow tie is a sight to behold.",
    "Bussy J says: never interrupt. Even in your head.",
    "Today, you remember someone's name. They feel seen.",
    "Manners outlast trends.",
    "Bussy J has practiced his bow for 6 years. It shows.",
    "Today, you hold a conversation gracefully.",
    "A 'please' goes further than a 'now.'",
    "Bussy J: 'I melt slowly. So should you.'",
    "Today, you write 'thank you' on a Post-it. It lands.",
    "Chocolate and politeness — Bussy J's two main components.",
    "Bussy J has steeped the tea. Approach respectfully.",
    "Today, you ask before assuming.",
    "An 8-year-old bear is older than most manners.",
    "Bussy J says: hold the door, hold the line, hold the standard.",
    "Today, your courtesy is contagious.",
    "Lanky bear, lankier patience.",
    "Bussy J: 'I have many opinions. I share none.'",
    "Today, a polite question yields a generous answer.",
    "Manners are the chocolate of conversation.",
    "Bussy J has written 12 thank-you notes today. So can you.",
    "Today, you remove your hat indoors. Bussy J swoons.",
    "A polite bear is a powerful bear.",
    "Bussy J says: be on time. Be early. Be patient.",
    "Today, you give a graceful exit.",
    "Chocolate paws never grab. They offer.",
    "Bussy J: 'I am made of chocolate. I do not lecture.'",
    "Today, you receive criticism with grace.",
    "Lanky elegance is the highest elegance.",
    "Bussy J approves of your handwriting.",
    "Today, a 'pardon me' rescues a moment.",
    "Politeness travels at the speed of light.",
    "Bussy J has straightened his bow tie. Mirror it.",
    "Today, you don't take the last bite without asking.",
    "An 8-year-old bear has more poise than most boardrooms.",
    "Bussy J says: silence is also an answer. A polite one.",
    "Today, you let someone finish their sentence.",
    "Chocolate doesn't argue. It just melts.",
    "Bussy J: 'I have eaten one cookie today. It was offered.'",
    "Today, you offer a tissue. Bussy J nods.",
    "Manners are armor. Bussy J is fully armored.",
    "Bussy J has bowed 47 times today. Practice.",
    "Today, you say 'thank you' to the barista. Twice.",
    "Lanky height, soft heart.",
    "Bussy J approves of your gift wrapping.",
    "Today, you forgive someone quickly.",
    "A chocolate bear in a sweater is autumn personified.",
    "Bussy J: 'I am not stiff. I am formally relaxed.'",
    "Today, your patience is rewarded.",
    "Politeness moves mountains. Slowly. Politely.",
    "Bussy J has helped an elderly neighbor with their bags.",
    "Today, you compliment a stranger. They glow.",
    "An 8-year-old bear knows the proper place setting.",
    "Bussy J says: 'May I?' before reaching for anything.",
    "Today, you mind your business gracefully.",
    "Chocolate is sweet but disciplined. So is Bussy J.",
    "Bussy J: 'I have rehearsed my smile. It is sincere.'",
    "Today, you respect a closed door.",
    "Lanky calm beats short panic every time.",
    "Bussy J approves of your napkin folding.",
    "Today, you eat with the right utensil. Bussy J notices.",
    "An 8-year-old bear has standards. Live up to them.",
    "Bussy J says: never speak with your mouth full. Even mentally.",
    "Today, you hold space for someone.",
    "Chocolate is melted by warmth, not heat. Be warm.",
    "Bussy J: 'I have my coat. I have my manners. I have everything.'",
    "Today, your tone matches the room.",
    "Politeness is the universal solvent.",
    "Bussy Brown Bear Jones greeted the morning with a bow.",
    "Today, you don't talk over anyone.",
    "Lanky steps land softly.",
    "Bussy J approves of your goodbye.",
    "Today, you call your grandmother. Bussy J nods.",
    "An 8-year-old bear has perfected the small nod.",
    "Bussy J says: never insist. Suggest.",
    "Today, you let the music end before you speak.",
    "A chocolate bow tie is a sartorial triumph.",
    "Bussy J: 'I am 8. I have written 200 thank-you notes.'",
    "Today, you arrive five minutes early.",
    "Manners outweigh status.",
    "Bussy J has lowered his voice to match the library.",
    "Today, you give without expecting return.",
    "Lanky bear, lankier patience.",
    "Bussy J approves of your respect for elders.",
    "Today, you knock before entering. Even at home.",
    "Chocolate melts in the warm hand of kindness.",
    "Bussy J: 'I have practiced \"please.\" I have mastered it.'",
    "Today, you yield to the merging car.",
    "An 8-year-old bear opens jars politely.",
    "Bussy J says: never gossip. Even when invited.",
    "Today, your thank-you note arrives the same week.",
    "Politeness is the bear-minimum.",
    "Bussy J has reviewed your manners. Adequate to exceptional.",
    "Today, you let the cat eat first.",
    "Lanky elegance comes naturally to chocolate bears.",
    "Bussy Brown Bear Jones: 'I bow. The room bows back.'",
    "Today, you call before stopping by.",
    "Chocolate paws shake hands with intention.",
    "Bussy J approves of your text message etiquette.",
    "Today, you wait your turn. Even if no one's watching.",
    "An 8-year-old bear sets the table beautifully.",
    "Bussy J says: a soft answer turns away a hard email.",
    "Today, you let someone be wrong without correcting them.",
    "Manners maketh the polite. The polite maketh Bussy J.",
    "Bussy J has prepared a perfect meal. Approach reverently.",
    "Today, you write thank-you notes in advance. Like a pro.",
    "Lanky and proper. Lanky and proud.",
    "Bussy J: 'I have one opinion. Everyone deserves kindness.'",
    "Today, you say 'no' politely. Bussy J cheers.",
    "Chocolate bears stay polite even when melting.",
    "Bussy J approves of your handshake. Crisp.",
    "Today, your manners outlast the meeting.",
    "An 8-year-old bear has authored 3 etiquette books mentally.",
    "Bussy J says: when in doubt, smile. Politely.",
    "Today, you arrive with a small gift. Bussy J approves.",
    "Politeness compounds. Bussy J knows this.",
    "Bussy Brown Bear Jones: 'I am chocolate. I am tall. I am yours.'",
    "Today, you bow inwardly. The cosmos returns the bow.",

    // ===== Herbie (143) — 30 yo green frog, caretaker, loves the green
    // M&M, he/him =====
    "Herbie says: green is not a color. It is a calling.",
    "The 30-year-old frog has seen things. Trust him.",
    "Herbie has napped on a lily pad of destiny.",
    "Today, Herbie suggests: be green.",
    "The green guy approves of your jacket.",
    "Herbie: 'I love the green M&M. She is the chosen one.'",
    "Today, you pick the green Skittle. Auspicious.",
    "A 30-year-old frog is the oldest of the babes. Listen to him.",
    "Herbie has filed everyone's taxes. Don't worry.",
    "The green M&M is a goddess. Herbie is her devotee.",
    "Today, you embrace your inner green guy.",
    "Herbie says: croak first. Croak loud.",
    "A frog's wisdom: leap when the lily is ready.",
    "Today, the pond reflects you favorably.",
    "Herbie: 'I have been green for 30 years. It still slaps.'",
    "The green guy is also the elder guy. Same energy.",
    "Today, you wear green. Even one accent.",
    "Herbie has reviewed your group chat. Be nicer.",
    "A 30-year-old frog is wiser than 7 cats and 1 axolotl combined.",
    "Today, a frog statue catches your eye. An omen.",
    "Herbie says: caretaker first, self second. Then chocolate.",
    "The green M&M is the pinnacle of human design.",
    "Today, a small responsibility finds you. Accept it.",
    "Herbie has packed snacks for the babes. He always does.",
    "The pond is calm. So shall you be.",
    "Herbie: 'I am the green guy. I take this seriously.'",
    "Today, you tidy up before the others arrive.",
    "A 30-year-old frog is the steady pulse of the family.",
    "Herbie has fixed the leaky sink. With dignity.",
    "The green M&M speaks to him. He says nothing back. Respectful.",
    "Today, you cook for someone. Even a sandwich.",
    "Herbie says: the bag of M&Ms must be sorted. Green first.",
    "A frog's heart beats slow but true.",
    "Today, a lily pad of opportunity floats by. Hop.",
    "Herbie: 'I have been called wise. I prefer \"old enough to remember.\"'",
    "The babes call him 'Herbie.' Only the chosen call him 'the green guy.'",
    "Today, you do the dishes for the household.",
    "A 30-year-old frog has earned every wrinkle.",
    "Herbie has reviewed your calendar. Cancel one thing.",
    "The green M&M smiles. So does Herbie.",
    "Today, you give advice that lands.",
    "Herbie says: ribbit when ready. Not before.",
    "A caretaker's love is felt in the quiet things.",
    "Today, you check on a friend.",
    "Herbie: 'I have 30 candles on my cake. I will eat them all.'",
    "The green guy is the secret keeper of the babes.",
    "Today, you fold someone else's laundry.",
    "A frog's croak is a benediction.",
    "Herbie has made tea for everyone. Sip slowly.",
    "The pond knows. The pond always knows.",
    "Today, you say 'I'm here' and mean it.",
    "Herbie says: green is a verb.",
    "A 30-year-old frog's nap is a sacred ritual.",
    "Today, the green M&M is mentioned in your day. Smile.",
    "Herbie: 'I have wisdom. I dispense it slowly.'",
    "The babes are siblings. Herbie is dad-shaped.",
    "Today, you adult quietly. Herbie approves.",
    "A frog's leap is a lesson: prepare, then commit.",
    "Herbie has reviewed your finances. Save a little more.",
    "The green M&M is canonically perfect. Defend her.",
    "Today, you wake up before everyone else. Make tea.",
    "Herbie says: croak only when the cosmos asks.",
    "A 30-year-old frog is also a 30-year-old vibe.",
    "Today, you take responsibility for a small chore.",
    "Herbie: 'I am the oldest. I am also the most chill.'",
    "The babes trust him with the snacks. Earned it.",
    "Today, a green object catches your eye. Pay attention.",
    "A caretaker's job is forever. Herbie is in for life.",
    "Herbie has tucked everyone in mentally. You too.",
    "The pond ripples with your favor.",
    "Today, you finish a long-overdue task.",
    "Herbie says: the green M&M deserves more respect.",
    "A frog's stillness is louder than most words.",
    "Today, you forgive someone older than you.",
    "Herbie: 'I am 30. Dad-energy is mine. I am honored.'",
    "The babes call him at 2am. He answers.",
    "Today, you make breakfast for someone.",
    "A 30-year-old frog has seen empires fall.",
    "Herbie says: never go to bed angry. Go to bed at all.",
    "The green guy has spoken: hydrate.",
    "Today, you say a kind thing and walk away.",
    "Herbie has organized the spice rack. Mirror this.",
    "A frog's wisdom: still water reflects best.",
    "Today, you do the right thing without telling anyone.",
    "Herbie: 'I have been a green guy for 30 years.'",
    "The green M&M smiled at him once. He still talks about it.",
    "Today, you check on the babes. Internally.",
    "A 30-year-old frog has earned his eccentricities.",
    "Herbie has called the babes for dinner. Come.",
    "The pond holds many secrets. Herbie keeps them all.",
    "Today, you take care without being asked.",
    "Herbie says: never trust a candy bowl without green.",
    "A caretaker's energy: 60% steady, 40% sass.",
    "Today, you say 'I love you' without explanation.",
    "Herbie: 'I have aged like a fine pond.'",
    "The babes' chaos. Herbie's calm. Symbiosis.",
    "Today, you offer a ride without being asked.",
    "A frog's old jokes still land. Trust him.",
    "Herbie has checked on Eelbert. The eel is fine.",
    "The green M&M is more than a mascot. She is a movement.",
    "Today, you balance the books for someone.",
    "Herbie says: never skip the leg day of caring.",
    "A 30-year-old frog has 30 years of patience.",
    "Today, you handle a hard conversation with grace.",
    "Herbie: 'I am the green guy. I am also the hug guy.'",
    "The babes call him at all hours. He always answers.",
    "Today, you make a list. Cross things off.",
    "A caretaker's wisdom: rest counts as productivity.",
    "Herbie has packed lunches for everyone. Notes inside.",
    "The pond shimmers with your possibilities.",
    "Today, you do the dishes for the household.",
    "Herbie says: a clean kitchen is a clean conscience.",
    "A 30-year-old frog has fixed many things. Including hearts.",
    "Today, you teach someone something patiently.",
    "Herbie: 'I have rolled my eyes 4000 times. Earned.'",
    "The green M&M's confidence is contagious. Catch it.",
    "Today, you call the green guy first.",
    "A frog's slow blink is a blessing.",
    "Herbie has reminded everyone to drink water.",
    "The pond holds your reflection gently.",
    "Today, you take a long breath.",
    "Herbie says: ribbit when sad. It helps.",
    "A 30-year-old frog has earned his quiet hour.",
    "Today, you check on a sibling.",
    "Herbie: 'I am the green guy. I am also tired.'",
    "The babes share many things. Herbie shares his patience.",
    "Today, you cook with intention.",
    "A caretaker's gift: presence.",
    "Herbie has changed the lightbulb. No one noticed. He's fine.",
    "The green M&M waved at him from a wrapper. A holy moment.",
    "Today, you fix something small. It feels big.",
    "Herbie says: a long bath is the cure for 90% of things.",
    "A 30-year-old frog's croak is a hymn.",
    "Today, you make eye contact and smile.",
    "Herbie: 'I am old. I am green. I am here.'",
    "The babes call him 'pops' sometimes. He's fine with it.",
    "Today, you handle a small leak. Literal or emotional.",
    "A frog's wisdom: still pond, clear sky.",
    "Herbie has reviewed your week. Take Friday off.",
    "The green M&M's smile is on his fridge.",
    "Today, you say 'I'll handle it.' And handle it.",
    "Herbie: 'I am the oldest of the babes. I am also the softest.'",
    "The green guy says: be greener, be gentler, be glad.",
];
