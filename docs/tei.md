# Importing TEI books

Open a TEI P5 `.tei` or `.xml` book with **File → Open**, or use **File → Import TEI
Book**. **Place** inserts it at the text selection, replaces the selected text
frame, or creates a frame using the existing placement rules. Both desktop and
browser imports show reading, style, and illustration options before importing.
The resulting report is also available from **File → TEI Import Report**.

Opening creates an unsaved document named from the TEI title. Save the edited
layout as `.designcraft`. The source XML is retained byte-for-byte; DesignCraft
does not reconstruct TEI from layout edits or adopt the XML path as its save path.
**Save Original TEI** retrieves the retained input, including its original encoding
and line endings. It is not an export of the edited text.

## Reading and layout

The importer recognizes the `http://www.tei-c.org/ns/1.0` namespace, including
prefixed elements. `TEI`, nested/composite texts, and `teiCorpus` are read in source
order. Each `text` has its own book navigation destination. The header is metadata,
not reading text. These mappings follow the TEI definitions of
[text structure](https://tei-c.org/release/doc/tei-p5-doc/en/html/DS.html),
[editorial alternatives](https://tei-c.org/release/doc/tei-p5-doc/en/html/ref-choice.html),
and [apparatus entries](https://tei-c.org/release/doc/tei-p5-doc/en/html/ref-app.html).

| TEI content | Editable result |
|---|---|
| Headers | Titles, contributors, edition, publication and source descriptions, identifiers, languages, and rights in the source report |
| Front, body, back; divisions and headings | Source order and heading levels; `head` and `ab type="heading"` become styled headings with navigation tied to text anchors |
| Paragraphs, quotations, lists and mixed inline text | Styled paragraphs and character formatting; default XML whitespace is collapsed across inline boundaries, `xml:space="preserve"` retains it |
| `lg`, `l` | Stanza spacing and one editable paragraph per verse line; no automatic hyphenation in the verse style |
| Speakers, speeches, stage directions and cast lists | Speaker, stage, body/verse and list styles in reading order |
| `choice` | First available `corr`, `reg`, or `expan`, then `orig`, `sic`, `abbr`, or first child; alternatives are never concatenated |
| `add`, `del`, `supplied`, `unclear`, `gap` | Additions included; deletions omitted with a notice; supplied text bracketed; uncertainty and gaps explicitly marked |
| `app` | Selected witness when available, otherwise lemma, otherwise first reading with a diagnostic. Alternatives and witness labels become endnotes |
| `note` | Footnotes by default; explicitly inline notes remain inline, endnotes remain endnotes, marginal notes become endnotes with a notice. Table notes become endnotes; nested notes are retained inline with a notice |
| `table`, `row`, `cell` | Native editable tables, header formatting, row and column spans, and rich cell text |
| `figure`, `graphic` | Anchored embedded illustrations, captions/descriptions; images scale proportionally to fit the text area. Missing/unsupported images retain a visible frame and a source diagnostic |
| `xml:id`, `ref`, `ptr` | Stable anchors and literal link labels. Internal destinations follow edits/reflow; HTTP(S) and mail links remain links |
| `pb` | Navigable source-page marker, without forcing original pagination |
| `lb` | A space in prose; a structural line break in verse/preserved whitespace. `break="no"` joins the divided word |
| Specialized or unknown content | Readable descendants retained once, or a visible placeholder. A diagnostic identifies the fallback and the unchanged encoding remains in the source |

Paragraph styles are named `TEI Body`, `TEI Title`, `TEI Subtitle`, `TEI Heading 1`
through `3`, `TEI Quote`, `TEI Verse`, `TEI Speaker`, `TEI Stage`, `TEI List`,
`TEI Caption`, `TEI Note`, `TEI Apparatus`, `TEI Table`, `TEI Bibliography`,
`TEI Small`, and `TEI Verse Heading`.
Style mapping applies inside table cells and notes as well as main text.

`rend` tokens and referenced `rendition` declarations support italic/bold,
small caps, capitals, underline, strikethrough, super/subscript, alignment, font
family/size, and paragraph margins/indentation. Lengths accept points, pixels,
inches, centimetres and millimetres. Unsupported declarations produce notices.
On `p`, `ab`, and `head`, the named block renditions `p`, `quotation`, `small`,
`title`, `heading`, and `versehead` select body, quote, small print, title,
depth-based heading, and verse-heading styles respectively. These are importer
mappings for reading masters; external stylesheets are not loaded.
This is a book reading importer, not a full renderer or schema validator for every
TEI module, CSS property, or diplomatic transcription convention.

Opening flows the body and rich endnotes through new pages. Unused trailing pages
created by autoflow estimates are removed; existing pages are retained. Placement honors its
`autoflow` setting; without autoflow the result reports overset text. Unplaceable
content and exhausted page limits return errors. A failed import preserves the
previous document, selection and undo history. Successful placement is one edit:
undo/redo includes styles, assets, references, preserved sources, and added pages.

## Illustrations

Desktop imports resolve relative illustration URIs beside the source XML and
apply inherited `xml:base`. Byte/browser imports use an explicit resource map.
Choose files in the import dialog to supply or override illustrations. Dropping
one TEI file with its images maps unique matching filenames; ambiguous names must
be assigned explicitly. Unresolved images are reported and remain placeholders.

There are no network resource requests. DTDs, external entities, and XIncludes are
not fetched. Unresolved entity references fail instead of silently losing text.

## Commands

All commands below are engine commands, available through CLI scripts, the running
desktop control channel, and headless or connected MCP `execute`.

| Command | Parameters and result |
|---|---|
| `file.openTei` | `{path}` or `{name, base64}`, plus options below; creates a new unsaved document and returns `index`, `story`, `source`, `pagesAdded`, `overset`, `warnings` |
| `file.open` | `{path, tei?, resources?}`; recognizes TEI roots and TEI/XML filenames |
| `file.openBytes` | `{name, base64, tei?, resources?}`; same importer |
| `file.place` | TEI path/bytes plus `frame?`, `page?` (1-based), `rect?`, `autoflow?`, `removeStyles?`, `styleMap?`, `styleConflicts?`, `tei?`, `resources?` |
| `tei.inspect` | Same source input and `tei?`; returns metadata, declared witnesses, styles, illustration URIs and parser diagnostics without modifying a document |
| `tei.report` | `{source?}`; returns an array of stored reports, including navigation with current 1-based pages; omitted source returns all reports |
| `tei.source` | `{source?}`; returns `{id, name, base64}` for the unchanged original; defaults to the most recent import |
| `tei.goTo` | `{anchor}`; selects its destination and returns the current 1-based page |

Options: `tei: {witness: "B"}` selects the witness with `xml:id="B"`; a leading
`#` is accepted. Omit it for the lemma. `styleMap` maps imported names to existing
styles. `styleConflicts` is `useExisting` (default), `redefine`, or `autoRename`.
`removeStyles` defaults to false. Placement `autoflow` defaults to false; opening
always enables it. Reading and layout options are stored with the import receipt.

Resource-map keys are the illustration URIs reported by `tei.inspect`, after
`xml:base` resolution. Values are `{path: "/absolute/image.svg"}` on desktop, or
`{name: "image.svg", base64: "…"}` on any platform. No basename guessing occurs in
engine commands. Byte payloads use standard padded base64.

```sh
cargo run -p designcraft-cli -- run --in book.tei --export book.designcraft
cargo run -p designcraft-cli -- run --in book.tei --all-pages rendered-book
```

```json
{"id":1,"method":"engine.execute","params":{"command":"file.openTei","params":{"path":"/books/book.xml","tei":{"witness":"B"},"resources":{"plates/front.svg":{"path":"/books/images/front.svg"}}}}}
{"id":2,"method":"engine.execute","params":{"command":"tei.report","params":{}}}
```

UI commands `app.teiImport`, `app.teiReport`, and `app.teiSource` expose the same
dialog/report/source-download actions through a connected application. Browser
file delivery retains whether the user chose Open or Place. The browser has no TCP
control server. The desktop control transport limits request lines to 4 MiB;
use a native path for larger books there.

## Diagnostics, limits and native compatibility

Every diagnostic records a code, element name, optional `xml:id`, decoded-source
line and byte column, and a message explaining the consequence/fallback. Notices
cover unsupported vocabulary/renditions, missing images and link targets, omitted
deletions, marginal/table/nested notes, and witness/reading fallbacks. Inspect the
report after import; retaining source is not a claim that every source feature
was typeset.

XML is bounded to 64 MiB, 128 nesting levels and one million parsing events.
XML 1.0 in UTF-8 and BOM-marked UTF-16 is supported; malformed XML, duplicate IDs, unresolved
entities, invalid character references and unsupported/mismatched encodings fail.
Tables are bounded to 10,000 rows, 256 columns and one million cells. Illustration
inputs are limited to 64 MiB each, 256 MiB total and 100 million pixels per image.
Automatic flow adds at most 2,000 pages per story and detects lack of progress.

Standalone native saves use **format version 2**, or **version 3** when the import
contains structured apparatus readings. `document.json` contains source records,
metadata, reading/layout options, diagnostics, navigation and resource/asset IDs;
`sources/<id>.xml` stores each original separately. Source bytes are required:
missing entries and length mismatches fail loading. Embedded illustrations use the
existing `assets/` entries. Version 1 archives still open; saving upgrades them to
the required version. Older builds reject newer versions instead of discarding preserved sources
on a later save. Historical single-file JSON remains unsupported.

Explicit conversion to a [publication](publications.md) uses format version 3,
retaining original TEI bytes in required source entries while sharing editable
content across editions.

Authored fixtures are in `crates/textimport/tests/fixtures/`, attributed in
`ASSETS.md`. Parser, engine, document and archive tests cover content order,
alternatives, malformed input, resources, rich notes, references after reflow,
source preservation, transactions and a generated book with hundreds of notes.
