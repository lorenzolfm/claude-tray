//! Drawing `◆ 3` into a tray pixmap.
//!
//! 🔴 **The finding this module is built on: the width budget is free.** Waybar scales a tray
//! pixmap to `icon-size` in *height* and preserves the aspect ratio in *width*
//! (`src/modules/sni/item.cpp`, `Item::updateImage`). A tray icon is a **height budget, not a
//! 20×20 box**, so a glyph *and* a count fit side by side, uncropped and unsquashed. This was
//! observed in Lorenzo's real bar, not reasoned about.
//!
//! Two rules fall out of the same reading, and breaking either one is silent:
//!
//! - 🔴 **Render at the target height, never larger.** An h40 pixmap left for Waybar to
//!   downscale came out visibly blurrier than an h20 one.
//! - 🔴 **The pixmap stays monochrome.** `NeedsAttention` cannot supply its own pixmap —
//!   `AttentionIconPixmap` is an unimplemented TODO — but `Item::setStatus` does add a
//!   `needs-attention` CSS class, so colour belongs in `style.css` where it can follow a theme.

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont, point};
use ksni::Icon;

/// Waybar's configured `icon-size` on this box. Rendering at exactly this height is rule 3.
pub const HEIGHT: u32 = 20;

/// Where to look when `CLAUDE_TRAY_FONT` is unset. The nix package wraps the binary with that
/// variable pointing into the store, so this list only matters for a `cargo run` dev build.
const FALLBACK_FONTS: &[&str] = &[
    "/run/current-system/sw/share/X11/fonts/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

/// The glyph vocabulary the applet needs a font to actually contain: `◇◆◈⊘○·`. DejaVu Sans has
/// all of them, which is why it is named rather than left to fontconfig's default.
const FAMILY: &str = "DejaVu Sans";

pub struct Renderer {
    font: Vec<u8>,
}

impl Renderer {
    /// Find a usable font, in order: the wrapper's variable, fontconfig, then known paths.
    pub fn load() -> Result<Self, String> {
        let mut tried: Vec<String> = Vec::new();

        if let Ok(p) = std::env::var("CLAUDE_TRAY_FONT") {
            match std::fs::read(&p) {
                Ok(font) => return Ok(Self { font }),
                Err(e) => tried.push(format!("CLAUDE_TRAY_FONT={p}: {e}")),
            }
        }

        if let Some(p) = fc_match() {
            match std::fs::read(&p) {
                Ok(font) => return Ok(Self { font }),
                Err(e) => tried.push(format!("fc-match {p}: {e}")),
            }
        }

        for p in FALLBACK_FONTS {
            if let Ok(font) = std::fs::read(p) {
                return Ok(Self { font });
            }
        }

        Err(format!(
            "no font with the glyphs \u{25C7}\u{25C6}\u{25C8}\u{2298}; tried {FAMILY} via \
             fontconfig and {} known paths{}",
            FALLBACK_FONTS.len(),
            if tried.is_empty() {
                String::new()
            } else {
                format!(" ({})", tried.join("; "))
            }
        ))
    }

    /// Rasterise `text` into an ARGB32 pixmap of exactly [`HEIGHT`] rows.
    ///
    /// ⚠️ `ksni::Icon` wants ARGB32 in **network byte order** — the bytes go `A, R, G, B`, not
    /// the little-endian `B, G, R, A` an in-memory `u32` would give you.
    pub fn render(&self, text: &str) -> Icon {
        let height = HEIGHT;
        let font = FontRef::try_from_slice(&self.font).expect("validated at load");

        // The diamonds are small relative to DejaVu's em box, so at 20px the glyph would read
        // as thin next to the digits if both were set at one scale. It gets its own.
        let digit_scale = PxScale::from(height as f32 * 0.68);
        let glyph_scale = PxScale::from(height as f32 * 0.86);
        let scale_for = |c: char| {
            if c.is_ascii_digit() {
                digit_scale
            } else {
                glyph_scale
            }
        };

        let pad = (height as f32 * 0.12).max(1.0);
        let advance: f32 = text
            .chars()
            .map(|c| font.as_scaled(scale_for(c)).h_advance(font.glyph_id(c)))
            .sum();
        let width = (advance + pad * 2.0).ceil().max(height as f32) as u32;

        // Centre the ink box vertically rather than sitting the text on the baseline; the bar
        // gives no vertical slack to be a pixel out in.
        let ref_scaled = font.as_scaled(glyph_scale);
        let text_h = ref_scaled.ascent() - ref_scaled.descent();
        let baseline = (height as f32 - text_h) / 2.0 + ref_scaled.ascent();

        let mut data = vec![0u8; (width * height * 4) as usize];
        let mut caret = pad;
        for c in text.chars() {
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
                let i = ((py as u32 * width + px as u32) * 4) as usize;
                let alpha = (coverage * 255.0) as u8;
                if alpha > data[i] {
                    data[i] = alpha;
                    data[i + 1] = 255;
                    data[i + 2] = 255;
                    data[i + 3] = 255;
                }
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

    /// Rule 3, as an assertion: whatever is drawn, it is exactly `icon-size` tall. Anything
    /// taller gets bilinearly downscaled by Waybar and looks it.
    #[test]
    fn always_exactly_icon_size_tall() {
        let Some(r) = renderer() else { return };
        for text in ["\u{25C7}", "\u{25C6} 1", "\u{25C8} 12", "\u{2298}"] {
            assert_eq!(r.render(text).height, HEIGHT as i32, "{text}");
        }
    }

    /// The width budget being free is the whole reason a count fits. A two-digit badge must
    /// come out wider than a bare glyph, not clipped into the same box.
    #[test]
    fn a_longer_badge_gets_a_wider_pixmap() {
        let Some(r) = renderer() else { return };
        let calm = r.render("\u{25C7}").width;
        let one = r.render("\u{25C6} 1").width;
        let twelve = r.render("\u{25C8} 12").width;
        assert!(one > calm, "{one} vs {calm}");
        assert!(twelve > one, "{twelve} vs {one}");
    }

    #[test]
    fn the_buffer_is_argb32_and_the_right_length() {
        let Some(r) = renderer() else { return };
        let icon = r.render("\u{25C6} 3");
        assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        // Something was actually inked, i.e. the font really had these glyphs.
        assert!(icon.data.chunks(4).any(|px| px[0] > 0));
    }
}
