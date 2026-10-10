# Publications and editions

A publication is one `.designcraft` archive containing an ordered tree of front
matter, chapters and back matter. Its editions share editable rich text, tables,
notes, illustrations and preserved TEI sources. Each edition keeps separate page
geometry, frame threads, typography and style definitions. The editor displays one component
of one edition at a time; Save writes the entire publication.

## Create and navigate

Use **File → New Publication** or **File → Convert to Publication**, then open
**Window → Publication**. Conversion opens a new unsaved tab and leaves the original
document and its file intact. Add a blank chapter, import a standalone document,
or use **Convert Book to Publication** to copy the chapters of a `.dcbook` file.
Right-click a chapter for its title, front/chapter/back role and ordering controls.

Native, IDML and TEI imports use the same opening logic as ordinary documents.
TEI import retains its reading options and illustration dialog. Imported chapters
keep their initial layouts and their own style definitions, including definitions
that share names with another chapter but have different values.

The edition selector changes the current view. **New edition** copies the current
layouts. Text edits are shared. Layout and style-definition edits affect the
selected edition. New stories added in one edition are retained as unplaced text
in other editions until given frames; deleting a story removes its frames from
every edition. Direct paragraph/character formatting, table and cell presentation,
and anchored-object presentation belong to the selected edition. Shared text edits
carry each edition's formatting through the changed ranges; table cells retain
their identities when rows move or are inserted. Conditions, change marks, language
and XML tags remain shared with the text. Named annotation streams have independent
numbering and edition placement policies. Whole-publication output is a later stage.
At this stage existing export commands still operate on the displayed component.

Chapter and edition navigation do not create undo steps or dirty a saved
publication. Edits, chapter imports, reordering and edition creation are undoable
in the publication's single history. Undo restores the component, edition and
selection associated with that edit.

## Metadata, values and numbering

The Publication panel's **Metadata** dialog edits bibliographic fields and reusable
text values. Both are shared by all chapters and editions. A text variable with
type `publicationValue`, a `key`, and optional `metadata: true` displays the current
value. Missing keys display an explicit missing-value label. Variable definitions
and their marker indices remain consistent across editions.

Use **Numbering** to define named chapter, heading, figure or table sequences.
Each sequence has a numeral style, starting value, prefix/suffix, subtree and role
filters, and continuous, per-component, per-parent or per-role counters. Explicit
chapter restarts override the counter at that component's first matching object.
A separate front-matter sequence can use Roman numerals while the chapter sequence
uses Arabic numerals. No numbering convention is forced onto imported chapters.

Chapter identities already exist. For headings, figures and tables, select their
paragraph or caption and use **Mark passage**. The marker follows text edits,
including in table cells and footnotes. A blank display title follows the current
paragraph text. A removed anchor remains visible as an unresolved target in the
numbering query. Registering a marker does not insert a printed numeral; insert a
`publicationNumber` text variable with its sequence and object identity for a bound
numeral. Omit the object identity to show the current chapter's number.

Numbering follows the publication tree and the canonical reading order established
when stories are imported. Moving frames in one edition does not reorder the
shared text. Sequences support 1–100,000 for compact numerals, up to 2,600 for the
repeating-letter formats, and up to 600 for repeating symbols. Out-of-range results
return an error rather than allocating unbounded labels.

## Reviewed style synchronization

Select a source chapter and choose **Synchronize styles**. The preview lists each
target chapter, its current definition, the proposed definition, dependencies and
whether the chapter has an override. Paragraph, character, object, cell and table
styles, swatches, export tags, composite fonts and mojikumi definitions can be
selected separately. Expand **Changes** to inspect differing properties.

Imported differences are overrides and start unselected. Replacing them requires
selecting the row and **Replace selected chapter overrides**. Identical definitions
can be explicitly adopted with **Show identical definitions**; later source changes
then count as updates while that target definition remains unchanged. Local text
formatting and definitions in other editions are preserved. Color and dependency
definitions are separate selections; the preview identifies required named styles.
For object, table and cell styles, synchronization also updates attributes copied
onto content by the original style application. Values that differ from the old
style remain local overrides, including individual cell edges and insets.

Apply commits every selected change as one undo step. The command retains the
publication snapshot used to build the preview and rejects a different snapshot.
Reopen the preview after intervening edits. Preview creation does not compose pages
or alter the document. A preview contains at most 100,000 rows; select fewer target
components for a larger publication.

Shared text alignment uses bounded character matching (1,024 insertion/deletion
steps after trimming unchanged ends, 16 million comparisons and a 16 MiB changed
region). Ordinary contiguous insertions and deletions need no matching. An edit
that exceeds these limits is rejected with an instruction to split it into smaller
operations; it does not silently discard another edition's formatting. Nested
table/note presentation is limited to 32 levels.

## Commands

These engine commands are available through CLI, the running application's control
channel and MCP. UI controls invoke these same operations.

| Command | Parameters and result |
| --- | --- |
| `publication.new` | `{title?, componentTitle?}` creates an unsaved publication with one print edition and chapter. |
| `publication.convert` | `{title?}` copies the active standalone document into a new unsaved publication. |
| `publication.importBook` | `{path?, title?}` copies a `.dcbook`; omitted input uses the currently open Book. Relative chapter paths resolve beside the Book file. Byte input uses `{name, base64, documents: {bookPath: {name, base64}}}`. Every chapter must load before the new tab is created. |
| `publication.list` | Returns ordered components, their roles, parent and children, page counts, persistent content identities, editions, metadata and revision. It does not compose pages. |
| `publication.addComponent` | `{path? \| base64 + name, title?, role?, parent?, tei?, resources?}` imports a standalone document. Omit the file for a blank chapter. Roles are `frontMatter`, `chapter`, `backMatter`. |
| `publication.activate` | `{component?, edition?}` changes the editor projection. |
| `publication.component` | `{component?, title?, role?}` updates a chapter's properties. |
| `publication.moveComponent` | `{component, parent?, at?}` moves a subtree. `parent: null` selects the root; `at` is a zero-based sibling index after removal. Cycles are rejected. |
| `publication.edition.create` | `{name, source?}` copies an edition layout and shares its canonical content. |
| `publication.metadata` | `{title?, metadata?: {key: string|null}, values?: {key: string|null}}` patches shared fields. Null removes a key. |
| `publication.numbering.list` | Returns named sequences and stable semantic objects. |
| `publication.numbering.define` | `{name, kind?, style?, start?, prefix?, suffix?, restart?, scope?, role?, level?, restartAt?}` creates or replaces a sequence. Kinds: `chapter`, `heading`, `figure`, `table`. Restart modes: `continuous`, `component`, `parent`, `role`. `restartAt` maps component ids to starting values. |
| `publication.numbering.mark` | `{kind, story?, pos?, level?, title?}` registers a heading/figure/table paragraph. Omit story and position to use the selected text, cell or footnote. Returns `{object, anchor}`. |
| `publication.numbering.resolve` | `{name}` returns ordered identities, titles, numeric values, formatted labels and resolution status without composition. |
| `publication.numbering.insert` | `{name, object?}` inserts a bound numeral as one edit. The Publication panel offers Insert beside resolved numbers. Omit object for the current chapter. |
| `publication.value.insert` | `{key, metadata?: false}` inserts a shared value as one edit. The Publication panel lists the available values and Insert controls. |
| `publication.styles.preview` | `{source?, sourceEdition?, edition?, components?}` returns a preview token and rows with before/after definitions. Defaults to the active chapter/edition and other components. |
| `publication.styles.apply` | `{preview, rows?, replaceOverrides?: false}` applies selected row indices. Omitted rows selects changed definitions that are not overrides. |
| `variables.define` | Also accepts `type: publicationValue` with `{key, metadata?}`, or `type: publicationNumber` with `{sequence, object?}`. Use `variables.insert` to place an instance. |
| `file.open`, `file.openBytes` | Recognize standalone and publication archives. |
| `file.save`, `file.saveAs`, `file.saveACopy`, `file.serialize` | Write the entire publication, including inactive components and editions. Native saves flush a new sibling file before replacing the destination. |
| `file.recovery.snapshots` | Returns complete native recovery archives for dirty documents/publications. |
| `file.recoverBytes` | `{base64}` opens a native recovery archive as a new unsaved tab. |
| `file.markDownloaded` | `{revision, name}` acknowledges successful host delivery. A changed revision is rejected, preserving the unsaved state. |

The existing `tei.report` and `tei.source` commands inspect the displayed
component's sources. Select a component with `publication.activate` to inspect its
imports. Preserved sources cannot be changed or discarded by editing a projection.

## Browser and recovery

Open and Save use named bytes and downloads. Chapter selection retains the target
publication while the browser picker is open. Book conversion accepts the `.dcbook`
and its chapters together; ambiguous or missing filenames produce an error rather
than selecting a chapter arbitrarily.

The browser saves complete unsaved-document snapshots to IndexedDB at the configured
recovery interval. **File → Recover Browser Documents** opens previous browser-run
snapshots as unsaved tabs. Recovery copies from earlier runs remain in site storage;
they are not erased merely by opening them. A browser run only replaces or removes
its own snapshots. Storage failures, including quota exhaustion, appear in the
application status. Clearing the site's browser data also removes these snapshots.
Downloads remain the portable saved copies.

For a GPU-independent browser storage check, build `recovery-check.html` in
`apps/designcraft-web` with Trunk into a fresh output directory and serve it locally.
The opt-in `browser-checks` target uses the actual Rust IndexedDB adapter and native
archive loader. It verifies exact bytes, chapters, editions, unsaved restoration
and other-run retention, leaving a named evidence snapshot in that test origin.
It does not verify the rendered editor or browser file-picker interactions.

Desktop recovery uses the existing recovery directory and includes every component
and edition. A failed import or candidate validation leaves the active publication,
selection and undo/redo state intact.

## Archive compatibility and identities

Publication archives use **format version 3**. They contain `meta.json`,
`publication.json`, unchanged `sources/<sourceId>.xml`, and embedded
`assets/<resourceId>.<ext>`. The archive manifest records required asset lengths;
source records retain their original byte lengths. Missing or truncated required
entries fail to open. A publication is not also written as a partial `document.json`.

Standalone documents save as version 2 unless they contain structured apparatus
or persistent publication-note identities. Those documents use version 3 with
`kind: "document"` and `document.json`; publications use `publication.json`.
Versions 1 and 2 remain readable; conversion to a publication is explicit.
Older builds reject version 3 rather than dropping the new records when saving.

Content, resources, sources, components and editions have archive-wide identities.
Chapter-local legacy item/story ids remain usable by editing commands. Importing
the same document twice allocates new canonical identities. Moving a chapter or
switching editions retains its identities. Frame threads and page models contain
no duplicate authored story content or embedded attachments.

Current bounds are 10,000 components, 64 editions, 256 MiB of publication JSON,
256 MiB per asset, 64 MiB per TEI source and 1 GiB total uncompressed publication
JSON plus attachments. Exceeding a bound returns an explicit error.

## References, generated sections and running heads

The Publication panel provides **Reference**, **Generated section**, **Templates**
and **Register destinations**. Registration can use preserved TEI heading anchors
or explicitly selected paragraph styles. Repeating registration preserves existing
identities. A paragraph or caption can also be registered individually with
**Mark passage**, including in cells and notes.

A query selects persistent object identities, a kind, component or subtree, role,
heading levels, and an inclusive range between two identities. Results follow
canonical reading order. The selector uses the semantic index; opening it does not
compose the book. Missing explicit identities remain visibly unresolved.

Templates contain ordered literal, title, component title, publication title,
number, page label, global page position, metadata and reusable-value parts. Each
part accepts a character style and local character formatting. **First** references
use one matching object; **Join** combines matches with a separator. These fields
also support captions. Generated text wraps as ordinary text and keeps its binding,
Unicode clusters and link destinations through line breaking. Editing the marker
changes or removes the entire field. Copying a field within the same open publication
retains its binding. Cross-publication formatted paste is rejected because a numeric
identity in another book can name a different object; Paste without Formatting uses
the copied visible text.

**Running head** selects the latest qualifying heading strictly before the page's
first body content in canonical order. It can suppress the first body page of each
chapter. Insert it in a header frame or parent-page header. A page with no qualifying
heading has an empty header. Primary text defines the body where configured;
otherwise ordinary stories excluding generated sections and running-head stories do.

Each named generated section owns a dedicated story and a new page. It can be a
TOC, a scoped contents list, or a figure/table list. Its entries are bound fields,
with title, entry and optional per-level paragraph styles. Reordering entries keeps
their identities and edition formatting. Text typing into a generated story returns
an error; change its query/template or detach it first. Detaching retains editable
bound entries and stops membership regeneration. Editions created before a generated
section receive their own frames when updated.

Live updates compose the active edition after an edit, inside the same undo step.
Deferred mode retains the previous values and displays **update needed**. **Update
now** and export resolve current fields. Updates iterate until destinations, page
body boundaries and field text agree, with an eight-pass limit and explicit errors
for nonconvergence or unplaceable content. Primary stories, endnotes and generated
stories autoflow; other overset stories require a frame or a primary-story setting.
A failed update preserves the prior document, selection and history. Inactive
editions keep their own cached values and update when requested.

Reference state records per-field dependencies, resolved destinations, diagnostics
and its source revision. Current limits are 100,000 field definitions, 1,024
templates, 1,024 generated sections, 128 parts per template, 1 MiB per expanded
field, 64 MiB of generated text per edition, and 10,000 pages per update. These
bounds return errors rather than truncating output.

| Command | Parameters and result |
| --- | --- |
| `publication.semantic.scan` | `{component?, tei?:true, styles?:[{style,kind,level?:1}]}` registers existing headings/captions once. |
| `publication.query` | `{query?:{ids?,kind?,component?,subtree?,role?,minLevel?,maxLevel?,between?}}` returns semantic objects without layout. |
| `publication.template.define` | `{name, parts:[{token:{type,...},format?}]}` creates/replaces a template. Token types: `literal` (`text`), `title`, `componentTitle`, `publicationTitle`, `number` (`sequence`), `page`, `globalPage`, `metadata`/`value` (`key`). |
| `publication.reference.insert` | `{query,template,use?:first\|join\|runningHead,separator?,suppressOpening?:true}` inserts a bound field at the caret and returns its identity. |
| `publication.reference.edit` | `{id,query,template,use?,separator?,suppressOpening?}` edits a field definition without changing its identity. |
| `publication.generated.define` | `{name,query,template?:"Contents entry",title?,titleStyle?,paragraphStyle?,levelStyles?,component?}` creates/reconfigures a named section. `levelStyles` maps heading levels to paragraph styles. Existing sections keep their destinations. |
| `publication.generated.detach` | `{name}` makes the section text editable and stops regeneration. |
| `publication.references` | Returns definitions, resolved locations, dependency records, update mode, revision, diagnostics and stale status. |
| `publication.updateMode` | `{mode:live\|deferred}` changes the active edition's update policy. |
| `publication.update` | Composes and resolves the active edition transactionally; returns pass/page/field counts and diagnostics. |
| `publication.reference.goTo` | `{object}` activates the destination chapter and returns its page; the UI scrolls there. |

PDF output of the displayed component includes generated-text links to destinations
in that component, including links inside table cells and notes. Whole-publication
Publication PDF destinations and semantic EPUB/HTML serialization are described in the edition output section below.

## Multiple structured indexes

The Publication panel offers **New index** and separate settings, topics, marking,
preview and output controls for each index. Topic spelling is independent of its
sort key. Parent topics form hierarchies up to 16 levels; **See** aliases and
**See also** links name persistent topic identities. Cyclic aliases and missing
parents are rejected. Renaming an index preserves its topics, occurrences and
output destination.

Select a passage and choose **Mark occurrence** to create a persistent range, or
place the caret to mark one page. Occurrences can belong to prose, table cells,
footnotes or endnotes. Endpoints follow text edits and story threading. Cutting a
passage including its anchors and pasting it into another chapter moves the
publication binding; additional pastes receive separate anchor identities.
Deleting a passage or its anchors produces an explicit unresolved occurrence and
printed placeholder. It does not silently attach the entry to nearby text. An
exclusive range ending at a page break uses the preceding page.

**Preview** lists every occurrence, excerpt, resolved page and unresolved reason
throughout the publication. It does not regenerate the index or compose pages.
Deferred edits show the previous pagination with a stale indicator. Select an
occurrence and press OK to navigate to its page. **Generate / update** creates a
dedicated bound story or updates its existing destination. Multiple indexes remain
independent. Editing the bound text directly is rejected; **Detach editable output**
retains its visible rich text and the index's source definitions. Copies of bound
entry fields elsewhere in the publication retain their bindings after detachment.

Formatting includes title, section and per-level paragraph styles, topic and page
character formats, run-in subtopics, separators, and publication page numbers,
component page labels, or component titles plus labels. Duplicate, overlapping and
adjacent page spans within a component consolidate; links retain their endpoint
destinations. Alphabetical section headings follow sort keys. Generated text wraps
and supports the same rich shaping and bounded update cycle as other references.

| Sorting mode | Typed topic key |
| --- | --- |
| `alphabetical` | `{type:"alphabetical", key:""}`; an empty key uses the unchanged display name. |
| `numeric` | `{type:"numeric", value:12}`; orders numerically, independently of the display name. |
| `biblical` | `{type:"biblical", book:"Jn", chapter:3, verse:16, endChapter:4, endVerse:2}`. An index supplies its own ordered `bookOrder` names and optional `bookAliases` mapping. Unknown books and reversed ranges fail explicitly. No canon or printed spelling is supplied implicitly. |
| `archival` | `{type:"archival", repository:"NLW", collection:"Peniarth", shelfmark:"MS 10", folio:23, side:"verso"}`. Repository and collection precede a natural numeric shelfmark key, then folio and recto/verso. |

| Command | Parameters and behavior |
| --- | --- |
| `publication.index.list` | `{}` returns definitions, topics, occurrences and output destinations. |
| `publication.index.define` | `{name, rename?, sorting?, bookOrder?, bookAliases?, format?}` patches index settings without replacing topics or occurrences. Format keys: `title`, `titleStyle`, `sectionStyle`, `levelStyles`, `topicFormat`, `pageFormat`, `sectionHeadings`, `runIn`, `followingTopic`, `betweenPages`, `pageRange`, `betweenEntries`, `beforeCrossRef`, `pageLabels` (`global`, `local`, `component`). Character formats accept `{style?, over?}`. |
| `publication.index.topic` | `{index, id?, display?, parent?, sort?, see?, seeAlso?}` creates or edits a topic; creating requires a display name and a key matching its index's sorting mode. Returns `{topic}`. |
| `publication.index.mark` | `{index, topic, story?, start?, end?, path?, suppressPages?}` uses the current selection by default. Explicit positions are UTF-8 byte offsets. A path selects nested content using `{type:"cell",table,row,col}`, `{type:"footnote",id}` or `{type:"endnote",id}` steps, with at most 32 levels. Returns `{occurrence}`. |
| `publication.index.addOccurrence` | `{index, topic, start:{component,content,anchor}, end?:{component,content,anchor}, suppressPages?}` binds existing anchors, including ranges crossing chapters. |
| `publication.index.removeOccurrence` | `{index, id}` removes the binding while retaining source text and anchor markers. |
| `publication.index.preview` | `{index}` returns ordered topics, every occurrence and generated rows with rich runs, unresolved explanations and stale status, without layout. |
| `publication.index.generate` | `{index, component?, page?, rect?}` creates a destination in the current component by default. Omitted page adds a new last page; supplied page is one-based. Existing output retains its destination. |
| `publication.index.goTo` | `{index, id, end?}` opens a resolved occurrence's component and page. Removed or unplaced destinations report errors. |
| `publication.index.detach` | `{index}` converts the current output to editable rich text and retains the source definitions. |
| `publication.index.migrate` | `{}` migrates legacy component markers once. Conversion/import performs this automatically. |

The existing `index.addReference`, `index.generate`, `index.list` and `index.update`
commands use named index state when a publication is open; `index` selects a name
and defaults to `Index` for marking/generation. Standalone documents retain their
existing behavior. Conversion preserves legacy topic hierarchy, sort keys,
See/See-also references, end-of-story/paragraph ranges and existing output frames.
If several imported components have their own legacy index, the first is named
`Index`; later ones include their component title and identity to remain distinct.
Original files are unchanged. Each index supports at most 100,000 topics and
100,000 occurrences; a publication supports at most 1,024 named indexes.

## Independent notes and apparatus

Open **Notes and critical readings** in the Publication panel. A stream owns its
numbering style, restart rule, callout formatting, paragraph style and endnote
heading. Each edition independently places that stream as footnotes, chapter
endnotes, publication endnotes, sidenotes, or omits it. Changing placement retains
the same note identity, canonical rich content and callout. It does not turn the
printed endnote area into a second editable copy.

Note areas support 1–12 columns independently of body columns. Run-in placement
joins adjacent single-paragraph notes; multi-paragraph notes keep their paragraph
structure. Long footnotes continue through later frames. The initial fragment
stays with its callout; unplaceable material and page-limit exhaustion reject the
candidate edit. Sidenotes use lanes inside the text frame, keep source order and
report collisions or overflow. Reduce their width or choose another placement
when the available area is insufficient.

Click a note fragment to edit its canonical text, including on continuation pages.
The note browser can visit the printed note or callout and open a Note Editor.
Clicking a publication-end note from another chapter activates its owning chapter
and opens that editor. Ordinary text and character/paragraph formatting commands
work after `publication.note.select`. Plain-text Note Editor changes replace the
smallest changed range, preserving unaffected rich formatting and embedded objects.
Gathered notes retain the owning chapter's edition styles, swatches and assets.

Legacy footnotes and endnotes acquire persistent identities during conversion.
Existing note paragraph styles are retained by default. A stream can explicitly
apply its paragraph style instead. Footnote page/spread/section restarts and
prefix/suffix placement are retained. Printed note labels link to callouts and
callouts link back to notes. Component PDF export resolves links within that
component; cross-component PDF navigation is part of publication export.

TEI apparatus now retains each reading's rich text, lemma status, witness IDs and
witness labels. **Witnesses** changes the active edition's displayed inline
reading, preferring the chosen witness, then lemma, then the first reading.
Canonical imported text and original XML remain intact. Edited passages remain
as edited and are flagged in `publication.apparatus.list` and reference diagnostics.
Block readings containing paragraph breaks, tables or illustrations are retained
in the structured record and original XML; the current page-layout substitution
retains the imported passage and reports that limitation. Alternate-only links
retain their labels and source encoding, with an import diagnostic.

**Recover readings** previews matches against preserved TEI from older imports.
Unambiguous source IDs or matching note text can attach reading structure. It
never replaces current passage or note text. Ambiguous matches and edited passages
without recoverable boundaries remain unresolved. Recovery uses already embedded
illustrations and never loads an external path or network resource.

| Command | Parameters and result |
| --- | --- |
| `publication.notes.list` | `{stream?}` returns streams, policies, labels, rich note text and canonical owner paths. |
| `publication.notes.stream` | `{name, style?, startAt?, restart?, prefix?, suffix?, affixIn?, refPosition?, refCharStyle?, paraStyle?, preserveParagraphStyles?, heading?, headingStyle?, separator?, defaultPlacement?}` patches a stream. Restarts: `publication`, `component`, `page`, `spread`, `section`. Affixes: `none`, `reference`, `text`, `both`. |
| `publication.notes.policy` | `{stream, placement?, columns?, gutter?, runIn?, betweenNotes?, maxFraction?, spaceBefore?, spaceBetween?, sideWidth?, sideGap?, side?}` patches this edition's placement. Placements: `footnote`, `componentEnd`, `publicationEnd`, `sidenote`, `omit`. Side: `left`, `right`, `outer`. Measurements are points. |
| `publication.note.insert` | `{stream, text?, story?, at?, path?}` inserts at the selected caret or explicit UTF-8 byte offset. Nested paths contain `{type:"cell",table,row,col}`, `{type:"footnote",id}` or `{type:"endnote",id}`. |
| `publication.note.edit` | `{id, text?, stream?}` changes membership or replaces the minimal changed text range. |
| `publication.note.select` | `{id, anchor?, focus?}` selects canonical rich text for ordinary editing commands. Offsets are UTF-8 byte boundaries. |
| `publication.note.goTo` | `{id, target?:"note"|"callout"}` visits the cached destination, returning its component and zero-based page. Missing destinations are errors. |
| `publication.notes.migrate` | `{}` assigns identities to legacy notes and prepares required endnote areas. |
| `publication.apparatus.list` | `{}` returns readings, witnesses, edition selection and edited-passage status. |
| `publication.apparatus.witness` | `{witness:string|null}` changes this edition's selected witness; null keeps the imported reading. |
| `publication.apparatus.recover` | `{component?, source?, apply?:false}` previews recovery; `apply:true` attaches only unambiguous matches in one undo step. |

UI dialog modes are `noteStream`, `notePolicy`, `noteInsert`, `notes`, `apparatus`
and `apparatusRecover` on `app.publicationDialog`. `app.storyEditor {note:id}`
opens a canonical note directly. These UI operations also work through the running
application's control channel and connected MCP.

## Edition configuration and publication output

`publication.edition.get {edition?}` returns the edition's name, output metadata,
generated-section inclusion, note policies, witness and page count.
`publication.edition.configure` applies the following as one undoable edit:

```json
{
  "edition": 12,
  "name": "Reading edition",
  "output": {
    "title": "A Reading Edition",
    "author": "Editor name",
    "language": "cy",
    "identifier": "urn:example:reading",
    "excludedSections": [31]
  },
  "notePolicies": {"Apparatus": {"placement": "omit"}},
  "witness": "B"
}
```

Use identities returned by the commands for `edition` and `excludedSections`.
Output metadata fields set to `null` inherit publication metadata. Inclusion uses
stable generated-content identities; authored text remains shared. Excluding a
section hides its content while retaining that edition's allocated pages and
geometry. Remove unwanted empty pages through ordinary page editing. Detaching a
generated section makes it authored content, so the former exclusion no longer
hides it. Note placement and witness selection are independent per edition.

The Publication panel provides **Edition settings** and **Export edition**.
Typography, frame geometry and detailed note-area rules use the existing editing
and Placement controls while that edition is active.

`file.exportPdf`, `file.exportEpub`, `file.exportFixedEpub` and `file.exportHtml`
accept `edition`, `scope` (`publication` or `component`) and `strict`. A publication
exports all components in canonical order by default. `component` exports only the
current chapter. PDF page selectors address positions in the selected scope.
Explicit edition export does not switch the active chapter/edition, change the
selection or add undo history. A successful export of the active edition without
an explicit `edition` also publishes its resolved generated-content update, as in
live/deferred reference updating. Failed exports preserve editor state.

All four commands return `warnings`. `strict:true` fails before writing when any
export diagnostic remains. The UI displays the report after export. Existing
format-specific options still apply; output metadata falls back to the selected
edition and then the publication. Without `path`, PDF and EPUB return `base64`;
HTML returns `text`, suitable for browser download, CLI, control and MCP callers.

PDF uses one page map for the whole publication. Cross-chapter fields, source
anchors, gathered-note links, callouts/backlinks and heading navigation resolve
through that map. Identical chapter-local IDs do not alias. Page selections that
exclude a destination produce a diagnostic. PDF structure tags currently cover
ordinary paragraphs and figures; table/note text remains searchable but its
missing semantic structure tags are reported when tagged output is requested.

Reflowable EPUB and self-contained HTML serialize canonical reading order, mixed
formatting and languages, tables and spans, illustrations, rich notes, selected
apparatus readings, anchors, source-page markers and backlinks. Chapter style
rules and assets are qualified by their owning component. Notes are linked from
their callouts and collected in a semantic notes section; an omitted stream is
absent. Page-specific links fall back to the chapter start with a diagnostic.
Unsupported objects and missing resources retain visible descriptions and report
what could not be represented. Executable external link schemes are left inactive
and reported. Original TEI bytes and assets remain unchanged in the archive.

Fixed-layout EPUB retains the rendered pages, searchable text including tables,
notes and generated fields, and clickable link regions using the same destinations
as PDF. Its internal links target pages. Reflowable output can serialize block
apparatus alternatives; paginated output currently preserves the imported main
reading for such alternatives and reports the unsupported layout, while keeping
all readings in the native publication.

## Publication review and proof baselines

**Review and proofs** in the Publication panel provides tracking, anchored comments,
named proof capture/approval, proof comparison and revised-manuscript comparison.
Tracking belongs to the publication: switching chapters or editions keeps the same
setting and author. Author names are supplied explicitly; an unset author is
recorded as `Unattributed`. Each inserted/deleted run receives a persistent review
identity. Notes and table cells, including nested text, use the same operation.
The Story Editor and publication note editor honor tracking too.

| Command | Parameters and behavior |
|---|---|
| `changes.track` | `{on?, author?}` sets publication-wide tracking and attribution. |
| `changes.list` | Returns pending ranges with identity, component, content, story, nested path, byte range, kind, text and author. |
| `changes.accept`, `changes.reject` | `{id}` resolves a persistent change across its fragments; the legacy `{story,start,path?}` address is also accepted. |
| `changes.acceptAll`, `changes.rejectAll` | `{story?}` resolves the whole publication, or one active-component story, in one undo step. |
| `publication.review.resolve` | `{ids:[id],accept:bool}` resolves a selected set atomically. Unknown/resolved IDs fail without an edit. |
| `publication.review.list` | Returns tracking settings, ranges, attribution/status records, comments and proof summaries without composing. |
| `publication.comment.add` | `{text,author?,story?,start?,end?,path?}` anchors a comment to authored text; defaults to the current selection. |
| `publication.comment.edit` | `{id,text?,resolved?}` updates the comment while retaining its original author, quote and anchors. |
| `publication.review.goTo` | `{id}` selects the change/comment passage and reports its component/page. Nested passages open in the editable review window. Removed passages report an explicit unresolved error. |
| `publication.review.replace` | `{id,start,end,text}` edits the review record's text leaf, honoring tracking. |
| `publication.proof.create` | `{name,author?,edition?}` captures a uniquely named, immutable baseline. |
| `publication.proof.approve` | `{id,author?,note?}` records a separate approval. An existing approval cannot be overwritten. |
| `publication.proof.compare` | `{before,after?}` compares two baselines; omitted `after` composes the current edition on a candidate without changing the document. |
| `publication.proof.goTo` | `{component,story?,start?,item?,page?}` navigates a comparison row in the current document. |

Comments retain a quoted passage when an anchor is removed. They become unresolved
instead of attaching to unrelated nearby text. Proofs retain canonical content,
component order, semantic/index definitions, edition settings, line and page
geometry, nested composition fingerprints, figure positions, font names, face
indices, variable-font coordinates and byte fingerprints. Font fingerprints use
FNV-1a with byte length to identify changed inputs; they are not authentication
signatures. Proofs do not embed executable engines or a second copy of font files.
The report distinguishes content, formatting, figure changes/movement, pagination,
reference output/destination resolution and removed index passages. Added comment
anchors are excluded from text and appearance differences. Reference rows navigate
to their source chapter and passage; destination changes navigate to the destination.
It can also report metadata, edition,
component-order and font-input changes. Overset or unresolved composition remains
visible in the proof diagnostics. Limits are 32 baselines, 100,000 changes/comments,
256-byte author/proof names and 64 KiB comment text/quoted passages.

`publication.manuscript.preview` accepts `{path | base64 + name, story?, tei?,
resources?, styleMap?, styleConflicts?}`. It uses the existing text importers in an
isolated candidate and compares rich paragraphs, including their nested notes,
tables and illustrations. Actual TEI `xml:id` values take precedence; generated
navigation labels are not treated as source identities. Unique text and positional
matches are explicitly uncertain. The preview does not change content or undo.

`publication.manuscript.apply {preview,rows:[row],reviewed?:[row],autoflow?:true}`
applies only selected rows. Every uncertain selected row must also appear in
`reviewed`. In the dialog, selecting such a row approves the displayed pairing.
Selected moves follow revised source order. Surviving source anchors keep their
identities; comments/index anchors in surviving text are aligned to that text.
Resolve pending tracked changes in affected paragraphs before applying a revised
manuscript. A stale preview fails and must be created again. Revisions, styles,
assets, source attachments and added pages form one undo transaction. Original TEI
bytes remain unchanged, and the revised XML is preserved separately. References
to unselected revised content may remain unresolved and appear in export reports.

Browser file delivery retains the intended publication, chapter, edition and story;
if the publication changes while the picker is open, delivery is rejected with an
explanation. The TEI options dialog also provides resource mapping before comparing.
Review and proof data persist in version 3 archives. Earlier version 3 publications
without review records acquire identities for legacy tracked text on load; a present
but inconsistent review record is rejected.

## Incremental pagination and layout evidence

The composition cache identifies stories by edition and chapter as well as their
local story ID. Opening another chapter with the same local IDs cannot reuse its
neighbour's text. Unchanged projections compare their actual content, styles,
frame geometry and resolved fields before reusing composed output.

For prose, verse and generated fields, the cache records paragraph checkpoints
before vertical justification. An edit records old/new byte ranges and restarts
before the changed paragraph and its preceding keep chain. The checkpoint retains
the frame/column cursor, pending spacing, numbering, keep decisions and prior
frame output. Later output is reused only when the unchanged paragraph suffix,
frame output and continuation state agree. Byte offsets and paragraph indices in
reused glyphs are remapped after insertion/deletion. Appending frames can resume
previously overset content; real pagination changes continue downstream.

Notes (including gathered streams and apparatus), tables, anchored objects,
legacy references, text variables and source stories over 8 MiB use full composition, with a named fallback
reason in the report. This preserves their continuation and numbering behavior;
it does not claim incremental speedups for those stories. External wrapping items
are included in frame geometry. Publication running-head fields include their
resolved values for every page in the dependency comparison. A global style
change restarts every story that uses that style collection.

Reference evaluation checks query membership and the exact template inputs:
titles, numbering, metadata, reusable values and required destination pages.
Unchanged values reuse rendered fields. Membership or page changes invalidate
only matching field inputs; updated field values then invalidate their paragraphs.
Live updates, explicit publication updates, automatic page growth and exports
share the session's composition cache.

| Command | Parameters and result |
| --- | --- |
| `publication.layout.metrics` | `{reset?:bool}` returns session counters, native elapsed seconds, estimated cached bytes and the last 128 composition events. Each event names its chapter/story, changed byte ranges, restart/convergence paragraphs, recomposed paragraphs/frames and fallback reason. Reset clears counters, retaining valid cached layouts. |
| `publication.layout.verify` | `{}` compares all placed stories in the active edition with full composition, including glyph placement, rich generated text, destinations, notes, cells and overset. A current edition also receives an independent fixed-point reference evaluation with an empty cache. Returns checked story/frame counts; mismatches are explicit errors. Deferred stale editions report `referencesChecked:false` until updated. |

Open **Publication → Layout work and verification** for the same counters and an
optional full comparison. Verification can be expensive on a large publication;
it leaves authored state, selection and undo unchanged. Headless CLI/MCP and the
running application's control channel expose the same two engine commands.

Paragraph counters measure main-story composition attempts (including retries on
the checkpoint path); fallback reports count the full outer story, without
claiming to count nested cell/note shaping or internal annotation convergence.
Frame counters count touched frames, not glyph operations. Elapsed time measures
cache-miss composition and dependency preparation, and excludes full-oracle work;
it is `null` in WASM. Browser callers may time command completion themselves.
Memory values estimate retained output/checkpoints and reference payload, excluding
allocator overhead, font storage and other application state. They are not RSS.
Composition entries are bounded to 512 MiB of estimated payload/4,096 entries,
previous compositions to 64 MiB/64 entries, and rendered fields to 64 MiB/100,000
entries. Eviction affects speed, not saved content.

The authored generator in `crates/engine/examples/publication_scale.rs` exercises
900 pages and 3,600 references, local edits, chapter insertion, a real pagination
cascade, global style synchronization, save/reopen and complete PDF export. Each
numbered section starts a frame; its body flows normally and can cross pages.
Free-flowing text, Unicode shifts, keep rules, lists, columns, wrapping and note
fallbacks also have smaller regression fixtures. To reproduce, choose a new path:

```sh
cargo run -p designcraft-engine --example publication_scale -- target/atlas-check-1
```

The example refuses an existing output directory and records per-stage work,
wall time and full-composition comparisons in `receipt.json`. On Linux,
`/usr/bin/time -v` around the built example measures process peak RSS separately.
