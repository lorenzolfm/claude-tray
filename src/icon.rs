//! Drawing the Claude mark, and a badge beside it, into a tray pixmap.
//!
//! 🔴 **The finding this module is built on: the width budget is free.** Waybar scales a tray
//! pixmap to `icon-size` in *height* and preserves the aspect ratio in *width*
//! (`src/modules/sni/item.cpp`, `Item::updateImage`). A tray icon is a **height budget, not a
//! 20×20 box**, so the mark *and* a count fit side by side, uncropped and unsquashed. This was
//! observed in Lorenzo's real bar, not reasoned about.
//!
//! 🔴 **Render at the target height, never larger.** An h40 pixmap left for Waybar to downscale
//! came out visibly blurrier than an h20 one — which is why [`crate::mark`] rasterises the SVG
//! at exactly [`HEIGHT`] instead of shipping a bitmap for Waybar to resample.
//!
//! 🔴 **The pixmap used to be monochrome so that colour could live in `style.css`. That reason
//! is gone.** [`ksni::Status::NeedsAttention`] does add a `needs-attention` CSS class, but a
//! tray item is a `Gtk::Image`: `color` does nothing to it, and the only cue CSS can actually
//! give is a border. So colour has to be *in the pixels* or nowhere, and the three it carries
//! each mean exactly one thing — the mark is always [`mark::CLAUDE`], and the badge is [`COUNT`]
//! or [`BLOCKED`] or [`FAULT`]. See `README.md`.

use crate::mark;
use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont, point};
use ksni::Icon;

/// Waybar's configured `icon-size` on this box. Rendering at exactly this height is the rule
/// above; the mark is rasterised to it, so changing it stays crisp rather than resampled.
pub const HEIGHT: u32 = 20;

/// The count, when it is merely a count. 🔴 **Not the mark's terracotta** — that was tried and
/// it is the dimmest thing on the bar, which is backwards: the mark is the part you already know
/// and the number is the part you have to read. This is `#fdf6e3` from his own `style.css`, the
/// colour every other figure in the bar is already drawn in.
pub const COUNT: [u8; 3] = [0xFD, 0xF6, 0xE3];

/// Something is *blocked*, not merely finished — the distinction the old `◈`-over-`◆` glyph
/// pair carried, now carried by the colour of the count. Same amber as the `needs-attention`
/// border in `style.css`, because it means the same thing: nothing is moving, look now.
pub const BLOCKED: [u8; 3] = [0xE5, 0xC0, 0x7B];

/// The producer is missing or failing. Not "you have work" — *the applet cannot see*, which is
/// the one failure that must never be mistakable for calm.
pub const FAULT: [u8; 3] = [0xE0, 0x6C, 0x75];

/// Where to look when `CLAUDE_TRAY_FONT` is unset. The nix package wraps the binary with that
/// variable pointing into the store, so this list only matters for a `cargo run` dev build.
const FALLBACK_FONTS: &[&str] = &[
    "/run/current-system/sw/share/X11/fonts/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

/// The badge vocabulary a font has to actually contain: the digits, and `⊘`. DejaVu Sans has
/// them, which is why it is named rather than left to fontconfig's default.
const FAMILY: &str = "DejaVu Sans";

pub struct Renderer {
    font: Vec<u8>,
    /// The mark, rasterised once. It never changes shape or size, so it never needs redoing.
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
    /// [`HEIGHT`] rows. An empty `badge` gives a bare square icon — the calm state is the mark
    /// and nothing else, the same width as every other item in the tray.
    ///
    /// ⚠️ `ksni::Icon` wants ARGB32 in **network byte order** — the bytes go `A, R, G, B`, not
    /// the little-endian `B, G, R, A` an in-memory `u32` would give you. ⚠️ And *straight*
    /// alpha, not premultiplied: Waybar hands the buffer to a `GdkPixbuf`, which is
    /// non-premultiplied.
    pub fn render(&self, badge: &str, rgb: [u8; 3]) -> Icon {
        let height = HEIGHT;
        let font = FontRef::try_from_slice(&self.font).expect("validated at load");

        // `⊘` is small relative to DejaVu's em box and would read as thin next to the digits if
        // both were set at one scale. It gets its own.
        let digit_scale = PxScale::from(height as f32 * 0.68);
        let glyph_scale = PxScale::from(height as f32 * 0.86);
        let scale_for = |c: char| {
            if c.is_ascii_digit() {
                digit_scale
            } else {
                glyph_scale
            }
        };

        // The mark fills its square, like any other tray icon. The gap is what stops the mark
        // and the count reading as one token; the trailing pad keeps the digit off the edge.
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

        // Centre the ink box vertically rather than sitting the text on the baseline; the bar
        // gives no vertical slack to be a pixel out in.
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

    /// Rule 3, as an assertion: whatever is drawn, it is exactly `icon-size` tall. Anything
    /// taller gets bilinearly downscaled by Waybar and looks it.
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

    /// The calm state is the bare mark, and a bare mark is square — the same footprint as every
    /// other tray item, which is what stops the applet twitching wider the moment anything
    /// happens and back again when it settles.
    #[test]
    fn the_calm_icon_is_a_plain_square() {
        let Some(r) = renderer() else { return };
        let calm = r.render("", mark::CLAUDE);
        assert_eq!(calm.width, calm.height);
        assert_eq!(calm.width, HEIGHT as i32);
    }

    /// The width budget being free is the whole reason a count fits. A two-digit badge must
    /// come out wider than a one-digit one, not clipped into the same box.
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
        // Something was actually inked, i.e. the font really had these glyphs.
        assert!(icon.data.chunks(4).any(|px| px[0] > 0));
    }

    /// 🔴 The mark keeps its own colour whatever the badge is doing. A red `⊘` means the
    /// producer is broken; it must not repaint the Claude mark red as well, or "I cannot see"
    /// and "Claude is on fire" become the same picture.
    #[test]
    fn the_badge_is_coloured_but_the_mark_is_not() {
        let Some(r) = renderer() else { return };
        let icon = r.render("\u{2298}", FAULT);
        // ⚠️ Not `== 255`: `⊘` is a thin ring at h20 and its stroke may never reach full
        // coverage. Solidly inked is the question, not perfectly inked.
        let inked = |want: [u8; 3]| {
            icon.data
                .chunks(4)
                .any(|px| px[0] > 128 && [px[1], px[2], px[3]] == want)
        };
        assert!(inked(mark::CLAUDE), "the mark lost its colour");
        assert!(inked(FAULT), "the badge is not the colour it was asked for");
    }

    /// The mark occupies the leading square and the badge everything after it. If the badge
    /// ever bled backwards over the mark the two would overlap at h20 and both become mush.
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
