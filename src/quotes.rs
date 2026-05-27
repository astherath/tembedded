//! QUOTES screen — a slow-roll motivational quote display.
//!
//! Shows one of 100 hardcoded short motivation quotes at a time, centered
//! and wrapped to fit the screen body. Auto-rotates every 60 seconds.
//!
//! Rotation order is a Fisher-Yates shuffled permutation of all 100
//! indices, regenerated each time the cycle completes — so every quote
//! is shown exactly once before any repeats (no-repeat-within-cycle
//! guarantee). Randomness comes from `esp_idf_svc::sys::esp_random()`.
//!
//! A tiny circular countdown sits in the top-right of the body: an
//! accent arc sweeps clockwise from 12 as the current quote ages,
//! emptying the inner ring just before the next rotation.
//!
//! Public surface:
//!   - `Quotes::new()`           — construct + perform initial shuffle.
//!   - `Quotes::tick(now_ms)`    — call each frame; returns true when
//!                                 the displayed quote just changed
//!                                 (caller flags the framebuffer dirty).
//!   - `render(fb, q, body_top, body_bottom, tick)` — draw a frame.
//!   - `QUOTES`                  — the const array of 100 quotes.
//!
//! All `now_ms` math uses `wrapping_sub` so a `u32` millisecond counter
//! wrapping (every ~49.7 days from u32::MAX ms, or ~248 days when the
//! caller passes `state.tick * 5` and `state.tick` itself wraps) cannot
//! pin the display on a single quote forever.

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

const F_QUOTE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR12_tf>();
const F_HEADER: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();

/// Rotation cadence — one full minute per quote.
const ROTATE_MS: u32 = 60_000;

/// How many chars per wrapped line we aim for at the F_QUOTE size.
/// Body width is ~300 px; helvR12 averages ~7 px/char, so ~32 is a
/// comfortable target that leaves margin on both sides.
const WRAP_CHARS: usize = 32;

/// Countdown ring radius (outer). Small, sits next to the counter.
const RING_R: i32 = 7;

pub struct Quotes {
    /// Shuffled permutation of 0..100. Indices into `QUOTES`.
    order: [u8; 100],
    /// Cursor into `order` — the quote currently displayed.
    cursor: u8,
    /// Monotonic ms timestamp of the last rotation. Used with
    /// `wrapping_sub` to compute elapsed time since the last rotation.
    last_rotation_ms: u32,
    /// `true` once `tick` has been called at least once; lets us seed
    /// `last_rotation_ms` from the first observed `now_ms` instead of
    /// 0 (which would make the first rotation happen prematurely if
    /// the device's tick has already been counting for a while when
    /// the user navigates onto this screen).
    seeded: bool,
}

impl Quotes {
    pub fn new() -> Self {
        let mut order: [u8; 100] = [0; 100];
        for (i, slot) in order.iter_mut().enumerate() {
            *slot = i as u8;
        }
        fisher_yates(&mut order);
        Self {
            order,
            cursor: 0,
            last_rotation_ms: 0,
            seeded: false,
        }
    }

    /// Index of the current quote within `QUOTES`.
    #[inline]
    fn current_index(&self) -> usize {
        self.order[self.cursor as usize] as usize
    }

    /// Returns the position within the rotation cycle (1-based) for
    /// display purposes ("#7 of 100").
    #[inline]
    fn display_number(&self) -> u32 {
        self.cursor as u32 + 1
    }

    /// Called on each frame from main.rs's event loop. `now_ms` is the
    /// monotonic millisecond timestamp (e.g., `state.tick * 5`).
    /// Returns `true` if the displayed quote changed this tick so the
    /// caller can mark the framebuffer dirty.
    pub fn tick(&mut self, now_ms: u32) -> bool {
        if !self.seeded {
            self.last_rotation_ms = now_ms;
            self.seeded = true;
            return false;
        }
        let elapsed = now_ms.wrapping_sub(self.last_rotation_ms);
        if elapsed >= ROTATE_MS {
            self.advance();
            // Set the next rotation anchor to "now" — using
            // `wrapping_add(ROTATE_MS)` on the previous anchor would
            // be more drift-resistant in principle, but with a 60 s
            // cadence and 5 ms ticks we never accumulate meaningful
            // drift, and anchoring to `now_ms` makes the countdown
            // arc visually correct after a screen revisit.
            self.last_rotation_ms = now_ms;
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        self.cursor = self.cursor.wrapping_add(1);
        if self.cursor as usize >= self.order.len() {
            // Finished a full cycle of 100 — reshuffle so the next
            // cycle is a fresh permutation.
            fisher_yates(&mut self.order);
            self.cursor = 0;
        }
    }

    /// Milliseconds remaining until the next rotation (0..=ROTATE_MS).
    /// Computed on demand; the renderer uses it for the countdown ring.
    fn remaining_ms(&self, now_ms: u32) -> u32 {
        if !self.seeded {
            return ROTATE_MS;
        }
        let elapsed = now_ms.wrapping_sub(self.last_rotation_ms);
        if elapsed >= ROTATE_MS {
            0
        } else {
            ROTATE_MS - elapsed
        }
    }
}

// ---------------------------------------------------------------------------
// Random / shuffle
// ---------------------------------------------------------------------------

fn esp_random_u32() -> u32 {
    unsafe { esp_idf_svc::sys::esp_random() }
}

/// In-place Fisher-Yates on `arr`. Uses `esp_random()` modulo the
/// shrinking range — bias is negligible at len=100 (range never
/// exceeds 100, so the modulo bias is well under 1 part in 4e7).
fn fisher_yates(arr: &mut [u8; 100]) {
    for i in (1..arr.len()).rev() {
        let j = (esp_random_u32() as usize) % (i + 1);
        arr.swap(i, j);
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn render(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    q: &Quotes,
    body_top: i32,
    body_bottom: i32,
    tick: u32,
) {
    // -------- title strip --------
    // F_HEADER (helvB12) gives the screen title visual weight, matching
    // the bold-header pattern used by the primary clock on the home screen.
    let _ = F_HEADER.render(
        "QUOTES",
        Point::new(10, body_top + 3),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );

    let counter = format!("#{} of {}", q.display_number(), QUOTES.len());
    let counter_w_px: i32 = 56; // approximate; F_MICRO is small
    // Position counter so it ends at right edge minus space for the ring.
    let ring_cx = W as i32 - 14;
    let ring_cy = body_top + 11;
    let counter_x = ring_cx - RING_R - 4 - counter_w_px;
    let _ = F_MICRO.render(
        counter.as_str(),
        Point::new(counter_x, body_top + 7),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );

    // -------- countdown ring --------
    // `now_ms` reconstructed from tick (tick counts 5 ms units in main.rs;
    // this matches what the caller passes to `tick(now_ms)`).
    let now_ms = tick.wrapping_mul(5);
    let remaining = q.remaining_ms(now_ms);
    draw_countdown_ring(fb, ring_cx, ring_cy, RING_R, remaining);

    // Thin separator under the title strip.
    Rectangle::new(
        Point::new(10, body_top + 20),
        Size::new((W as i32 - 20) as u32, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(rgb(40, 50, 70)))
    .draw(fb)
    .unwrap();

    // -------- quote body --------
    let text = QUOTES[q.current_index()];
    let lines = wrap_text(text, WRAP_CHARS);
    let line_h: i32 = 16; // helvR12_tf is ~12 px tall; 16 gives breathing room
    let max_lines = 5;
    let shown = lines.len().min(max_lines);
    let block_h = (shown as i32) * line_h;

    // Vertically center the wrapped block within the body, leaving room
    // for the title strip at the top and the footer hint at the bottom.
    let body_h = body_bottom - body_top;
    let avail_top = body_top + 26;
    let avail_bottom = body_bottom - 16;
    let avail_h = (avail_bottom - avail_top).max(0);
    let pad_top = ((avail_h - block_h) / 2).max(0);
    let text_top = avail_top + pad_top;

    for (i, line) in lines.iter().take(max_lines).enumerate() {
        let y = text_top + (i as i32) * line_h;
        // Center each line horizontally by estimating width from char
        // count (helvR12 is roughly proportional; a ~7 px/char average
        // is close enough that centering looks intentional even though
        // it's not pixel-perfect).
        let approx_w_px = (line.chars().count() as i32) * 7;
        let x = ((W as i32 - approx_w_px) / 2).max(10);
        let _ = F_QUOTE.render(
            line.as_str(),
            Point::new(x, y),
            VerticalPosition::Top,
            FontColor::Transparent(FG),
            fb,
        );
    }

    // Quiet decoration: a small accent stripe to the left of the quote
    // block — only when there's room for it. Mirrors the "primary card"
    // accent stripe pattern used by `home.rs` and `cal.rs`.
    if shown >= 2 && body_h >= 80 {
        let bar_h = (block_h - 4).max(8);
        let bar_y = text_top + 2;
        Rectangle::new(Point::new(4, bar_y), Size::new(2, bar_h as u32))
            .into_styled(PrimitiveStyle::with_fill(PANEL))
            .draw(fb)
            .unwrap();
        Rectangle::new(Point::new(4, bar_y), Size::new(2, 6))
            .into_styled(PrimitiveStyle::with_fill(ACCENT))
            .draw(fb)
            .unwrap();
    }

    // -------- footer hint --------
    let secs_left = (remaining + 999) / 1000; // round up so "1s" lingers
    let hint = format!("rotates every 60s  ·  next in {}s", secs_left);
    let _ = F_MICRO.render(
        hint.as_str(),
        Point::new(10, body_bottom - 12),
        VerticalPosition::Top,
        FontColor::Transparent(OK),
        fb,
    );
}

// ---------------------------------------------------------------------------
// Word-wrap helper (adapted from `src/fortune.rs::wrap_text`).
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Countdown ring
// ---------------------------------------------------------------------------

/// Draw the countdown ring. `remaining_ms` shrinks from ROTATE_MS down to 0;
/// the inner accent arc sweeps OUT (clockwise from 12) as the quote ages,
/// so a freshly-rotated quote shows a full ring and a near-due quote
/// shows nearly nothing.
fn draw_countdown_ring(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    cx: i32,
    cy: i32,
    r: i32,
    remaining_ms: u32,
) {
    // Muted outer gutter.
    draw_circle_outline(fb, cx, cy, r, MUTED);
    // Inner-radius accent points, only for angles that haven't elapsed yet.
    // Fraction remaining: 1.0 just after rotation, ~0.0 right before.
    let frac_remaining = (remaining_ms as f32) / (ROTATE_MS as f32);
    let frac_remaining = frac_remaining.clamp(0.0, 1.0);

    // We sweep CLOCKWISE FROM 12: as time elapses the arc shrinks from
    // 360° down to 0° of remaining-time. So the arc that should be drawn
    // spans angles [0°, 360° * frac_remaining), measured clockwise from
    // 12 o'clock (i.e., the top of the circle).
    let arc_deg = (360.0 * frac_remaining) as i32;
    if arc_deg <= 0 {
        return;
    }

    // Inner fill radius — leaves a 1 px gap to the outline.
    let inner_r = (r - 2).max(1);
    // Step in 3° increments for a smooth-enough arc at this size. The
    // inner area is small (radius 5 px), so this is plenty.
    let step_deg = 3;
    let mut a = 0;
    while a < arc_deg {
        // Convert "clockwise from 12" to standard math angle:
        //   screen y grows downward, so clockwise-from-top in screen
        //   space corresponds to a positive rotation from the +y axis
        //   pointing UP (i.e., -screen_y).
        let theta = (a as f32) * core::f32::consts::PI / 180.0;
        // Unit vector for "clockwise from 12":
        //   dx =  sin(theta)
        //   dy = -cos(theta)   (negative because screen y is inverted)
        let dx = theta.sin();
        let dy = -theta.cos();
        // Fill radii 0..=inner_r along the ray, so the wedge is solid.
        let mut rr = 0;
        while rr <= inner_r {
            let px = cx + (dx * rr as f32).round() as i32;
            let py = cy + (dy * rr as f32).round() as i32;
            set_pixel(fb, px, py, ACCENT);
            rr += 1;
        }
        a += step_deg;
    }
}

#[inline]
fn set_pixel(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32, color: Rgb565) {
    if x < 0 || y < 0 || x >= crate::W as i32 || y >= crate::H as i32 {
        return;
    }
    fb.data.0[(y as usize) * crate::W + (x as usize)] = color;
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

// =========================================================================
// THE 100 QUOTES
//
// Punchy, varied in tone: Stoic, modern, athletic, philosophical. All
// short (under ~80 chars). No author attribution. No verbatim quoting
// of copyrighted works — these are paraphrases or originals.
// =========================================================================

pub const QUOTES: [&str; 100] = [
    // ----- Stoic / classical -----
    "The obstacle in the way becomes the way.",
    "You have power over your mind, not outside events.",
    "Waste no more time arguing what a good person should be. Be one.",
    "What stands in the way of action advances action.",
    "The best revenge is to be unlike the one who wronged you.",
    "Difficulty shows what we are made of.",
    "First say to yourself what you would be, then do what you have to do.",
    "Choose not to be harmed and you will not feel harmed.",
    "If it is not right, do not do it. If it is not true, do not say it.",
    "He who fears death will never do anything worthy of a living person.",

    // ----- Discipline / craft -----
    "Discipline is the bridge between goals and accomplishment.",
    "Show up before you feel ready.",
    "Amateurs wait for inspiration. The rest of us get to work.",
    "Small habits, repeated. That's the whole secret.",
    "You don't rise to the level of your goals; you fall to your systems.",
    "Reps over rumination.",
    "Boring consistency beats heroic effort.",
    "Master the basics until they're invisible.",
    "The grind is the gift.",
    "Make the boring thing the daily thing.",

    // ----- Athletic / momentum -----
    "Pain is fuel if you let it be.",
    "Train for the moment you stop wanting to.",
    "Hard now, easy later. Easy now, hard later. Choose.",
    "The body keeps the receipts.",
    "You'll never regret the workout you did.",
    "Outwork your doubts.",
    "Lungs first, ego later.",
    "Pace, don't sprint. The race is long.",
    "Strong is what you build, not what you're given.",
    "Sweat is just stress leaving the body.",

    // ----- Philosophical / wide -----
    "The unexamined life is not worth the rush.",
    "Be quick to listen, slow to react.",
    "Wisdom begins in noticing.",
    "The longer the question, the shorter the honest answer.",
    "The map will never be the territory. Walk anyway.",
    "Certainty is the enemy of growth.",
    "Time is the only currency that doesn't compound back.",
    "You become what you repeatedly attend to.",
    "Most of what you fear will never happen. Some of it already has.",
    "Endings are the price of beginnings.",

    // ----- Modern / direct -----
    "Done is the engine. Perfect is the brake.",
    "Start ugly. Finish anyway.",
    "If it scares you a little, it's probably the move.",
    "Stop waiting for the right time. Make this time right.",
    "Your future self is begging you to start today.",
    "Comparison is the thief of momentum.",
    "Burn the boats, then look for the boats.",
    "Stop polishing the plan. Ship the plan.",
    "What got you here is what kept you here. Change one thing.",
    "The cost of inaction is silent and enormous.",

    // ----- Resilience / setbacks -----
    "Falling is not failing. Staying down is.",
    "You can lose every round but the last and still win.",
    "Scars are receipts for surviving.",
    "The wound is where the light gets in next time.",
    "Bend, don't break. Then straighten up.",
    "What didn't kill you is now in your skill tree.",
    "Setbacks are setups in disguise.",
    "Resilience is just stubbornness with a better PR team.",
    "The bottom is a great place to push off from.",
    "Try again. Fail again. Fail less.",

    // ----- Identity / courage -----
    "Be the person you needed when you were younger.",
    "Quiet courage is still courage.",
    "Take up the space you've earned.",
    "Apologize for nothing you'd do again the same way.",
    "If the world calls you intense, the world's just tired.",
    "You don't need permission to begin.",
    "Make your life unrecognizable to your former self.",
    "Be loud about the things that scare you most.",
    "The person you're becoming is watching what you do today.",
    "Walk like someone already chose you.",

    // ----- Focus / clarity -----
    "Subtract until what remains is essential.",
    "The hard part is choosing what to ignore.",
    "Focus is the rarest and most expensive resource you own.",
    "If you chase two rabbits, you'll lose both rabbits.",
    "You can do anything, but not everything. Pick.",
    "Saying yes to one thing is saying no to a thousand others.",
    "Cut the busy. Keep the important.",
    "Clarity beats cleverness.",
    "What you tolerate becomes your standard.",
    "Slow is smooth. Smooth is fast.",

    // ----- Gratitude / presence -----
    "This moment is the youngest you'll ever be again.",
    "Notice it now. The future won't have this exact light.",
    "The day you have is the day you have.",
    "If you can taste your coffee, you're already winning.",
    "Most ordinary days will be missed eventually.",
    "Be early to your own life.",
    "The view is better when you stop checking your phone.",
    "Today will be a story someday. Make it a good one.",
    "Joy is a skill. Practice it.",
    "There is no later. There is only what you do next.",

    // ----- Closing / charged -----
    "Decide once. Act often.",
    "Done scares perfect.",
    "The best time was earlier. The next best is now.",
    "Bet on yourself. The odds are better than you think.",
    "Make the call you've been avoiding.",
    "Less explaining. More doing.",
    "Your future is built from boring afternoons.",
    "Refuse to be the bottleneck in your own life.",
    "Run your own race at your own pace.",
    "Begin again, and again, and again.",
];
