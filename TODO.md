# Milestone to-do list

Extracted on 2026-10-09 from the [milestone implementation inventory](ROADMAP.md#milestone-implementation-inventory-2026-10-09).
Unchecked items include both missing implementation and outstanding verification; they are
grouped by milestone, not ranked by priority.

- [ ] **M1 — UI:** Complete the remaining UI depth and visual-fidelity work.
- [ ] **M2 — CJK:** Complete the remaining [CJK typesetting phases](docs/cjk.md#phases).
- [ ] **M2 — Arabic:** Address the documented [Arabic typography limitations](docs/arabic-typography.md#remaining-limitations).
- [ ] **M4 — PDF:** Implement overprint export.
- [ ] **M4 — Preview:** Implement spot-colour separation plate preview.
- [ ] **M5 — EPS:** Implement PostScript interpretation beyond TIFF-preview/placeholder placement.
- [ ] **M6 — PDF/X:** Validate PDF/X-4 output against a certified checker.
- [ ] **M6 — Interchange:** Address remaining IDML/PDF interchange depth and hardening work.
- [ ] **M8 — Tables:** Compose rotated cell text.
- [ ] **M8 — Tables:** Implement diagonal cell strokes.
- [ ] **M9 — Composition:** Implement finer-grained incremental composition beyond the existing caches and parallel composition.
- [ ] **M9 — Rendering:** Implement GPU rasterization.
- [ ] **M10 — Language:** Bundle non-English hyphenation dictionaries.
- [ ] **M12 — Media:** Implement in-app audio/video playback.
- [ ] **M14 — Release:** Complete and verify 1.0 acceptance criteria.
- [ ] **M14 — UI:** Complete and verify remaining visual polish.

M7's native-filesystem requirement for books is recorded as a platform limitation, not an
explicit unfinished milestone task; this extraction does not introduce a web-books project.
M0, M3, M11 and M13 have no unfinished items identified in the source inventory.

The original local-only `plan/` acceptance criteria were unavailable during the source review.
This list does not establish their contents or claim that verification has been performed.
