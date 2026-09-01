use crate::mark;
use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont, point};
use ksni::Icon;

pub const HEIGHT: u32 = 20;

pub const BLOCKED: [u8; 3] = [0xE5, 0xC0, 0x7B];

pub const FAULT: [u8; 3] = [0xE0, 0x6C, 0x75];

const FALLBACK_FONTS: &[&str] = &[
    "/run/current-system/sw/share/X11/fonts/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

const FAMILY: &str = "DejaVu Sans";

pub struct Renderer {
    font: Vec<u8>,
    mark: Vec<u8>,
}

impl Renderer {
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

    pub fn render(&self, badge: &str, rgb: [u8; 3]) -> Icon {
        let height = HEIGHT;
        let font = FontRef::try_from_slice(&self.font).expect("validated at load");

        let digit_scale = PxScale::from(height as f32 * 0.68);
        let glyph_scale = PxScale::from(height as f32 * 0.86);
        let scale_for = |c: char| {
            if c.is_ascii_digit() {
                digit_scale
            } else {
                glyph_scale
            }
        };

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
                continue;
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

    #[test]
    fn the_calm_icon_is_a_plain_square() {
        let Some(r) = renderer() else { return };
        let calm = r.render("", mark::CLAUDE);
        assert_eq!(calm.width, calm.height);
        assert_eq!(calm.width, HEIGHT as i32);
    }

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
        assert!(icon.data.chunks(4).any(|px| px[0] > 0));
    }

    #[test]
    fn the_badge_is_coloured_but_the_mark_is_not() {
        let Some(r) = renderer() else { return };
        let icon = r.render("\u{2298}", FAULT);
        let inked = |want: [u8; 3]| {
            icon.data
                .chunks(4)
                .any(|px| px[0] > 128 && [px[1], px[2], px[3]] == want)
        };
        assert!(inked(mark::CLAUDE), "the mark lost its colour");
        assert!(inked(FAULT), "the badge is not the colour it was asked for");
    }

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
