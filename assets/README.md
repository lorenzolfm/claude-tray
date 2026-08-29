# assets

`claude-mark.svg` is the Claude mark, fetched verbatim from `https://claude.ai/favicon.svg`.

It is kept unmodified on purpose: it is a single closed path in a 248×248 viewBox filled with one
colour (`#D97757`), which is what lets `src/mark.rs` rasterise it at any `icon-size` with a
~90-line scanline filler instead of a vector-graphics dependency or a committed bitmap.

⚠️ If you replace it, keep it a **single** `<path d="…">`. `src/mark.rs` reads the first `d="…"`
in the file and nothing else, and its tests assert the resulting outline's bounding box.
