# pdf-inspector

Fast PDF text extraction to structured Markdown. CLI binary: `pdf2md`. Detection binary: `detect-pdf`.

## Build & Test

```bash
cargo fmt                                    # format
cargo clippy -- -D warnings                  # lint (enforced, zero warnings)
cargo test                                   # unit + integration tests (267+ unit, 73+ integration)
cargo build --release                        # release binary for benchmarks
```

All three must pass before committing.

## Binaries

- `pdf2md` — extract PDF → Markdown. Supports `--json` for structured output.
- `detect-pdf` — classify PDF type (TextBased/Scanned/Mixed/ImageBased). Supports `--analyze --json`.

## Architecture

```
src/
  lib.rs                        – public API, process_pdf_with_options, encoding issue detection
  detector.rs                   – PDF type classification, tiled-scan detection, page sampling
  types.rs                      – TextItem, TextLine, PdfRect, PdfLine
  tounicode.rs                  – CMap/ToUnicode parsing, CID decoding
  text_utils.rs                 – CJK/RTL handling, Otsu threshold, ligature expansion, NFKC
  extractor/
    mod.rs                      – top-level extraction orchestrator
    content_stream.rs           – PDF operator state machine (Tj/TJ/Td/Tm/q/Q)
    geometry.rs                 – run boxes and baseline rotation shared by both content parsers
    fonts.rs                    – font width/encoding, CMapDecisionCache, TrueType cmap fallback
    layout.rs                   – column detection (histogram), newspaper/tabular classification,
                                  spanning-line pre-masking, sidebar detection
  tables/
    detect_rects.rs             – rect-based table detection (union-find clustering)
    detect_heuristic.rs         – heuristic table detection (gap-histogram, body-font tables)
    detect_lines.rs             – line-based table detection (H/V line grids)
    grid.rs                     – column/row boundaries, cell assignment
    format.rs                   – table→Markdown formatting, continuation row merging
  markdown/
    convert.rs                  – core line→Markdown loop, struct-tree role support
    analysis.rs                 – font stats, heading tiers, paragraph thresholds
    classify.rs                 – line classification (header, list, code, caption)
    preprocess.rs               – drop cap merging, heading line merging
    postprocess.rs              – dot leaders, hyphenation, page numbers, URL formatting
```

## Key design decisions

- **Primary audience is AI agents.** Output optimized for token efficiency and semantic quality, not visual formatting. No cosmetic padding.
- **Three table detection strategies** run in priority order: rect-based → line-based → heuristic. First valid result wins.
- **Column detection** uses horizontal projection histograms with valley detection. Multi-item spanning lines (titles, headers) are pre-masked using column-aware thresholds before column assignment.
- **Newspaper vs tabular** classification determines reading order: newspaper reads columns sequentially, tabular Y-interleaves them.
- **Tiled-scan detection** catches scanned PDFs with JBIG2/strip images where no single tile exceeds the template threshold but aggregate area does (≥2M pixels).
- **Garbage text upgrade** reclassifies Mixed PDFs as Scanned when extracted text is <50% alphanumeric.
- **Tagged PDF support** uses structure tree roles (H1-H6, P, L, Code, BlockQuote) when available, falling back to font-size heuristics.

## Testing

- **Unit tests**: inline `#[cfg(test)] mod tests` in each module with synthetic data.
- **Integration tests**: `tests/integration_tests.rs` with fixture PDFs in `tests/fixtures/`.
- **Regression suite**: sibling repo `pdf-evals` with ~200 snapshot PDFs. Run `cargo build --release` then `bench.py test` in that repo before committing. While iterating, prefer a subset run (`bench.py test -q` for the quick set, or `-s <name>` for a named test set) and save the full `bench.py test` for the final pre-commit check.
- **Semantic quality**: run `bench.py score` in `pdf-evals` for the semantic verdict (TEDS + MHS + reading order + char/word + list preservation, composited). Character-level diff alone misclassifies structural improvements (e.g., column-detection rewrites) as regressions — `score` is the tie-breaker. See `pdf-evals/CLAUDE.md` "Semantic scoring".

## Debugging

```bash
RUST_LOG=pdf_inspector::extractor::layout=debug cargo run --bin pdf2md -- file.pdf
RUST_LOG=pdf_inspector::tables=debug cargo run --bin pdf2md -- file.pdf
RUST_LOG=pdf_inspector::detector=debug cargo run --release --bin detect-pdf -- file.pdf
```

## Conventions

- Clippy: use `is_some_and(...)` not `map_or(false, ...)`
- lopdf quirk: `ParseError` is private — match by string for `InvalidFileHeader`
- Column limit for tables: 40 (`MAX_TABLE_COLUMNS` in `tables/mod.rs`, single-sourced across all three detectors)
- `propagate_merged_cells` gates on `decorative_fill_rects`, not on column count. A multi-row rect covering a strict majority of columns is decoration whenever it spans at most half the rows (the old row-count rule). A rect that does NOT clear that row-count rule — including a NARROW band, fewer than three columns or at most half the columns — is instead evaluated by the content+subdivision test: both (a) its covered cells already hold their own per-cell text — a merged cell holds one run of content, a band sits over cells that each hold their own — and (b) its area is subdivided by two or more vertically disjoint sub-rects, because a band is painted BEHIND per-cell rects while a merged cell has none inside it, must hold (narrow bands additionally require `rows_spanned >= 4` in a table with `num_cols > 10` before this test is trusted at all). Row count alone is a bad proxy: a band can legitimately cover most of a table, and narrow bands/column stripes are decoration too. Content alone is not enough either — side-by-side merged cells whose values wrap read exactly like a band by content, and not at all like one by geometry; the content test itself is a minority/majority read on how many covered columns show that wrapped-value pattern, not a strict zero-tolerance check, so a single stray populated cell doesn't defeat it. **An EXACT tie on that column-population ratio (`self_populated * 2 == cols.len()`) is not resolvable by the ratio itself** — round 6 found a real fixture (`test_snapshot_2013_app2`, 2-of-4 columns) and three synthetic regressions (4-of-8, 4-of-8, 1-of-2 columns) that are numerically identical on the ratio but semantically opposite. The tie is broken on absolute band height instead (`LARGE_BAND_ROW_COUNT` in `src/tables/detect_rects.rs`): every measured decorative band is at most ~8 rows, the real fixture is 45-48, so a tied band under that threshold reads as decoration and one at or above it reads as a genuine merge. This is a known, narrower-than-ideal boundary, not a closed problem — a short band that is ALSO exactly tied on column population has no fixture yet and would still misclassify; see the comment at the tie branch in `decorative_fill_rects` for what a further fix would need to measure.
- **Ordering invariant in `try_build_grid`, easy to break by moving one line.** Both decoration predicates read `cells` as ASSIGNED, so both must run BEFORE the fold; `record_merge_coverage` is a readout OF the fold, so it must run AFTER it and take `coverage_excluded`, which is built from `propagate_merged_cells`' returned `Vec<bool>` fold-mask (not from `evidence_excluded`, and not from a second judgement of its own). Occupancy and cell text are two views of one decision and may not disagree — computing them from different predicates is what produced a cell reading `"r0c0 r1c0"` (folded) alongside `is_own = true` (not folded). Single-row rects drive no fold, so they keep the `evidence_excluded` colspan discriminator. `non_merge_evidence_rects` calls `decorative_fill_rects` internally with the same arguments, so the two pre-fold predicates cannot reach different answers.
- Neither decoration predicate is independent of the earlier `w > 10 * median_width` size filter: a full-width band is dropped there and never reaches them. A fixture meant to exercise decoration logic must use a band narrower than that threshold, or it tests nothing.
- **Known defect, not fixed:** the contained-sub-rect dedup in `detect_rects` (`bh < ah * 4.0`) strips the per-cell rects inside any shading band under 4 cell-heights tall, so those bands' rows collapse before any decoration predicate can see them. Reproduction: the ignored `test_short_bands_keep_their_per_cell_rects`. Two narrowings were implemented and measured against the fixture corpus, and both changed real output; the comment at the dedup step records which and how. This defect also caps the decoration predicate above: its subdivision test reads the very sub-rects the dedup removes, so a band under 4 cell-heights covering more than half the rows still folds (`test_short_band_covering_most_rows_still_folds`). Fixing the dedup widens the predicate for free.
