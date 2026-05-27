//! Optional compile-time WiFi credentials.
//!
//! Leave these empty (the default) and the device's first boot will start
//! the captive-portal AP for you to provision over the air — that's the
//! supported path.
//!
//! Fill them in if you want a brand-new board to come up online with no
//! interaction the first time. They're used **only** when NVS is empty:
//! once the device successfully connects with them it persists the creds to
//! NVS, and from that point on this file is irrelevant. The "forget WiFi"
//! gesture (hold the encoder 2 s) wipes NVS — if these are non-empty the
//! migration will then re-populate NVS on the next boot, which is usually
//! _not_ what you want when testing the captive-portal flow.
//!
//! Anything you put here is baked into the binary. Don't commit real
//! passwords to a public repo.
pub const SSID: &str = "";
pub const PASS: &str = "";
