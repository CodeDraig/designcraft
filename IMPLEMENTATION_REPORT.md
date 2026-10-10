# Publication implementation report

Implemented the eight stages in [the approved plan](IMPLEMENTATION_PLAN.md).
The implementation keeps the Rust command boundary and the self-contained archive
selected during planning. Verification was completed before the delivery commit;
no installation or deployment was performed. The initial working tree and original TEI work were preserved;
its recovery copy remains in `target/publication-backups/20261010T1634Z/`.

## Delivered behavior

1. **Publication archive and navigation:** canonical chapter content and assets,
   independent edition layouts, hierarchy, Book conversion, import, transactional
   editing, save/reopen, recovery and native/browser byte delivery. Publications use
   native format version 3; older supported standalone archives still open.
2. **Metadata, styles and numbering:** shared metadata and reusable values, stable
   semantic identities, named numbering sequences, and explicit style previews that
   retain local overrides until the user chooses replacements.
3. **References and generated content:** semantic queries, rich templates, contents
   and other generated sections, running heads, current page values and bounded
   live/deferred fixed-point updates.
4. **Structured indexes:** independent index definitions, hierarchical topics,
   anchored occurrences and ranges, cross-references, custom sorting and generated
   destinations that follow edits and pagination.
5. **Notes and apparatus:** independent streams and numbering, per-edition placement,
   rich editable notes, run-in/multicolumn/continued note areas, lemma/witness choices,
   and preserved critical readings with explicit diagnostics for unsupported forms.
6. **Editions and output:** per-edition policies and output metadata, whole-publication
   PDF, reflowable/fixed EPUB and HTML, stable internal destinations and note backlinks.
7. **Review:** publication-wide tracked authors and accept/reject, anchored comments,
   immutable named proofs with separate approvals, semantic/layout comparisons,
   destination navigation and reviewed revised-manuscript import. Uncertain matches
   require explicit decisions, and stale previews fail without changing the document.
8. **Incremental layout:** changed paragraph ranges, resumable numbering/keep/cursor
   checkpoints, verified output/state convergence, reference-input memoization,
   bounded runtime caches, metrics and a full-composition oracle.

All operations use engine commands. The shared Rust UI presents publication,
reference, index, note, edition, review and layout dialogs through those commands.
Command parameters and behavior are documented in [publications.md](docs/publications.md).

## Verification

`cargo xtask ci` passed for each stage: formatting, Clippy with warnings denied,
workspace tests, asset attribution, dependency layering and all configured WASM
crate checks. Final focused composition and engine fixtures cover the new behavior.

| Stage | Tests passed | Evidence |
| --- | ---: | --- |
| 1 | 852 | [CI log](target/publication-acceptance/20261010T1702Z/ci-final.log) |
| 2 | 862 | [CI log](target/publication-r2-verified-ci.log) |
| 3 | 874 | [CI log](target/publication-r3-verified-ci.log) |
| 4 | 888 | [CI log](target/publication-r4-verified-ci.log) |
| 5 | 900 | [CI log](target/publication-r5-ci-4.log) |
| 6 | 904 | [CI log](target/publication-r6-ci-5.log) |
| 7 | 918 | [CI log](target/publication-r7-ci-6.log) |
| 8 | 927 | [CI log](target/publication-r8-ci-5.log) |

The final native CLI and real CLI MCP process exercised command discovery,
archive opening, editing, independent layout verification, undo/redo and named-byte
opening. UI control requests exercised the same commands and dialog confirmation;
whole-window offscreen screenshots were opened and inspected. Exported HTML was
also inspected in the browser during stage 6, including note links and backlinks.

The supplied British Kymry XML was tested again through native import and the full
layout oracle (112 frames, with its local illustrations). Its original 619,411 XML
bytes were retrieved unchanged. Earlier byte-only export acceptance had 109 pages
and two explicitly diagnosed image placeholders because it supplied no resource map;
those are different import conditions. Original source SHA-256:
`43f1f3aec19d95af43c2fbac1193d3eb03d95fbf1a64f8f67dba3aab26e03685`.
The source has no encoded notes/apparatus; authored TEI fixtures exercise rich notes,
critical readings, tables and illustrations.

## Book-scale evidence

The reproducible Rust generator starts with 12 chapters, 900 pages and 3,600
references. Each section starts a frame, while body text flows normally and can
cross pages. Separate regression fixtures exercise unrestricted paragraph flow.
Each stage below matched full composition and an independent reference fixed point.

| Operation | Pages | Paragraphs recomposed | Frames recomposed | Command wall time |
| --- | ---: | ---: | ---: | ---: |
| cold | 900 | 1800 | 1800 | 2.533 s |
| local-edit | 900 | 3 | 3 | 0.935 s |
| chapter-insertion | 930 | 930 | 930 | 3.348 s |
| pagination-cascade | 933 | 943 | 948 | 2.138 s |
| global-style | 933 | 930 | 933 | 3.638 s |

Cold layout includes the initial unresolved-field pass and its resolved-field pass.
Work counters describe composition, not all command CPU work. Timings are measurements
on this workstation, not an isolated or portable performance guarantee. The timed
acceptance process peaked at **1035.7 MiB RSS**, including independent full
verification and export; cache estimates are a different metric.

The final [933-page PDF](target/publication-acceptance/20261010T2225Z/atlas.pdf)
contains **4,500 valid internal link annotations**. First, middle
and final pages were rendered and inspected, with no silent overset reported by
composition. [The release-eight receipt](target/publication-acceptance/20261010T2225Z/release-8-receipt.json)
links the benchmark, MCP, CLI, UI and PDF evidence.

## Changed areas and reproducibility

- `crates/doc/src/publication/` and `crates/format/src/publication/`: canonical model,
  identities, edition projections, validation, native archives and compatibility.
- `crates/engine/src/cmd/publication/`, editing/import/export commands and recovery:
  shared transactional behavior and automation interfaces.
- `crates/compose/src/`: generated text, note areas, marker shaping, dependency-aware
  caching, checkpoints and exact geometry comparison.
- `crates/pdf/`, `crates/epub/`: whole-publication output and stable destinations.
- `crates/ui-egui/`, desktop services and `apps/designcraft-web/`: dialogs, panels,
  original file-picker intent, byte delivery and browser recovery.
- Tests, authored TEI/scale fixtures, `ASSETS.md`, `README.md`, control/MCP and
  publication documentation. [The file manifest](target/publication-implementation-files.json)
  records changed paths and hashes against the preserved pre-implementation state.

Principal commands run:

```sh
cargo test -p designcraft-compose incremental -- --nocapture
cargo test -p designcraft-engine --test publication_incremental -- --nocapture
cargo xtask ci
cargo build -p designcraft-engine --example publication_scale
cargo build -p designcraft-cli
cargo build -p designcraft-ui-egui --example ui_shot
/usr/bin/time -v target/debug/examples/publication_scale NEW_OUTPUT_DIRECTORY
target/debug/designcraft-cli run --in BOOK.designcraft --cmd 'publication.layout.verify={}'
target/debug/examples/ui_shot SCRIPT.jsonl
```

PDF assertions and rendering used PyMuPDF; whole-window screenshots used the existing
wgpu harness. Acceptance scripts, logs and native/PDF outputs remain locally under
the ignored `target/` directory; they are not included in the source commit.
Tests for failure preservation, stale decisions and unsupported input intentionally
exercise errors. Development compile/test issues were corrected before final CI;
stage-specific receipts retain their details. In stage 8, these included a CrossRef
Arc dereference compile error, a WASM unused-mut warning, a CLI harness incorrectly
expecting pretty JSON, and the reopened-reference counter found by UI acceptance.
No final CI or acceptance failure remains.

## Explicit limits and next sharp edges

- Notes, tables, anchored objects, legacy variables/references and source stories
  over 8 MiB use the full composer with a named fallback. Resumable complex
  continuations are the next layout optimization; correctness uses the tested full path.
- The application editor's browser graphics could not be validated because WebGL2
  was unavailable in the tested browsers. WASM compilation, asynchronous byte routing,
  and the browser IndexedDB recovery probe passed. Offscreen UI and rendered HTML
  evidence do not claim browser-editor visual acceptance.
- PDF note/table text is searchable, but semantic PDF structure tagging remains
  limited and produces diagnostics. Strict export rejects such diagnostics.
- Complex block apparatus stays preserved with explicit diagnostics. Proofs retain
  content, edition/layout and font fingerprints, not a frozen executable or every font.
- Revised-manuscript comparison uses persistent source identities and explicit
  reviewed fallbacks; it does not silently treat uncertain text matches as identities.

The approved defaults remain reading-edition text, preserved source XML, reflowed
pages with stable source markers, and one self-contained publication archive.
