//! World Cup 2026 screen — scrollable list of the next 10 upcoming
//! matches of the FIFA World Cup 2026 (48-team format; opens June 11,
//! 2026 at Estadio Azteca, Mexico City; final July 19, 2026 at MetLife
//! Stadium, NJ).
//!
//! Data source: a JSON document fetched from
//!
//!   https://binsbucket.blob.core.windows.net/firmware/wc2026.json
//!
//! with the schema
//!
//! ```json
//! {
//!   "matches": [
//!     {
//!       "kickoff_unix": 1749672000,
//!       "stage": "Group A",
//!       "venue": "Estadio Azteca, Mexico City",
//!       "team_a": "MEX",
//!       "team_b": "TBD",
//!       "team_a_flag": "🇲🇽",
//!       "team_b_flag": ""
//!     }
//!   ]
//! }
//! ```
//!
//! `kickoff_unix` is UTC epoch seconds; `team_a`/`team_b` are 3-letter
//! FIFA-style codes; the flag emoji fields are optional and ignored on the
//! LCD (the font has no emoji glyphs anyway).
//!
//! Fetch happens in a background thread launched by `kick_fetch()` — same
//! pattern as `STATUS_RESULT` in `main.rs`. While in-flight a second call
//! is a no-op. On success the parsed matches are filtered to only those
//! still in the future, sorted ascending by kickoff time, and trimmed to
//! the first 10. The render path uses these if present; otherwise it
//! falls back to the baked-in `FALLBACK_MATCHES` slice so the screen
//! always shows something sensible (even pre-WiFi).
//!
//! Note on fallback matchups: the actual 2026 final draw landed in
//! December 2025 — the bracket below uses real qualifying nations and
//! real opening-stage venues, but the specific pairings are plausible
//! guesses, not the official draw. The fetched JSON overrides them.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_graphics_framebuf::FrameBuf;
use embedded_svc::http::client::Client;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use u8g2_fonts::{
    fonts,
    types::{FontColor, VerticalPosition},
    FontRenderer,
};

use crate::cal::{self, CalEvent, ROW_GAP, ROW_H};
use crate::{VecFb, ACCENT, BODY_LEFT, MUTED, W};

// Local mirror of the Miami offset used in home.rs — keeps the displayed
// time-of-day consistent with the home clock so users can mentally diff
// kickoff against the on-device clock without timezone math.
const ET_OFFSET_S: i64 = -4 * 3600;

const F_MICRO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR08_tf>();
const F_HEADER: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();

const WC_JSON_URL: &str = "https://binsbucket.blob.core.windows.net/firmware/wc2026.json";

/// y of the first row, leaving room for the title strip above. Mirrors
/// birthdays.rs.
const ROWS_TOP_OFFSET: i32 = 22;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// One fallback match — all-borrowed `&'static str` so the slice can sit
/// in `.rodata`.
struct Match {
    kickoff_unix: i64,
    stage: &'static str,
    venue: &'static str,
    team_a: &'static str,
    team_b: &'static str,
}

/// Owned counterpart for matches that arrived via JSON. Same logical
/// fields as `Match` but with `String` so the parser can keep them.
struct OwnedMatch {
    kickoff_unix: i64,
    stage: String,
    venue: String,
    team_a: String,
    team_b: String,
}

#[derive(serde::Deserialize)]
struct MatchJson {
    kickoff_unix: i64,
    #[serde(default)]
    stage: String,
    #[serde(default)]
    venue: String,
    #[serde(default)]
    team_a: String,
    #[serde(default)]
    team_b: String,
    // flag fields are accepted but ignored — no emoji glyphs in the font
    #[allow(dead_code)]
    #[serde(default)]
    team_a_flag: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    team_b_flag: Option<String>,
}

#[derive(serde::Deserialize)]
struct Schedule {
    matches: Vec<MatchJson>,
}

// ---------------------------------------------------------------------------
// Fallback bracket
// ---------------------------------------------------------------------------
//
// Ten plausible early-stage WC2026 matches starting on the opening day,
// June 11, 2026. Times are in UTC seconds (kickoff_unix).
//
// Times-of-day are eyeballed for the venue's local timezone but stored
// as UTC; the render path shows them in ET. They are *placeholders* until
// the JSON fetch lands.
//
// Real venues only. Stage labels use Group A..P (the 48-team / 16-group
// format adopted by FIFA in 2023).

const FALLBACK_MATCHES: &[Match] = &[
    // June 11, 2026 — opener at Azteca, Mexico hosting (per FIFA tradition).
    Match {
        kickoff_unix: 1_781_208_000, // 2026-06-11 20:00 UTC
        stage: "Group A",
        venue: "Estadio Azteca, Mexico City",
        team_a: "MEX",
        team_b: "MAR",
    },
    // June 12, 2026 — Canada hosts at BMO Field, Toronto.
    Match {
        kickoff_unix: 1_781_292_000, // 2026-06-12 19:20 UTC
        stage: "Group B",
        venue: "BMO Field, Toronto",
        team_a: "CAN",
        team_b: "BEL",
    },
    // June 12, 2026 (evening local LA = night ET) — USA at SoFi Stadium.
    Match {
        kickoff_unix: 1_781_316_000, // 2026-06-13 02:00 UTC (= 22:00 ET on the 12th)
        stage: "Group C",
        venue: "SoFi Stadium, Los Angeles",
        team_a: "USA",
        team_b: "KOR",
    },
    // June 13, 2026 — heavyweight at MetLife Stadium, NJ.
    Match {
        kickoff_unix: 1_781_373_600, // 2026-06-13 18:00 UTC
        stage: "Group D",
        venue: "MetLife Stadium, East Rutherford",
        team_a: "ARG",
        team_b: "JPN",
    },
    // June 13, 2026 — France at AT&T Stadium, Arlington.
    Match {
        kickoff_unix: 1_781_388_000, // 2026-06-13 22:00 UTC
        stage: "Group E",
        venue: "AT&T Stadium, Arlington",
        team_a: "FRA",
        team_b: "AUS",
    },
    // June 14, 2026 — Brazil at BC Place, Vancouver.
    Match {
        kickoff_unix: 1_781_461_800, // 2026-06-14 18:30 UTC
        stage: "Group F",
        venue: "BC Place, Vancouver",
        team_a: "BRA",
        team_b: "SUI",
    },
    // June 14, 2026 — England at Gillette Stadium, Foxborough.
    Match {
        kickoff_unix: 1_781_474_400, // 2026-06-14 22:00 UTC
        stage: "Group G",
        venue: "Gillette Stadium, Foxborough",
        team_a: "ENG",
        team_b: "SEN",
    },
    // June 15, 2026 — Germany at Mercedes-Benz Stadium, Atlanta.
    Match {
        kickoff_unix: 1_781_546_400, // 2026-06-15 18:00 UTC
        stage: "Group H",
        venue: "Mercedes-Benz Stadium, Atlanta",
        team_a: "GER",
        team_b: "URU",
    },
    // June 15, 2026 — Spain at Hard Rock Stadium, Miami Gardens.
    Match {
        kickoff_unix: 1_781_560_800, // 2026-06-15 22:00 UTC
        stage: "Group I",
        venue: "Hard Rock Stadium, Miami Gardens",
        team_a: "ESP",
        team_b: "POR",
    },
    // June 16, 2026 — Netherlands at NRG Stadium, Houston.
    Match {
        kickoff_unix: 1_781_632_800, // 2026-06-16 18:00 UTC
        stage: "Group J",
        venue: "NRG Stadium, Houston",
        team_a: "NED",
        team_b: "ECU",
    },
];

// ---------------------------------------------------------------------------
// Async fetch — same pattern as STATUS_RESULT in main.rs
// ---------------------------------------------------------------------------

/// Live (fetched) schedule, sorted ascending by kickoff_unix, capped at 10.
/// `None` until the first successful fetch — render falls back to the
/// hardcoded slice in that case.
static MATCHES: Mutex<Option<Vec<OwnedMatch>>> = Mutex::new(None);

/// Set while a fetch worker is running. A second `kick_fetch` call while
/// this is true is a no-op.
static FETCHING: AtomicBool = AtomicBool::new(false);

/// Spawn the background fetcher. Call from main once after WiFi is up.
/// Subsequent calls while a fetch is in flight are no-ops.
pub fn kick_fetch() {
    if FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawn = std::thread::Builder::new()
        .name("worldcup-fetch".into())
        .stack_size(16 * 1024)
        .spawn(|| {
            log::info!("worldcup: GET {WC_JSON_URL}");
            match fetch_and_parse() {
                Ok(mut list) => {
                    let now = now_unix();
                    list.retain(|m| m.kickoff_unix > now);
                    list.sort_by_key(|m| m.kickoff_unix);
                    list.truncate(10);
                    log::info!("worldcup: kept {} upcoming matches", list.len());
                    *MATCHES.lock().unwrap() = Some(list);
                }
                Err(e) => {
                    log::warn!("worldcup: fetch failed: {e}");
                    // Leave MATCHES as-is — fallback (or last good data) keeps showing.
                }
            }
            FETCHING.store(false, Ordering::SeqCst);
        });
    if let Err(e) = spawn {
        log::warn!("worldcup: failed to spawn fetcher: {e}");
        FETCHING.store(false, Ordering::SeqCst);
    }
}

fn fetch_and_parse() -> anyhow::Result<Vec<OwnedMatch>> {
    let conn = EspHttpConnection::new(&HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(Duration::from_secs(20)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(conn);
    let mut response = client.get(WC_JSON_URL)?.submit()?;
    let status = response.status();
    if !(200..300).contains(&status) {
        anyhow::bail!("worldcup: HTTP {status}");
    }

    let mut body: Vec<u8> = Vec::with_capacity(4096);
    let mut buf = [0u8; 512];
    loop {
        let n = response.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
        // Cap body to avoid runaway alloc — the real document is well
        // under this.
        if body.len() > 64 * 1024 {
            break;
        }
    }

    let parsed: Schedule = serde_json::from_slice(&body)?;
    let owned = parsed
        .matches
        .into_iter()
        .map(|m| OwnedMatch {
            kickoff_unix: m.kickoff_unix,
            stage: m.stage,
            venue: m.venue,
            team_a: m.team_a,
            team_b: m.team_b,
        })
        .collect();
    Ok(owned)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Row-source helpers
// ---------------------------------------------------------------------------
//
// `ViewMatch` is a borrow-only flattened view of a single match used by
// the row renderer. Both source paths (live `OwnedMatch` and `&'static`
// fallback `Match`) populate it identically, keeping the actual
// `draw_row` body source-agnostic.

struct ViewMatch<'a> {
    kickoff_unix: i64,
    stage: &'a str,
    venue: &'a str,
    team_a: &'a str,
    team_b: &'a str,
}

fn live_view_len() -> usize {
    MATCHES
        .lock()
        .map(|g| g.as_ref().map(|v| v.len()).unwrap_or(0))
        .unwrap_or(0)
}

/// Number of rows currently displayable (post-merge of fallback + fetched).
/// If live data is present, we show only that (already trimmed to ≤10).
/// Otherwise we fall back to FALLBACK_MATCHES.
pub fn row_count() -> usize {
    let live = live_view_len();
    if live > 0 {
        live
    } else {
        FALLBACK_MATCHES.len()
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render the screen.
pub fn render(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    scroll: usize,
    body_top: i32,
    body_bottom: i32,
) {
    // ----- title strip -----
    let _ = F_MICRO.render(
        "WORLD CUP 2026",
        Point::new(10, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(ACCENT),
        fb,
    );
    let title_count = row_count();
    let sub = if live_view_len() > 0 {
        format!("{} upcoming · live", title_count)
    } else {
        format!("{} upcoming · FIFA", title_count)
    };
    let _ = F_MICRO.render(
        sub.as_str(),
        Point::new(108, body_top + 5),
        VerticalPosition::Top,
        FontColor::Transparent(MUTED),
        fb,
    );
    // Keep F_HEADER referenced for future use / parity with the spec.
    let _ = F_HEADER;

    // ----- render visible window -----
    //
    // Two sources need rendering through the same widget. We branch on
    // whether the Mutex is populated so we can keep its lock held across
    // the row loop (the rows reference borrowed slices from inside it).
    // The lock-scoped path also computes effective_total under the lock,
    // protecting against a swap-in of a shorter live vec mid-render.
    let rows_top = body_top + ROWS_TOP_OFFSET;
    let row_pitch = ROW_H + ROW_GAP;
    let avail = (body_bottom - rows_top).max(0);
    let visible_rows = (avail / row_pitch) as usize;

    let (effective_total, end) = {
        let guard = MATCHES.lock().ok();
        // The live source is rendered while still holding the lock so the
        // borrowed slice doesn't dangle. The fallback source is &'static so
        // it doesn't need the lock at all — but holding it through the call
        // is harmless and keeps the code simple.
        let live_ref: Option<&Vec<OwnedMatch>> = guard
            .as_ref()
            .and_then(|g| g.as_ref())
            .filter(|l| !l.is_empty());
        if let Some(list) = live_ref {
            let total = list.len();
            let start = scroll.min(total);
            let end = (start + visible_rows).min(total);
            draw_window_live(fb, list, start, end, rows_top, row_pitch);
            (total, end)
        } else {
            let total = FALLBACK_MATCHES.len();
            let start = scroll.min(total);
            let end = (start + visible_rows).min(total);
            draw_window_fallback(fb, start, end, rows_top, row_pitch);
            (total, end)
        }
    };

    // ----- scroll indicators -----
    if scroll > 0 {
        draw_up_indicator(fb, W as i32 - BODY_LEFT - 6, body_top + ROWS_TOP_OFFSET - 6);
    }
    if end < effective_total {
        draw_down_indicator(fb, W as i32 - BODY_LEFT - 6, body_bottom - 8);
    }
}

fn draw_window_live(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    list: &[OwnedMatch],
    start: usize,
    end: usize,
    rows_top: i32,
    row_pitch: i32,
) {
    for (i, idx) in (start..end).enumerate() {
        let m = &list[idx];
        let vm = ViewMatch {
            kickoff_unix: m.kickoff_unix,
            stage: m.stage.as_str(),
            venue: m.venue.as_str(),
            team_a: m.team_a.as_str(),
            team_b: m.team_b.as_str(),
        };
        draw_row(fb, rows_top + (i as i32) * row_pitch, &vm, idx == 0);
    }
}

fn draw_window_fallback(
    fb: &mut FrameBuf<Rgb565, VecFb>,
    start: usize,
    end: usize,
    rows_top: i32,
    row_pitch: i32,
) {
    for (i, idx) in (start..end).enumerate() {
        let m = &FALLBACK_MATCHES[idx];
        let vm = ViewMatch {
            kickoff_unix: m.kickoff_unix,
            stage: m.stage,
            venue: m.venue,
            team_a: m.team_a,
            team_b: m.team_b,
        };
        draw_row(fb, rows_top + (i as i32) * row_pitch, &vm, idx == 0);
    }
}

fn draw_row(fb: &mut FrameBuf<Rgb565, VecFb>, y: i32, m: &ViewMatch, highlight: bool) {
    // Date label uses the ET-local Y/M/D so it matches the time-of-day
    // shown below it. (UTC kickoff + ET offset → local civil date.)
    let local_secs = m.kickoff_unix + ET_OFFSET_S;
    let (_y, mo, d) = cal::unix_to_ymd(local_secs);
    let date_label = format!("{} {}", cal::month_abbr(mo), d);

    // Time-of-day in ET, 24-hour, e.g., "20:00 ET".
    let mod_day = local_secs.rem_euclid(cal::SECS_PER_DAY) as u32;
    let h = mod_day / 3600;
    let mm = (mod_day % 3600) / 60;
    let date_sub = format!("{:02}:{:02} ET", h, mm);

    let title = format!("{} vs {}", m.team_a, m.team_b);
    let venue_short = shorten_venue(m.venue);
    let subtitle = format!("{} · {}", m.stage, venue_short);

    let _ = cal::render_row(
        fb,
        y,
        &CalEvent {
            date_label: date_label.as_str(),
            date_sub: date_sub.as_str(),
            title: title.as_str(),
            subtitle: subtitle.as_str(),
            highlight,
        },
    );
}

/// Collapse a "<stadium>, <city>" venue down to just the stadium name so
/// it fits the row's narrow subtitle column. Falls back to the raw string
/// if there's no comma.
fn shorten_venue(venue: &str) -> &str {
    match venue.find(',') {
        Some(i) => venue[..i].trim_end(),
        None => venue,
    }
}

// ---------------------------------------------------------------------------
// Scroll indicators — tiny chevrons in the right margin
// ---------------------------------------------------------------------------

fn draw_down_indicator(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32) {
    Rectangle::new(Point::new(x, y), Size::new(5, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .ok();
    Rectangle::new(Point::new(x + 1, y + 1), Size::new(3, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .ok();
    Rectangle::new(Point::new(x + 2, y + 2), Size::new(1, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .ok();
}

fn draw_up_indicator(fb: &mut FrameBuf<Rgb565, VecFb>, x: i32, y: i32) {
    Rectangle::new(Point::new(x + 2, y), Size::new(1, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .ok();
    Rectangle::new(Point::new(x + 1, y + 1), Size::new(3, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .ok();
    Rectangle::new(Point::new(x, y + 2), Size::new(5, 1))
        .into_styled(PrimitiveStyle::with_fill(ACCENT))
        .draw(fb)
        .ok();
}
