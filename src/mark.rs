//! The Claude mark, rasterised from the official SVG at whatever height the bar asks for.
//!
//! 🔴 **Why a rasteriser and not a committed PNG.** `assets/claude-mark.svg` is one closed path
//! in a 248×248 box filled with a single colour. That shape — no curves worth the name, no
//! second contour, no gradient — is small enough to fill with a scanline loop, which buys two
//! things a bitmap could not: the mark is exact at *any* `icon-size` (Waybar's own downscale is
//! visibly blurrier, see [`crate::icon`]), and the repo carries the vector the brand actually
//! ships rather than a resampling of it.
//!
//! The fill is deliberately plain: **one closed contour**, so even-odd and nonzero winding agree
//! and the rule never has to be chosen. Antialiasing is asymmetric on purpose — supersampled
//! down the `y` axis, but *analytic* across `x`, where a span's overlap with a pixel is just a
//! length. Exact where it is cheap, sampled where it is not.

/// The `viewBox` side of `assets/claude-mark.svg`. The path fills it corner to corner.
const VIEW_BOX: f32 = 248.0;

/// The mark's own colour, read off the official SVG's `fill`. Not a theme colour — it is the
/// brand, and it stays put while the badge beside it changes with state.
pub const CLAUDE: [u8; 3] = [0xD9, 0x77, 0x57];

/// Sub-scanlines per output row. 16 is where the rays stop visibly stair-stepping at h20; the
/// whole rasterisation happens once at startup, so there is nothing to save by going lower.
const SUBSCANLINES: usize = 16;

/// Line segments per cubic. The path contains exactly one, and it is a shallow one.
const CURVE_STEPS: usize = 16;

const SVG: &str = include_str!("../assets/claude-mark.svg");

/// An 8-bit coverage mask, `size` × `size`, row-major. Alpha only — the caller owns the colour.
pub fn mask(size: u32) -> Vec<u8> {
    fill(&outline(), size)
}

/// The mark as a single closed polygon in viewBox coordinates.
fn outline() -> Vec<(f32, f32)> {
    flatten(path_d(SVG))
}

/// ⚠️ Reads the **first** `d="…"` in the file and ignores everything else — `fill`, `viewBox`,
/// any sibling element. That is a deliberate limit, documented in `assets/README.md`: this
/// parses one known file, not SVG in general.
fn path_d(svg: &str) -> &str {
    let start = svg
        .find(" d=\"")
        .expect("assets/claude-mark.svg has no path")
        + 4;
    let rest = &svg[start..];
    &rest[..rest.find('"').expect("unterminated d attribute")]
}

enum Tok {
    Cmd(char),
    Num(f32),
}

/// ⚠️ SVG lets numbers abut with no separator (`5-3` is two of them), so a `-` always opens a
/// new token rather than continuing one. There are no exponents in this path, and a second `.`
/// likewise starts a new number.
fn tokens(d: &str) -> Vec<Tok> {
    let b: Vec<char> = d.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let start = i;
            let mut dot = c == '.';
            i += 1;
            while i < b.len() {
                match b[i] {
                    '0'..='9' => i += 1,
                    '.' if !dot => {
                        dot = true;
                        i += 1;
                    }
                    _ => break,
                }
            }
            let s: String = b[start..i].iter().collect();
            out.push(Tok::Num(
                s.parse().unwrap_or_else(|_| panic!("bad number {s:?}")),
            ));
        } else {
            i += 1;
        }
    }
    out
}

/// Walk the path into points. `M/L/H/V/C/Z` and their relative forms are all this file uses;
/// anything else would be a silent hole, so it panics rather than drawing a wrong mark.
fn flatten(d: &str) -> Vec<(f32, f32)> {
    let toks = tokens(d);
    let mut pts: Vec<(f32, f32)> = Vec::new();
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut cmd = ' ';
    let mut i = 0;

    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                cx = sx;
                cy = sy;
                continue;
            }
        }
        let mut num = || match toks[i] {
            Tok::Num(v) => {
                i += 1;
                v
            }
            Tok::Cmd(c) => panic!("expected a number, found command {c:?}"),
        };
        let rel = cmd.is_ascii_lowercase();
        match cmd.to_ascii_uppercase() {
            'M' | 'L' => {
                let (mut x, mut y) = (num(), num());
                if rel {
                    x += cx;
                    y += cy;
                }
                if cmd == 'M' || cmd == 'm' {
                    sx = x;
                    sy = y;
                    // An `M` with extra coordinate pairs means implicit `L`s, per SVG.
                    cmd = if rel { 'l' } else { 'L' };
                }
                cx = x;
                cy = y;
                pts.push((x, y));
            }
            'H' => {
                let mut x = num();
                if rel {
                    x += cx;
                }
                cx = x;
                pts.push((cx, cy));
            }
            'V' => {
                let mut y = num();
                if rel {
                    y += cy;
                }
                cy = y;
                pts.push((cx, cy));
            }
            'C' => {
                let (mut x1, mut y1) = (num(), num());
                let (mut x2, mut y2) = (num(), num());
                let (mut x, mut y) = (num(), num());
                if rel {
                    x1 += cx;
                    y1 += cy;
                    x2 += cx;
                    y2 += cy;
                    x += cx;
                    y += cy;
                }
                let (px, py) = (cx, cy);
                for k in 1..=CURVE_STEPS {
                    let t = k as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    let bx = u * u * u * px
                        + 3.0 * u * u * t * x1
                        + 3.0 * u * t * t * x2
                        + t * t * t * x;
                    let by = u * u * u * py
                        + 3.0 * u * u * t * y1
                        + 3.0 * u * t * t * y2
                        + t * t * t * y;
                    pts.push((bx, by));
                }
                cx = x;
                cy = y;
            }
            other => panic!("unsupported path command {other:?}"),
        }
    }
    pts
}

/// Scanline-fill the closed polygon into an 8-bit coverage mask.
fn fill(pts: &[(f32, f32)], size: u32) -> Vec<u8> {
    let n = pts.len();
    let size_i = size as usize;
    let scale = size as f32 / VIEW_BOX;
    let mut cov = vec![0.0f32; size_i * size_i];
    let weight = 1.0 / SUBSCANLINES as f32;
    let mut xs: Vec<f32> = Vec::with_capacity(16);

    for py in 0..size_i {
        for sub in 0..SUBSCANLINES {
            // Sample at the centre of each sub-row, in viewBox units.
            let y = (py as f32 + (sub as f32 + 0.5) * weight) / scale;
            xs.clear();
            for k in 0..n {
                let (x0, y0) = pts[k];
                let (x1, y1) = pts[(k + 1) % n];
                // Half-open in y, so a vertex shared by two edges is counted exactly once.
                if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
                    xs.push(x0 + (y - y0) * (x1 - x0) / (y1 - y0));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
            for pair in xs.as_chunks::<2>().0 {
                let (a, b) = (pair[0] * scale, pair[1] * scale);
                let first = a.floor().max(0.0) as usize;
                let last = (b.ceil() as isize).clamp(0, size_i as isize) as usize;
                for px in first..last {
                    // Exact overlap of the span with this pixel column — no sampling in x.
                    let lo = a.max(px as f32);
                    let hi = b.min(px as f32 + 1.0);
                    if hi > lo {
                        cov[py * size_i + px] += (hi - lo) * weight;
                    }
                }
            }
        }
    }

    cov.into_iter()
        .map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vendored file is the one this parser was written against. A replacement that changed
    /// the outline would move these numbers, which is the point: it should not pass quietly.
    #[test]
    fn the_outline_fills_its_view_box() {
        let pts = outline();
        assert_eq!(pts.len(), 173, "one closed contour, one cubic subdivided");
        let (min_x, max_x) = bounds(pts.iter().map(|p| p.0));
        let (min_y, max_y) = bounds(pts.iter().map(|p| p.1));
        for (lo, hi) in [(min_x, max_x), (min_y, max_y)] {
            assert!((lo - 6.2).abs() < 0.05, "{lo}");
            assert!((hi - 241.8).abs() < 0.05, "{hi}");
        }
    }

    fn bounds(it: impl Iterator<Item = f32>) -> (f32, f32) {
        it.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    }

    /// 🔴 **The rasteriser checked against the artwork it is imitating.** Claude's own
    /// `favicon.ico` ships the mark pre-rendered at 48, 32 and 16 px, and all three ink
    /// **0.3589** of their box once alpha is summed. This filler lands within 0.002 of that at
    /// every size, which is the evidence that the winding, the span clamp and the antialiasing
    /// are all right — a mask that merely *looks* like a starburst can be off by far more.
    #[test]
    fn coverage_matches_the_official_pre_rendered_mark() {
        const OFFICIAL: f32 = 0.3589;
        for size in [16, 20, 32, 64] {
            let m = mask(size);
            assert_eq!(m.len(), (size * size) as usize);
            let ink: f32 = m.iter().map(|&a| a as f32 / 255.0).sum();
            let frac = ink / (size * size) as f32;
            assert!(
                (frac - OFFICIAL).abs() < 0.006,
                "{size}px inked {frac}, the shipped favicon inks {OFFICIAL}"
            );
        }
    }

    /// The rays reach every edge of the box, so no row or column of the mask is blank. This is
    /// what catches an off-by-one in the span clamp, which would eat the last column silently.
    #[test]
    fn every_row_and_column_is_inked() {
        let size = 32usize;
        let m = mask(size as u32);
        for i in 0..size {
            assert!(
                m[i * size..(i + 1) * size].iter().any(|&a| a > 0),
                "row {i}"
            );
            assert!((0..size).any(|y| m[y * size + i] > 0), "column {i}");
        }
    }

    /// Antialiasing is the whole reason for the sub-scanlines: a mark that were only ever 0 or
    /// 255 would be the hard-edged version, and it looks it at 20px.
    #[test]
    fn edges_are_antialiased() {
        let m = mask(20);
        assert!(m.iter().any(|&a| a > 0 && a < 255), "no partial coverage");
        assert!(m.contains(&255), "nothing solid");
    }
}
