`glyph-gap.ttf` is a synthetic 544-byte test font (no third-party font data).
Reproduce with `python3 crates/engine/tests/fixtures/make_glyph_gap.py` (stdlib only).
Adapted from tasks/review-r3/scratch/make_font.py: A has a rectangular glyf outline;
B is mapped but has only an sbix PNG bitmap. The native outliner returns A for AB.
