# assets

`claude-mark.svg` is the Claude mark, fetched verbatim from `https://claude.ai/favicon.svg`.

The file stays unmodified on purpose. It is one closed path in a 248×248 viewBox, filled with
one colour (`#D97757`), which lets `src/mark.rs` rasterise it at each `icon-size` with a scanline
filler of about 90 lines instead of a vector-graphics dependency or a committed bitmap.

If you replace it, keep one `<path d="…">` in it. `src/mark.rs` reads the first `d="…"` in the
file and nothing else, and its tests check the bounding box of the resulting outline.
