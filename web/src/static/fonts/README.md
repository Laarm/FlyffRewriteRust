# Fallback font (Plan/40 section 2.1, 42 section 8.1)

`DejaVuSans-subset.woff2` is DejaVu Sans (Bitstream Vera licence, see `LICENSE-DejaVu.txt`), subset to
Latin, Latin-1, Latin Extended-A, Greek, general punctuation and a few symbols (about 33 KB). It is the
fallback for the original Verdana metrics (D7); the CSS family name is `"Flyff DejaVu Sans"`
(`src/styles/fonts.css`). Imported through CSS so Vite emits it as a content-hashed file under
`dist/assets/` (never `dist/fonts/`).

Regenerate from `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf` with the `subset-font` npm package
(`targetFormat: 'woff2'`) when more glyph ranges are needed.
