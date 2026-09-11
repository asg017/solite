//! Color depth: how much of a [`ColorValue`](crate::ColorValue) the terminal
//! can actually render.
//!
//! Truecolor (`38;2;R;G;B`) is not universally supported. Terminals that do
//! support it advertise it with `COLORTERM=truecolor` (or `24bit`); on
//! everything else a 24-bit escape is either ignored or mangled, so `Rgb`
//! values are downgraded to the nearest xterm-256 palette index at *emission
//! time* — the theme itself keeps the exact color, which matters for the HTML
//! surfaces (CSS has no such limit) and for round-tripping a theme file.
//!
//! The depth is a process-global, set once from the CLI (`colors::init`)
//! exactly like the color-gating decision. It defaults to
//! [`ColorDepth::TrueColor`], so anything that never calls
//! [`set_color_depth`] behaves as it always did.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::color::{xterm256_rgb, ColorValue};

/// How many colors the output surface can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorDepth {
    /// 24-bit color: `Rgb` values are emitted as-is.
    #[default]
    TrueColor,
    /// 256-color palette: `Rgb` values are downgraded to the nearest index.
    Ansi256,
}

const TRUECOLOR: u8 = 0;
const ANSI256: u8 = 1;

static DEPTH: AtomicU8 = AtomicU8::new(TRUECOLOR);

/// Set the process-wide emission depth. Call once, early (from `colors::init`).
pub fn set_color_depth(depth: ColorDepth) {
    let v = match depth {
        ColorDepth::TrueColor => TRUECOLOR,
        ColorDepth::Ansi256 => ANSI256,
    };
    DEPTH.store(v, Ordering::Relaxed);
}

/// The current process-wide emission depth.
pub fn color_depth() -> ColorDepth {
    match DEPTH.load(Ordering::Relaxed) {
        ANSI256 => ColorDepth::Ansi256,
        _ => ColorDepth::TrueColor,
    }
}

/// Detect the terminal's color depth from the environment: `COLORTERM` equal
/// to `truecolor` or `24bit` means 24-bit color is safe, anything else (unset
/// included) does not.
pub fn detect_color_depth() -> ColorDepth {
    match std::env::var("COLORTERM") {
        Ok(v) if v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit") => {
            ColorDepth::TrueColor
        }
        _ => ColorDepth::Ansi256,
    }
}

/// The xterm-256 palette index closest to an RGB triple.
///
/// Only searches 16-255: the first sixteen entries are the user's own
/// terminal palette and have no fixed RGB, so matching against their nominal
/// values would produce colors that look nothing like the request.
pub fn nearest_xterm256(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 16u8;
    let mut best_distance = u32::MAX;
    for index in 16..=255u8 {
        let (cr, cg, cb) = xterm256_rgb(index);
        let dr = cr as i32 - r as i32;
        let dg = cg as i32 - g as i32;
        let db = cb as i32 - b as i32;
        let distance = (dr * dr + dg * dg + db * db) as u32;
        if distance < best_distance {
            best_distance = distance;
            best = index;
            if distance == 0 {
                break;
            }
        }
    }
    best
}

impl ColorValue {
    /// This value as the given depth can express it: [`ColorValue::Rgb`]
    /// becomes the nearest [`ColorValue::Indexed`] at
    /// [`ColorDepth::Ansi256`]; everything else is returned unchanged.
    pub fn downgrade(self, depth: ColorDepth) -> ColorValue {
        match (self, depth) {
            (ColorValue::Rgb(r, g, b), ColorDepth::Ansi256) => {
                ColorValue::Indexed(nearest_xterm256(r, g, b))
            }
            (other, _) => other,
        }
    }

    /// This value as the *process-wide* depth can express it. Used by the SGR
    /// emitters, so `Style::paint` needs no depth argument.
    pub fn emitted(self) -> ColorValue {
        self.downgrade(color_depth())
    }
}
