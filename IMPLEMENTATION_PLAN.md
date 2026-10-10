# Long-document publishing: publication and edition model

Approved by the user on 2026-10-10. This plan covers eight staged releases using a
self-contained publication archive. Each release includes usable engine commands,
the corresponding UI, persistence and verification.

The central design is one publication containing canonical text, structure,
annotations and references, with separate edition layouts. Text edits update the
shared content; page geometry, typography and note placement belong to the selected
edition. Original TEI sources remain immutable attachments.

The current code provides useful foundations, but its
[Book implementation](crates/engine/src/cmd/book.rs) coordinates external files,
its [document model](crates/doc/src/lib.rs) holds one generated index and TOC, and
its [composition cache](crates/compose/src/cache.rs) works at story granularity.

## 1. A publication that can be opened, edited and saved

Add a publication model in [the document crate](crates/doc/src), containing an
ordered front-matter/chapter/back-matter tree, shared content and assets,
persistent entity identities, and edition records. Separate shared story content
from edition-specific frame bindings, reusing the existing rich-text, table and
anchored-object representations.

Extend [session and history handling](crates/engine/src/document_state.rs) so
publication edits, selection and undo belong to one coherent snapshot. Existing
editing commands must address the canonical content through the active edition.

Extend [native persistence](crates/format/src/lib.rs) with version 3 publication
archives. Continue reading versions 1 and 2. Provide explicit conversion from
existing documents and `.dcbook` collections into a new archive, preserving their
initial layouts and original files. Older builds must reject the new format.

Deliver a publication tree and desktop/browser open, save and recovery workflows.
The first usable result is a multi-chapter publication with reliable editing and
undo.

## 2. Shared metadata, styles and numbering

Extend the existing [book commands](crates/engine/src/cmd/book.rs),
[style model](crates/doc/src/styles.rs) and
[numbering support](crates/doc/src/attrs/numbering.rs).

Add publication metadata and reusable values, chapter reordering, and named
numbering sequences for chapters, headings, figures and tables. Support continuous
numbering, scoped restarts, prefixes and front-matter conventions.

Replace publication style synchronization with a preview showing proposed changes
and affected components. Permit selective application as one undoable edit.
Preserve intentional chapter overrides unless explicitly selected for replacement,
and reject application of a preview whose underlying publication has changed.

Imported chapters with identically named but different styles must retain their
appearance until the user resolves those differences.

## 3. Publication references and generated content

Build a shared, typed query and template layer around
[references](crates/doc/src/xref.rs), [variables](crates/doc/src/vars.rs),
[TOCs](crates/engine/src/cmd/toc.rs) and
[captions](crates/engine/src/cmd/captions.rs).

Queries select semantic objects by identity, kind, component, level or range.
Templates combine their titles, numbers, metadata and resolved locations. Use this
for cross-references, multiple scoped TOCs, figure/table lists, captions and running
heads.

Generated text must support ordinary wrapping and rich formatting while retaining
its source binding. Include the proposed running-head rule: the latest qualifying
heading before the page's first body content, with suppression on chapter-opening
pages.

Introduce dependency tracking and a bounded composition/update cycle here.
Pagination-dependent fields must converge or report a failure. Provide live and
deferred updating with visible stale status; export resolves against the current
publication. Reference selection and inspection should use the semantic index
without triggering full layout merely to open a dialog.

## 4. Multiple structured indexes

Extend [index data and generation](crates/doc/src/index.rs) and
[index commands](crates/engine/src/cmd/index.rs) from one index to named index
definitions.

Each definition has its own topics, aliases, hierarchy, sorting rules, formatting
and generated destination. Occurrences use persistent anchored ranges, allowing
reliable range consolidation and references inside tables and notes.

Support ordinary alphabetical/numeric keys, configured biblical book order with
chapter-and-verse ranges, and archival keys containing shelfmarks, folios and
recto/verso values. Do not assume one biblical canon or silently rewrite display
names.

Add a publication-wide preview showing every occurrence and unresolved destination
without regenerating the printed index. Migrate the existing index into a default
named index.

## 5. Independent note streams and critical apparatus

Extend [notes](crates/doc/src/notes.rs), [endnotes](crates/doc/src/endnotes.rs) and
[note composition](crates/compose/src/notes.rs) so note identity and content are
independent of their placement.

Add named streams with independent numbering, restart rules, reference formatting
and paragraph styles. Edition policies choose footnotes, component endnotes,
publication endnotes or sidenotes. Implement run-in notes and note areas whose
columns differ from the body.

Support continuation of long notes, keeping the initial fragment with its callout.
Sidenotes preserve order, avoid collisions and report overflow. Maintain editable
rich content and links between notes and their callouts.

Extend [TEI conversion](crates/textimport/src/tei) to retain structured apparatus
readings and witness associations for edition selection. Existing imported
readings remain intact; recovering additional structure from preserved XML must
not overwrite subsequent edits.

## 6. Several editions and complete publication outputs

Expose edition creation and configuration using the model established in release
1. A scholarly print edition, reading edition and ebook share canonical content
while selecting their own styles, included annotation streams, generated sections
and layout rules.

Update [PDF output](crates/pdf/src), [EPUB/HTML output](crates/epub/src) and
[export commands](crates/engine/src/cmd/export/books.rs).

PDF export must resolve references and navigation across the entire publication.
EPUB and HTML must follow semantic reading order and serialize tables,
illustrations, notes, apparatus, anchors and backlinks. The current EPUB text path
skips internal note/reference markers, so this requires explicit conversion work.

Export diagnostics identify unresolved references, unsupported content and overset
material. Content must not disappear silently. Each edition can be saved, reopened
and regenerated independently.

## 7. Publication-wide revision review and proof comparison

Extend [tracked changes](crates/engine/src/cmd/changes.rs) with persistent change
identities, author attribution, anchored comments and publication-wide
accept/reject operations.

Add named proof baselines containing the content revision, edition settings,
composition results and the font/composition inputs needed to interpret
differences. Compare baselines by stable identity to distinguish content changes,
formatting changes, moved objects and pagination shifts.

Produce a navigable report covering affected chapters, moved figures, reference
resolution and index occurrences whose passages were removed.

Add revised-manuscript comparison using the existing importers. Match persistent
source identities first; present uncertain text-based matches for review before
applying changes. Approved proofs remain immutable, and revisions are applied
transactionally.

## 8. Incremental pagination at book scale

Extend [composition](crates/compose/src/lib.rs),
[caching](crates/compose/src/cache.rs) and
[page reflow](crates/engine/src/reflow.rs) using the dependencies introduced in
release 3.

Record changed content ranges and introduce resumable composition checkpoints.
Restart before the affected paragraph and its keep dependencies, then propagate
through subsequent frames until both output and continuation state match the
previous composition.

Checkpoints must account for numbering, continuing notes and tables, running
heads, wrapping objects and generated fields. Retain full composition as the
correctness reference and fallback.

Instrument recomposed paragraphs/frames, reference updates, elapsed time and
memory. Verify local edits, chapter insertions and global style changes on an
800–1,000-page generated publication with at least 3,000 references. This release
must demonstrate reduced work where dependencies permit it, while still allowing
genuine pagination cascades.

## Integration across releases

Every release should expose its operations through the existing command registry,
making them available to CLI, control-channel and MCP clients. The UI should invoke
those same commands. Browser workflows use byte-based archive delivery.
Documentation should ship with each release's commands and format changes.

## Verification

- **Persistence and transactions:** legacy loading, exact preservation of TEI bytes
  and assets, stable identities through copy/move/import, failed edits preserving
  state, and publication-wide undo/redo.
- **Publishing behavior:** independent indexes and numbering streams; chapter
  reordering; references after reflow; wrapping generated text; multi-column,
  run-in and continuing notes; and several editions sharing content.
- **Output and interaction:** inspect representative PDF pages and whole-window UI
  captures, verify PDF destinations and EPUB/HTML links, and exercise desktop,
  browser and headless workflows.
- **Real and generated books:** reuse the supplied British Kymry XML for migration
  and content preservation. Add committed fixtures with actual encoded notes and
  apparatus, which that XML lacks. Compare incremental composition against full
  composition.
- Run focused tests in the affected crates, followed by `cargo xtask ci` for each
  release.

## Boundaries

This plan reuses the Rust stack and current typography engine. It does not include
hosted collaboration, a replacement GUI framework, reconstruction of missing
scholarly markup, or writing edited content back over original TEI files.
