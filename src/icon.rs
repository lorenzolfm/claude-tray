//! How to draw the Claude mark, and a badge beside it, into a tray pixmap.
//!
//! This module is built on one measurement: the width costs nothing. Waybar scales a tray pixmap
//! to `icon-size` in height and keeps the aspect ratio in width (`src/modules/sni/item.cpp`,
//! `Item::updateImage`). A tray icon is thus a height limit and not a 20×20 box, so the mark and
//! a count fit side by side without a crop. This was measured in a real bar.
//!
//! Render at the target height and never larger. Waybar scales an h40 pixmap down, and the
//! result is more blurred than an h20 one. [`crate::mark`] therefore rasterises the SVG at
//! exactly [`HEIGHT`] instead of a bitmap for Waybar to resample.
//!
//! The colour is in the pixels, because CSS cannot supply it.
//! [`ksni::Status::NeedsAttention`] adds a `needs-attention` CSS class, but a tray item is a
//! `Gtk::Image`: `color` has no effect on it, and a border is the only signal that CSS can give.
//! Each colour here has one meaning. The mark is always [`mark::CLAUDE`], and the badge is
//! [`BLOCKED`] or [`FAULT`]. See `README.md`.

use crate::mark;
use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont, point};
use ksni::Icon;

/// Waybar's `icon-size` on this machine. The rule above says to render at exactly this height.
/// The mark rasterises to it, so a different value stays sharp instead of resampled.
pub const HEIGHT: u32 = 20;

/// The count. Each agent in it waits for you. This is the same amber as the `needs-attention`
/// border in `style.css`, because it has the same meaning: nothing moves, look now.
///
/// If a second colour becomes necessary, note the result of an earlier test: the terracotta of
/// the mark is the least visible colour on the bar. That is the wrong result, because you
/// already know the mark and you must read the number.
pub const BLOCKED: [u8; 3] = [0xE5, 0xC0, 0x7B];

/// The producer is absent or it fails. This does not mean "you have work". It means that the
/// applet cannot see, which is the one failure that must not look like a quiet state.
pub const FAULT: [u8; 3] = [0xE0, 0x6C, 0x75];

/// Where to look when `CLAUDE_TRAY_FONT` has no value. The nix package sets that variable to a
/// store path, so this list applies only to a `cargo run` build.
const FALLBACK_FONTS: &[&str] = &[
    "/run/current-system/sw/share/X11/fonts/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

/// The characters that the badge needs from a font: the digits and `⊘`. DejaVu Sans has them,
/// which is why this code names it instead of the fontconfig default.
const FAMILY: &str = "DejaVu Sans";

pub struct Renderer {
    font: Vec<u8>,
    /// The mark, rasterised one time. Its shape and size do not change.
    mark: Vec<u8>,
}

impl Renderer {
    /// Find a usable font, in order: the wrapper's variable, fontconfig, then known paths.
    pub fn load() -> Result<Self, String> {
        let mark = mark::mask(HEIGHT);
        let mut tried: Vec<String> = Vec::new();

        if let Ok(p) = std::env::var("CLAUDE_TRAY_FONT") {
            match std::fs::read(&p) {
                Ok(font) => return Ok(Self { font, mark }),
                Err(e) => tried.push(format!("CLAUDE_TRAY_FONT={p}: {e}")),
            }
        }

        if let Some(p) = fc_match() {
            match std::fs::read(&p) {
                Ok(font) => return Ok(Self { font, mark }),
                Err(e) => tried.push(format!("fc-match {p}: {e}")),
            }
        }

        for p in FALLBACK_FONTS {
            if let Ok(font) = std::fs::read(p) {
                return Ok(Self { font, mark });
            }
        }

        Err(format!(
            "no font with the digits and \u{2298}; tried {FAMILY} via fontconfig and {} known \
             paths{}",
            FALLBACK_FONTS.len(),
            if tried.is_empty() {
                String::new()
            } else {
                format!(" ({})", tried.join("; "))
            }
        ))
    }

    /// Rasterise the mark, then `badge` beside it in `rgb`, into an ARGB32 pixmap of exactly
    /// [`HEIGHT`] rows. An empty `badge` gives a square icon that holds the mark alone, at the
    /// same width as each other item in the tray.
    ///
    /// `ksni::Icon` needs ARGB32 in network byte order: the bytes are `A, R, G, B` and not the
    /// little-endian `B, G, R, A` of a `u32` in memory. It also needs straight alpha and not
    /// premultiplied alpha, because Waybar sends the buffer to a `GdkPixbuf`.
    pub fn render(&self, badge: &str, rgb: [u8; 3]) -> Icon {
        let height = HEIGHT;
        let font = FontRef::try_from_slice(&self.font).expect("validated at load");

        // `⊘` is small in DejaVu's em box and looks thin beside the digits at one scale. It
        // therefore has its own scale.
        let digit_scale = PxScale::from(height as f32 * 0.68);
        let glyph_scale = PxScale::from(height as f32 * 0.86);
        let scale_for = |c: char| {
            if c.is_ascii_digit() {
                digit_scale
            } else {
                glyph_scale
            }
        };

        // The mark fills its square, like each other tray icon. The gap keeps the mark and the
        // count separate, and the pad at the end keeps the digit away from the edge.
        let gap = (height as f32 * 0.16).max(1.0);
        let tail = (height as f32 * 0.12).max(1.0);
        let advance: f32 = badge
            .chars()
            .map(|c| font.as_scaled(scale_for(c)).h_advance(font.glyph_id(c)))
            .sum();
        let width = if badge.is_empty() {
            height
        } else {
            (height as f32 + gap + advance + tail).ceil() as u32
        };

        let mut data = vec![0u8; (width * height * 4) as usize];
        let mut put = |x: u32, y: u32, alpha: u8, rgb: [u8; 3]| {
            let i = ((y * width + x) * 4) as usize;
            if alpha > data[i] {
                data[i] = alpha;
                data[i + 1] = rgb[0];
                data[i + 2] = rgb[1];
                data[i + 3] = rgb[2];
            }
        };

        for y in 0..height {
            for x in 0..height {
                let a = self.mark[(y * height + x) as usize];
                if a > 0 {
                    put(x, y, a, mark::CLAUDE);
                }
            }
        }

        if badge.is_empty() {
            return Icon {
                width: width as i32,
                height: height as i32,
                data,
            };
        }

        // Centre the ink box vertically instead of the text on the baseline. The bar gives no
        // vertical space for an error of one pixel.
        let ref_scaled = font.as_scaled(glyph_scale);
        let text_h = ref_scaled.ascent() - ref_scaled.descent();
        let baseline = (height as f32 - text_h) / 2.0 + ref_scaled.ascent();

        let mut caret = height as f32 + gap;
        for c in badge.chars() {
            let scale = scale_for(c);
            let id = font.glyph_id(c);
            let positioned = id.with_scale_and_position(scale, point(caret, baseline));
            caret += font.as_scaled(scale).h_advance(id);

            let Some(outline) = font.outline_glyph(positioned) else {
                continue; // a space, or a glyph this font does not have
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = gx as i32 + bounds.min.x as i32;
                let py = gy as i32 + bounds.min.y as i32;
                if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                    return;
                }
                put(px as u32, py as u32, (coverage * 255.0) as u8, rgb);
            });
        }

        Icon {
            width: width as i32,
            height: height as i32,
            data,
        }
    }
}

fn fc_match() -> Option<String> {
    let out = std::process::Command::new("fc-match")
        .args(["--format=%{file}", FAMILY])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Option<Renderer> {
        Renderer::load().ok()
    }

    /// Each drawing is exactly `icon-size` high. Waybar scales a higher pixmap down, and the
    /// result is visibly worse.
    #[test]
    fn always_exactly_icon_size_tall() {
        let Some(r) = renderer() else { return };
        for badge in ["", "1", "12", "\u{2298}"] {
            assert_eq!(
                r.render(badge, mark::CLAUDE).height,
                HEIGHT as i32,
                "{badge}"
            );
        }
    }

    /// The quiet state is the mark alone, and that mark is square. It has the same size as each
    /// other tray item, so the applet does not change width when the state changes.
    #[test]
    fn the_calm_icon_is_a_plain_square() {
        let Some(r) = renderer() else { return };
        let calm = r.render("", mark::CLAUDE);
        assert_eq!(calm.width, calm.height);
        assert_eq!(calm.width, HEIGHT as i32);
    }

    /// The free width is the reason that a count fits. A badge of two digits must be wider than
    /// a badge of one digit, and not cut to the same box.
    #[test]
    fn a_longer_badge_gets_a_wider_pixmap() {
        let Some(r) = renderer() else { return };
        let calm = r.render("", mark::CLAUDE).width;
        let one = r.render("1", mark::CLAUDE).width;
        let twelve = r.render("12", mark::CLAUDE).width;
        assert!(one > calm, "{one} vs {calm}");
        assert!(twelve > one, "{twelve} vs {one}");
    }

    #[test]
    fn the_buffer_is_argb32_and_the_right_length() {
        let Some(r) = renderer() else { return };
        let icon = r.render("3", mark::CLAUDE);
        assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        // The renderer drew something, which shows that the font has these glyphs.
        assert!(icon.data.chunks(4).any(|px| px[0] > 0));
    }

    /// The mark keeps its colour for each badge. A red `⊘` means that the producer failed. It
    /// must not make the Claude mark red, or the two failures become one picture.
    #[test]
    fn the_badge_is_coloured_but_the_mark_is_not() {
        let Some(r) = renderer() else { return };
        let icon = r.render("\u{2298}", FAULT);
        // Not `== 255`: `⊘` is a thin ring at h20, and its stroke can stay below full
        // coverage. The question is whether the ink is solid, not whether it is complete.
        let inked = |want: [u8; 3]| {
            icon.data
                .chunks(4)
                .any(|px| px[0] > 128 && [px[1], px[2], px[3]] == want)
        };
        assert!(inked(mark::CLAUDE), "the mark lost its colour");
        assert!(inked(FAULT), "the badge is not the colour it was asked for");
    }

    /// The mark occupies the first square, and the badge occupies the remainder. If the badge
    /// moved back over the mark, the two would overlap at h20 and neither would be legible.
    #[test]
    fn the_badge_never_draws_over_the_mark() {
        let Some(r) = renderer() else { return };
        let bare = r.render("", mark::CLAUDE);
        let badged = r.render("12", BLOCKED);
        let w = badged.width as usize;
        for y in 0..HEIGHT as usize {
            for x in 0..HEIGHT as usize {
                let a = (y * HEIGHT as usize + x) * 4;
                let b = (y * w + x) * 4;
                assert_eq!(bare.data[a..a + 4], badged.data[b..b + 4], "at {x},{y}");
            }
        }
    }
}
