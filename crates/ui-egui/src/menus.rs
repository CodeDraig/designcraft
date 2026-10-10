//! The menu bar tree (InDesign order), UI-only commands, keyboard shortcuts and Quick Apply.

mod files;
mod view;
mod workspace;
use files::{download_copy, download_document, export_bytes, export_idml, export_pdf, export_png};
mod shortcuts;
use shortcuts::parse_shortcut;
pub use shortcuts::{default_shortcut, effective_shortcuts, shortcut_of, shortcut_string, shortcut_text, shortcuts};
mod tree;
pub use tree::{Item, all_menu_items, menu_tree};
mod bar;
pub use bar::{menu_bar, menu_key};
mod palette;
pub use palette::{QuickItem, palette, quick_apply_items};

use serde_json::{Value, json};

use crate::{DesignApp, ScreenMode};

/// UI commands: (id, label, shortcut, params).
pub const UI_COMMANDS: &[(&str, &str, Option<&str>, &str)] = &[
    ("app.newDocumentDialog", "New Document…", Some("Cmd+N"), "{}"),
    ("app.openDialog", "Open…", Some("Cmd+O"), "{}"),
    ("app.close", "Close", Some("Cmd+W"), "{index?} — asks before closing a document with unsaved changes"),
    ("app.placeDialog", "Place…", Some("Cmd+D"), "{}"),
    ("app.saveDialog", "Save As…", Some("Cmd+Shift+S"), "{}"),
    ("app.save", "Save", Some("Cmd+S"), "{}"),
    ("app.saveCopyDialog", "Save a Copy…", Some("Cmd+Alt+S"), "{} — choose where to write a copy (the document stays as it is)"),
    ("app.loadSwatches", "Load Swatches…", None, "{} — pick a swatch exchange (.ase) file"),
    ("app.packageDialog", "Package…", Some("Cmd+Alt+Shift+P"), "{} — choose the package folder to create (file.package)"),
    ("app.saveSwatches", "Save Swatches for Exchange…", None, "{path?} — write the colour swatches as .ase"),
    ("app.exportPng", "Export Page as PNG…", Some("Cmd+E"), "{}"),
    ("app.exportIdml", "Export IDML…", None, "{path?} — InDesign Markup (IDML) package"),
    ("app.exportEpub", "Export EPUB…", None, "{path?, edition?, scope?, strict?, cover?} — EPUB 3; no parameters opens publication output choices"),
    (
        "app.exportInteractivePdf",
        "Export PDF (Interactive)…",
        None,
        "{path?, fullScreen?, advanceSeconds?} — single pages fitted, bookmarks panel open, buttons, forms, transitions",
    ),
    ("app.exportFixedEpub", "Export EPUB (Fixed Layout)…", None, "{path?, edition?, scope?, strict?} — pre-paginated EPUB 3"),
    ("app.exportHtml", "Export HTML…", None, "{path?, edition?, scope?, strict?} — one self-contained web page"),
    ("app.exportXml", "Export XML…", None, "{path?} — the tagged content"),
    ("app.printBooklet", "Print Booklet…", None, "{path?} — saddle-stitched printer spreads as PDF"),
    ("app.qrCode", "Generate QR Code…", None, "{} — the QR Code dialog (object.qrCode does the work)"),
    ("app.userDictionary", "User Dictionary…", None, "{} — hyphenation exceptions (hyphenation.* commands)"),
    ("app.fittingOptions", "Frame Fitting Options…", None, "{} — the dialog for object.fittingOptions"),
    ("app.menus", "Menus…", None, "{} — show or hide menu items"),
    ("window.hideMenuItem", "Hide Menu Item", None, "{item: \"Menu/Label\", hidden?: bool}"),
    ("edit.dynamicSpelling", "Dynamic Spelling", None, "{on?: bool} — underline misspelled words on the canvas"),
    (
        "app.language",
        "Interface Language",
        None,
        "{lang: \"\"|de|fr|es|ja|zh|ar|pt-br} — menus and panel names (the macOS menu bar follows on the next launch)",
    ),
    (
        "app.flattener",
        "Transparency Flattener Presets",
        None,
        "{preset: \"\"|high|medium|low} — PDF export rasterises spreads with transparency at 300 / 150 / 72 ppi",
    ),
    ("app.graphicCell", "Convert Cell to Graphic Cell…", None, "{} — pick an image for the target table cell"),
    ("app.placeAndLink", "Place and Link", None, "{} — a linked copy of the selected frame's story, beside it"),
    ("app.teiImport", "Import TEI Book…", None, "{path | base64 + name, purpose?: open|place} — review TEI reading and resource options"),
    ("app.teiReport", "TEI Import Report…", None, "{} — inspect preserved sources, notices and navigation"),
    ("app.teiSource", "Save Original TEI…", None, "{source?, path?} — retrieve the unchanged imported XML"),
    (
        "app.publicationDialog",
        "Publication…",
        None,
        "{mode?: new|component|edition|editionSettings|export|settings|styles|numbering|mark|reference|generated|template|scan|index|indexTopic|indexMark|indexPreview|noteStream|notePolicy|noteInsert|notes|apparatus|apparatusRecover|review|comment|proof|proofCompare|manuscript|layout, stream?, index?, id?, topic?, component?, name?, source?, sourceEdition?, edition?, components?, path? | base64 + name} — publication settings, references, review, proofs, manuscript comparison and layout verification",
    ),
    ("app.publicationImport", "Import Publication Chapter…", None, "{} — add a standalone document to the active publication"),
    (
        "app.publicationManuscript",
        "Compare Revised Manuscript…",
        None,
        "{} — choose a revised manuscript for explicit review before applying to the active chapter",
    ),
    ("app.publicationBook", "Convert Book to Publication…", None, "{} — copy .dcbook chapters into a new publication"),
    ("app.browserRecovery", "Recover Browser Documents", None, "{} — reopen preserved browser snapshots as unsaved documents"),
    ("app.placeWithOptions", "Place with Import Options…", Some("Cmd+Shift+D"), "{} — Word/RTF style mapping before placing"),
    ("app.exportText", "Export Text…", None, "{path?} — the story being edited, as Text Only (.txt) or Rich Text Format (.rtf)"),
    ("app.exportPdf", "Export PDF…", None, "{path?, …file.exportPdf options} — asks for a path when none is given"),
    ("app.palette", "Quick Apply…", Some("Cmd+Return"), "{} — search styles and commands"),
    ("app.preferences", "Preferences…", Some("Cmd+K"), "{}"),
    ("window.uiScale", "UI Scaling", None, "{scale: 0.5–3 (1 = 100%)} — the size of the whole interface"),
    ("app.keyboardShortcuts", "Keyboard Shortcuts…", None, "{} — view and change the shortcut of any command"),
    ("window.richBlack", "Appearance of Black", None, "{on: bool} — show 100% black as rich black on screen (off: accurately)"),
    ("window.setShortcut", "Set Shortcut", None, "{id, shortcut: \"Cmd+Alt+J\" | \"\" (none) | null (default)} → {shortcut, conflicts: [ids]}"),
    ("window.resetShortcuts", "Reset Shortcuts", None, "{} — every command back to its default shortcut"),
    ("app.findChange", "Find/Change…", Some("Cmd+F"), "{}"),
    ("app.insertTableDialog", "Create Table…", None, "{} — Insert Table dialog (body/header/footer rows, columns)"),
    ("app.footnoteOptionsDialog", "Document Footnote Options…", None, "{} — numbering, formatting and layout of footnotes"),
    ("app.rubyDialog", "Ruby…", None, "{} — the reading set over the selected text"),
    ("app.findFontDialog", "Find/Replace Font…", None, "{} — fonts used (missing ones flagged) and replacing them"),
    ("app.insertXrefDialog", "Insert Cross-Reference…", None, "{} — New Cross-Reference dialog (paragraph or text anchor, format)"),
    ("app.deleteAllGuides", "Delete All Guides on Spread", None, "{} — the spread in view"),
    ("app.deletePage", "Delete Page", None, "{} — deletes the page in view"),
    ("app.duplicateSpread", "Duplicate Spread", None, "{} — duplicates the spread in view (pages and items)"),
    ("app.tablePanel", "Table Panel", Some("Shift+F9"), "{}"),
    ("app.storyEditor", "Edit in Story Editor", Some("Cmd+Y"), "{story?, note?: publication note id}"),
    ("view.zoomIn", "Zoom In", Some("Cmd+="), "{}"),
    ("view.zoomOut", "Zoom Out", Some("Cmd+-"), "{}"),
    ("view.fitPage", "Fit Page in Window", Some("Cmd+0"), "{}"),
    ("view.fitSelection", "Fit Selection in Window", Some("Cmd+Alt+="), "{}"),
    ("view.fitSpread", "Fit Spread in Window", Some("Cmd+Alt+0"), "{}"),
    ("view.actualSize", "Actual Size", Some("Cmd+1"), "{}"),
    ("view.entirePasteboard", "Entire Pasteboard", Some("Cmd+Alt+Shift+0"), "{}"),
    ("view.zoom", "Zoom To", None, "{zoom: 1.0 = 100%}"),
    ("view.screenMode", "Screen Mode", None, "{mode: normal|preview|bleed|slug|presentation}"),
    ("view.togglePreview", "Toggle Normal/Preview", Some("W"), "{}"),
    ("view.frameEdges", "Show/Hide Frame Edges", Some("Cmd+H"), "{}"),
    ("view.rulers", "Show/Hide Rulers", Some("Cmd+R"), "{}"),
    ("view.guides", "Show/Hide Guides", Some("Cmd+;"), "{}"),
    ("view.snapToGuides", "Snap to Guides", Some("Cmd+Shift+;"), "{}"),
    ("view.snapToDocumentGrid", "Snap to Document Grid", None, "{}"),
    ("view.smartGuides", "Smart Guides", None, "{}"),
    (
        "view.snapPreferences",
        "Smart Guide Options",
        None,
        "{alignEdges?, alignCenters?, smartDimensions?, smartSpacing?: bool, zone?: px} — what smart guides snap to and how close; returns the current values",
    ),
    ("view.baselineGrid", "Show/Hide Baseline Grid", Some("Cmd+Alt+'"), "{}"),
    ("view.textThreads", "Show/Hide Text Threads", Some("Cmd+Alt+Y"), "{}"),
    ("view.hiddenCharacters", "Show/Hide Hidden Characters", Some("Cmd+Alt+I"), "{}"),
    ("view.taggedFrames", "Show/Hide Tagged Frames", None, "{} — XML-tagged frames outlined in their tag colour"),
    ("window.hidePanels", "Show/Hide Panels", Some("Tab"), "{} — every panel, the Tools panel included"),
    ("window.hidePanelsExceptTools", "Show/Hide Panels Except Tools", Some("Shift+Tab"), "{}"),
    ("window.nextDocument", "Next Document", Some("Cmd+F6"), "{}"),
    ("window.previousDocument", "Previous Document", Some("Cmd+Shift+F6"), "{}"),
    ("app.layerOptions", "Object Layer Options…", None, "{} — show or hide the layers of the selected placed PDF"),
    ("view.tagMarkers", "Show Tag Markers", None, "{on?: bool} — brackets around inline-tagged text"),
    ("view.flattenerPreview", "Flattener Preview", None, "{on?: bool} — highlight objects that involve transparency"),
    ("view.rotateSpread", "Rotate Spread", None, "{angle: 90 (clockwise) | -90 | 180 | 0 (clear)} — turn the view in quarter turns"),
    ("view.proofColors", "Proof Colors", None, "{on?: bool} — simulate the proof target on screen"),
    (
        "view.proofSetup",
        "Proof Setup",
        None,
        "{target?: workingCmyk|srgb|legacyMacRgb|monitorRgb|protanopia|deuteranopia|cmyk:<profile>, intent?, simulatePaper?: bool|\"toggle\"}",
    ),
    ("app.colorSettings", "Color Settings…", None, "{} — the working spaces, intent and black-point compensation"),
    ("view.separations", "Separations Preview", None, "{plate?: cyan|magenta|yellow|black|null, inkLimit?: percent|null}"),
    ("view.overprintPreview", "Overprint Preview", Some("Cmd+Alt+Shift+Y"), "{} — show how overprinting inks mix"),
    ("view.fastDisplay", "Fast Display", Some("Cmd+Alt+Shift+Z"), "{} — placed graphics as grey boxes, no effects"),
    ("view.typicalDisplay", "Typical Display", Some("Cmd+Alt+Z"), "{} — low-resolution image proxies"),
    ("view.highQualityDisplay", "High Quality Display", Some("Cmd+Alt+H"), "{} — full-resolution images"),
    ("view.goToPage", "Go to Page…", Some("Cmd+J"), "{page: number (position) | \"name\" (section page name, or \"+n\" for a position)}"),
    (
        "window.panel",
        "Show Panel",
        None,
        "{panel: properties|pages|layers|swatches|paragraphStyles|characterStyles|stroke|character|paragraph|textWrap|links|table}",
    ),
    ("window.floatPanel", "Float Panel", None, "{panel, x?, y?} — the panel in its own movable window"),
    ("window.dockPanel", "Dock Panel", None, "{panel} — back into the dock's icon column"),
    ("window.controlBar", "Control", Some("Cmd+Alt+6"), "{}"),
    ("window.split", "Split Window", None, "{on?: bool} — two views of the document side by side, each with its own zoom and scroll"),
    ("window.newWindow", "New Window", None, "{on?: bool} — another view of the active document in its own window"),
    ("window.taskBar", "Contextual Task Bar", None, "{}"),
    ("help.discord", "Join the ArtCraft Discord…", None, "{} — opens https://discord.gg/artcraft in the browser"),
    ("help.appPage", "DesignCraft Website…", None, "{} — opens https://getartcraft.com/apps/designcraft"),
    ("help.github", "DesignCraft on GitHub…", None, "{} — opens https://github.com/storytold/designcraft"),
    ("help.issues", "Report an Issue…", None, "{} — opens the GitHub issue tracker"),
    ("help.website", "ArtCraft Website…", None, "{} — opens https://getartcraft.com"),
    ("help.app", "ArtCraft App Page…", None, "{app} — opens https://getartcraft.com/apps/{app} (e.g. photocraft)"),
    (
        "help.about",
        "About DesignCraft",
        None,
        "{open?: true, tab?: \"about\"|\"contributors\"|\"models\"} — the About window: splash with community links, contributor and model credits (open: false closes it)",
    ),
    ("window.toolsDoubleColumn", "Tools: Double Column", None, "{}"),
    (
        "window.workspace",
        "Workspace",
        None,
        "{name: Essentials|Advanced|Book|Digital Publishing|Interactive for PDF|Printing and Proofing|Typography}",
    ),
    ("window.newWorkspace", "New Workspace…", None, "{name} — saves the current bars and panel arrangement"),
    ("window.deleteWorkspace", "Delete Workspace…", None, "{name}"),
    ("window.resetWorkspace", "Reset Workspace", None, "{} — back to the current workspace as saved (or its defaults)"),
    ("window.brightness", "Interface Color Theme", None, "{brightness: dark|mediumDark|mediumLight|light|highContrast}"),
];

/// Menu bar: (menu, entries). Entries: `cmd:<id>`, `ui:<id>`, `-` separator, `>Submenu` … `<`.
pub const MENUS: &[(&str, &[&str])] = &[
    (
        "File",
        &[
            "ui:app.newDocumentDialog",
            "ui:app.publicationDialog|New Publication…",
            "cmd:publication.convert",
            "ui:app.publicationBook",
            "ui:app.openDialog",
            "ui:app.browserRecovery",
            "-",
            "ui:app.close",
            "ui:app.save",
            "ui:app.saveDialog",
            "ui:app.saveCopyDialog",
            "cmd:file.revert",
            "-",
            "ui:app.placeDialog",
            "ui:app.placeWithOptions",
            "ui:app.teiImport",
            "ui:app.teiReport",
            "ui:app.teiSource",
            "-",
            "ui:app.exportPdf",
            "ui:app.packageDialog",
            "ui:app.printBooklet",
            "ui:app.exportPng",
            "ui:app.exportIdml",
            "ui:app.exportEpub",
            "ui:app.exportFixedEpub",
            "ui:app.exportInteractivePdf",
            "ui:app.exportHtml",
            "ui:app.exportXml",
            "ui:app.exportText",
            ">Export",
            "cmd:snippet.export",
            "<",
            "-",
            "cmd:layout.documentSetup",
        ],
    ),
    (
        "Edit",
        &[
            "cmd:edit.undo",
            "cmd:edit.redo",
            "-",
            "cmd:edit.cut",
            "cmd:edit.copy",
            "cmd:edit.paste",
            "cmd:edit.pasteWithoutFormatting",
            "cmd:edit.pasteInto",
            "cmd:edit.pasteInPlace",
            "cmd:edit.clear",
            "-",
            "cmd:edit.duplicate",
            "ui:app.placeAndLink",
            "cmd:edit.stepAndRepeat",
            "-",
            "cmd:edit.selectAll",
            "cmd:edit.deselectAll",
            "-",
            "ui:app.findChange",
            "cmd:find.next",
            "ui:app.storyEditor",
            "-",
            ">Spelling",
            "cmd:spelling.check",
            "ui:edit.dynamicSpelling",
            "cmd:spelling.addWord",
            "ui:app.userDictionary",
            "<",
            ">Transparency Blend Space",
            "cmd:edit.transparencyBlendSpace|Document RGB|{\"space\": \"rgb\"}",
            "cmd:edit.transparencyBlendSpace|Document CMYK|{\"space\": \"cmyk\"}",
            "<",
            "ui:app.colorSettings",
            ">Interface Language",
            "ui:app.language|English|{\"lang\": \"\"}",
            "ui:app.language|Deutsch|{\"lang\": \"de\"}",
            "ui:app.language|Français|{\"lang\": \"fr\"}",
            "ui:app.language|Español|{\"lang\": \"es\"}",
            "ui:app.language|日本語|{\"lang\": \"ja\"}",
            "ui:app.language|简体中文|{\"lang\": \"zh\"}",
            "ui:app.language|العربية|{\"lang\": \"ar\"}",
            "ui:app.language|Português (Brasil)|{\"lang\": \"pt-br\"}",
            "<",
            ">Transparency Flattener Presets",
            "ui:app.flattener|None (keep transparency)|{\"preset\": \"\"}",
            "ui:app.flattener|High Resolution|{\"preset\": \"high\"}",
            "ui:app.flattener|Medium Resolution|{\"preset\": \"medium\"}",
            "ui:app.flattener|Low Resolution|{\"preset\": \"low\"}",
            "<",
            "-",
            "ui:app.palette",
            "ui:app.keyboardShortcuts",
            "ui:app.menus",
            "ui:app.preferences",
        ],
    ),
    (
        "Layout",
        &[
            ">Pages",
            "cmd:layout.pages.insert",
            "cmd:layout.pages.move",
            "ui:app.duplicateSpread",
            "ui:app.deletePage",
            "-",
            "cmd:layout.pages.applyParent",
            "cmd:layout.parents.new",
            "<",
            "cmd:layout.marginsAndColumns",
            "cmd:layout.createGuides",
            "-",
            "ui:view.goToPage",
            "-",
            "cmd:layout.section",
            "cmd:toc.generate",
            "cmd:toc.update",
            ">Index",
            "cmd:index.addReference",
            "cmd:index.generate",
            "cmd:index.update",
            "<",
        ],
    ),
    (
        "Type",
        &[
            "ui:app.findFontDialog",
            "-",
            "cmd:type.createOutlines",
            ">Bulleted and Numbered Lists",
            "cmd:list.define",
            "<",
            ">Type on a Path",
            "cmd:type.pathOptions",
            "cmd:type.pathOptions|Delete Type from Path|{\"delete\": true}",
            "<",
            "ui:window.panel|Glyphs|{\"panel\": \"glyphs\"}",
            "cmd:type.fillWithPlaceholder",
            ">Insert Special Character",
            ">Symbols",
            "cmd:text.insert|Bullet Character|{\"text\": \"\u{2022}\", \"raw\": true}",
            "cmd:text.insert|Copyright Symbol|{\"text\": \"\u{a9}\", \"raw\": true}",
            "cmd:text.insert|Ellipsis|{\"text\": \"\u{2026}\", \"raw\": true}",
            "cmd:text.insert|Paragraph Symbol|{\"text\": \"\u{b6}\", \"raw\": true}",
            "cmd:text.insert|Registered Trademark Symbol|{\"text\": \"\u{ae}\", \"raw\": true}",
            "cmd:text.insert|Section Symbol|{\"text\": \"\u{a7}\", \"raw\": true}",
            "cmd:text.insert|Trademark Symbol|{\"text\": \"\u{2122}\", \"raw\": true}",
            "<",
            ">Markers",
            "cmd:text.insert|Current Page Number|{\"text\": \"\u{e000}\", \"raw\": true}",
            "cmd:text.insert|Next Page Number|{\"text\": \"\u{e007}\", \"raw\": true}",
            "cmd:text.insert|Previous Page Number|{\"text\": \"\u{e008}\", \"raw\": true}",
            "cmd:text.insert|Section Marker|{\"text\": \"\u{e001}\", \"raw\": true}",
            "<",
            ">Hyphens and Dashes",
            "cmd:text.insert|Em Dash|{\"text\": \"\u{2014}\", \"raw\": true}",
            "cmd:text.insert|En Dash|{\"text\": \"\u{2013}\", \"raw\": true}",
            "cmd:text.insert|Discretionary Hyphen|{\"text\": \"\u{ad}\", \"raw\": true}",
            "cmd:text.insert|Nonbreaking Hyphen|{\"text\": \"\u{2011}\", \"raw\": true}",
            "<",
            ">Quotation Marks",
            "cmd:text.insert|Double Left Quotation Marks|{\"text\": \"\u{201c}\", \"raw\": true}",
            "cmd:text.insert|Double Right Quotation Marks|{\"text\": \"\u{201d}\", \"raw\": true}",
            "cmd:text.insert|Single Left Quotation Mark|{\"text\": \"\u{2018}\", \"raw\": true}",
            "cmd:text.insert|Single Right Quotation Mark|{\"text\": \"\u{2019}\", \"raw\": true}",
            "cmd:text.insert|Straight Double Quotation Marks|{\"text\": \"\\\"\", \"raw\": true}",
            "cmd:text.insert|Straight Single Quotation Mark (Apostrophe)|{\"text\": \"'\", \"raw\": true}",
            "<",
            ">Other",
            "cmd:text.insert|Tab|{\"text\": \"\\t\", \"raw\": true}",
            "cmd:text.insert|Right Indent Tab|{\"text\": \"\u{e006}\", \"raw\": true}",
            "cmd:text.insert|Indent to Here|{\"text\": \"\u{e005}\", \"raw\": true}",
            "<",
            "<",
            ">Insert White Space",
            "cmd:text.insert|Em Space|{\"text\": \"\u{2003}\", \"raw\": true}",
            "cmd:text.insert|En Space|{\"text\": \"\u{2002}\", \"raw\": true}",
            "cmd:text.insert|Nonbreaking Space|{\"text\": \"\u{a0}\", \"raw\": true}",
            "cmd:text.insert|Hair Space|{\"text\": \"\u{200a}\", \"raw\": true}",
            "cmd:text.insert|Sixth Space|{\"text\": \"\u{2006}\", \"raw\": true}",
            "cmd:text.insert|Thin Space|{\"text\": \"\u{2009}\", \"raw\": true}",
            "cmd:text.insert|Quarter Space|{\"text\": \"\u{2005}\", \"raw\": true}",
            "cmd:text.insert|Third Space|{\"text\": \"\u{2004}\", \"raw\": true}",
            "cmd:text.insert|Punctuation Space|{\"text\": \"\u{2008}\", \"raw\": true}",
            "cmd:text.insert|Figure Space|{\"text\": \"\u{2007}\", \"raw\": true}",
            "cmd:text.insert|Flush Space|{\"text\": \"\u{2001}\", \"raw\": true}",
            "<",
            ">Insert Break Character",
            "cmd:text.insert|Column Break|{\"text\": \"\u{e002}\", \"raw\": true}",
            "cmd:text.insert|Frame Break|{\"text\": \"\u{e003}\", \"raw\": true}",
            "cmd:text.insert|Page Break|{\"text\": \"\u{e004}\", \"raw\": true}",
            "cmd:text.insert|Paragraph Return|{\"text\": \"\\n\", \"raw\": true}",
            "cmd:text.insert|Forced Line Break|{\"text\": \"\u{2028}\", \"raw\": true}",
            "<",
            "-",
            ">Change Case",
            "cmd:type.changeCase|UPPERCASE|{\"case\": \"upper\"}",
            "cmd:type.changeCase|lowercase|{\"case\": \"lower\"}",
            "cmd:type.changeCase|Title Case|{\"case\": \"title\"}",
            "cmd:type.changeCase|Sentence case|{\"case\": \"sentence\"}",
            "<",
            "-",
            "cmd:type.alignLeft",
            "cmd:type.alignCenter",
            "cmd:type.alignRight",
            "cmd:type.justify",
            "-",
            "cmd:type.bold",
            "cmd:type.italic",
            "cmd:type.underline",
            "cmd:type.allCaps",
            "-",
            "cmd:type.sizeUp",
            "cmd:type.sizeDown",
            "-",
            "ui:view.hiddenCharacters",
            "ui:view.taggedFrames",
            ">Rotate Spread",
            "ui:view.rotateSpread|90° CW|{\"angle\": 90}",
            "ui:view.rotateSpread|90° CCW|{\"angle\": -90}",
            "ui:view.rotateSpread|180°|{\"angle\": 180}",
            "ui:view.rotateSpread|Clear Rotation|{\"angle\": 0}",
            "<",
            ">Proof Setup",
            "ui:view.proofSetup|Working CMYK|{\"target\": \"workingCmyk\"}",
            "ui:view.proofSetup|Internet Standard RGB (sRGB)|{\"target\": \"srgb\"}",
            "ui:view.proofSetup|Legacy Macintosh RGB|{\"target\": \"legacyMacRgb\"}",
            "ui:view.proofSetup|Monitor RGB|{\"target\": \"monitorRgb\"}",
            "ui:view.proofSetup|Color Blindness – Protanopia|{\"target\": \"protanopia\"}",
            "ui:view.proofSetup|Color Blindness – Deuteranopia|{\"target\": \"deuteranopia\"}",
            "-",
            "ui:view.proofSetup|Simulate Paper Color|{\"simulatePaper\": \"toggle\"}",
            "<",
            "ui:view.proofColors",
            "ui:view.flattenerPreview",
            "ui:view.tagMarkers",
            ">Separations Preview",
            "ui:view.separations|Off|{\"plate\": null, \"inkLimit\": null}",
            "ui:view.separations|Cyan|{\"plate\": \"cyan\"}",
            "ui:view.separations|Magenta|{\"plate\": \"magenta\"}",
            "ui:view.separations|Yellow|{\"plate\": \"yellow\"}",
            "ui:view.separations|Black|{\"plate\": \"black\"}",
            "ui:view.separations|Ink Limit 300%|{\"inkLimit\": 300}",
            "<",
            "-",
            ">Hyperlinks & Cross-References",
            "cmd:hyperlink.create",
            "cmd:anchor.create",
            "-",
            "ui:app.insertXrefDialog",
            "cmd:xref.defineFormat",
            "<",
            ">Text Variables",
            "cmd:variables.define",
            "cmd:variables.insert",
            "<",
            "-",
            "cmd:footnote.insert",
            "cmd:endnote.insert",
            "cmd:math.insert",
            ">Story Direction",
            "cmd:type.storyDirection|Horizontal|{\"vertical\": false}",
            "cmd:type.storyDirection|Vertical|{\"vertical\": true}",
            "<",
            "cmd:type.tateChuYoko",
            "ui:app.rubyDialog",
            "cmd:type.kenten",
            ">Track Changes",
            "cmd:changes.track",
            "cmd:changes.acceptAll",
            "cmd:changes.rejectAll",
            "<",
            "ui:app.footnoteOptionsDialog",
            "cmd:footnote.goToReference",
        ],
    ),
    (
        "Object",
        &[
            ">Transform",
            "cmd:transform.move",
            "cmd:transform.scale",
            "cmd:transform.rotate",
            "cmd:transform.shear",
            "-",
            "cmd:transform.flip",
            "-",
            "cmd:transform.clear",
            "<",
            ">Transform Again",
            "cmd:transform.again",
            "cmd:transform.again|Transform Again Individually|{\"individually\": true}",
            "cmd:transform.again|Transform Sequence Again|{\"sequence\": true}",
            "cmd:transform.again|Transform Sequence Again Individually|{\"sequence\": true, \"individually\": true}",
            "<",
            ">Paths",
            "cmd:object.makeCompoundPath",
            "cmd:object.releaseCompoundPath",
            "<",
            ">Pathfinder",
            "cmd:object.pathfinder|Add|{\"op\": \"add\"}",
            "cmd:object.pathfinder|Subtract|{\"op\": \"subtract\"}",
            "cmd:object.pathfinder|Intersect|{\"op\": \"intersect\"}",
            "cmd:object.pathfinder|Exclude Overlap|{\"op\": \"exclude\"}",
            "cmd:object.pathfinder|Minus Back|{\"op\": \"minusBack\"}",
            "<",
            ">Arrange",
            "cmd:object.bringToFront",
            "cmd:object.bringForward",
            "cmd:object.sendBackward",
            "cmd:object.sendToBack",
            "<",
            "-",
            "cmd:object.group",
            "cmd:object.ungroup",
            "cmd:object.lock",
            "cmd:object.unlockAll",
            "cmd:object.hide",
            "cmd:object.showAll",
            "-",
            "cmd:object.textFrameOptions",
            "ui:app.layerOptions",
            "cmd:object.exportOptions",
            "cmd:object.primaryTextFrame",
            "cmd:object.cornerOptions",
            "-",
            ">Effects",
            "cmd:object.dropShadow",
            "cmd:object.innerShadow",
            "cmd:object.outerGlow",
            "cmd:object.innerGlow",
            "cmd:object.bevel",
            "cmd:object.satin",
            "cmd:object.feather",
            "cmd:object.directionalFeather",
            "<",
            ">Select",
            "cmd:select.firstAbove",
            "cmd:select.nextAbove",
            "cmd:select.nextBelow",
            "cmd:select.lastBelow",
            "-",
            "cmd:select.container",
            "cmd:select.content",
            "-",
            "cmd:select.previousInGroup",
            "cmd:select.nextInGroup",
            "<",
            ">Convert Shape",
            "cmd:object.convertShape|Rectangle|{\"to\": \"rectangle\"}",
            "cmd:object.convertShape|Rounded Rectangle|{\"to\": \"roundedRectangle\"}",
            "cmd:object.convertShape|Beveled Rectangle|{\"to\": \"beveledRectangle\"}",
            "cmd:object.convertShape|Inverse Rounded Rectangle|{\"to\": \"inverseRoundedRectangle\"}",
            "cmd:object.convertShape|Ellipse|{\"to\": \"ellipse\"}",
            "cmd:object.convertShape|Triangle|{\"to\": \"triangle\"}",
            "cmd:object.convertShape|Polygon|{\"to\": \"polygon\"}",
            "cmd:object.convertShape|Line|{\"to\": \"line\"}",
            "cmd:object.convertShape|Orthogonal Line|{\"to\": \"orthogonalLine\"}",
            "-",
            "cmd:object.convertShape|Open Path|{\"to\": \"openPath\"}",
            "cmd:object.convertShape|Closed Path|{\"to\": \"closedPath\"}",
            "<",
            "ui:app.qrCode",
            ">Clipping Path",
            "cmd:object.clippingPath|Alpha Channel|{\"type\": \"alpha\"}",
            "cmd:object.clippingPath|Detect Edges|{\"type\": \"edges\"}",
            "<",
            ">Captions",
            "cmd:object.caption",
            "cmd:object.caption|Generate Live Caption|{\"live\": true}",
            "<",
            ">Anchored Object",
            "cmd:anchored.options",
            "cmd:anchored.release",
            "<",
            ">Fitting",
            "cmd:object.fit|Fill Frame Proportionally|{\"mode\":\"fillProportionally\"}",
            "cmd:object.fit|Fit Content Proportionally|{\"mode\":\"fitProportionally\"}",
            "cmd:object.fit|Fit Frame to Content|{\"mode\":\"fitFrameToContent\"}",
            "cmd:object.fit|Fit Content to Frame|{\"mode\":\"fitContentToFrame\"}",
            "cmd:object.fit|Center Content|{\"mode\":\"centerContent\"}",
            "-",
            "ui:app.fittingOptions",
            "<",
            ">Content",
            "cmd:object.content|Graphic|{\"type\":\"graphic\"}",
            "cmd:object.content|Text|{\"type\":\"text\"}",
            "cmd:object.content|Unassigned|{\"type\":\"unassigned\"}",
            "<",
        ],
    ),
    (
        "Table",
        &[
            "ui:app.insertTableDialog",
            "cmd:table.convertFromText",
            "cmd:table.convertToText",
            "-",
            ">Insert",
            "cmd:table.insertRowAbove",
            "cmd:table.insertRowBelow",
            "cmd:table.insertColumnLeft",
            "cmd:table.insertColumnRight",
            "<",
            ">Delete",
            "cmd:table.deleteRow",
            "cmd:table.deleteColumn",
            "cmd:table.delete",
            "<",
            ">Select",
            "cmd:table.selectRow",
            "cmd:table.selectColumn",
            "cmd:table.selectTable",
            "<",
            "-",
            "cmd:table.merge",
            "cmd:table.unmerge",
            ">Convert Cell Type",
            "ui:app.graphicCell|Convert Cell to Graphic Cell…",
            "cmd:table.textCell",
            "<",
            "cmd:table.splitHorizontally",
            "cmd:table.splitVertically",
            "cmd:table.distributeColumns",
            "-",
            "ui:app.tablePanel",
        ],
    ),
    (
        "View",
        &[
            "ui:view.zoomIn",
            "ui:view.zoomOut",
            "ui:view.fitPage",
            "ui:view.fitSelection",
            "ui:view.fitSpread",
            "ui:view.actualSize",
            "ui:view.entirePasteboard",
            "-",
            "ui:view.overprintPreview",
            "ui:view.togglePreview",
            ">Display Performance",
            "ui:view.fastDisplay",
            "ui:view.typicalDisplay",
            "ui:view.highQualityDisplay",
            "<",
            "-",
            "ui:view.rulers",
            "ui:view.frameEdges",
            "ui:view.textThreads",
            "ui:view.hiddenCharacters",
            "-",
            ">Grids & Guides",
            "ui:view.guides",
            "ui:view.snapToGuides",
            "ui:view.snapToDocumentGrid",
            "ui:view.smartGuides",
            "ui:view.baselineGrid",
            "-",
            "ui:app.deleteAllGuides",
            "<",
        ],
    ),
    (
        "Window",
        &[
            ">Arrange",
            "ui:window.newWindow",
            "ui:window.split",
            "<",
            "-",
            "ui:window.hidePanels",
            "ui:window.hidePanelsExceptTools",
            "ui:window.nextDocument",
            "ui:window.previousDocument",
            "-",
            "ui:window.controlBar",
            "ui:window.taskBar",
            "ui:window.toolsDoubleColumn",
            "-",
            "ui:window.panel",
            "-",
            ">Type & Tables",
            "ui:window.panel|Conditional Text|{\"panel\": \"conditions\"}",
            "ui:window.panel|Notes|{\"panel\": \"notes\"}",
            "ui:window.panel|Glyphs|{\"panel\": \"glyphs\"}",
            "<",
            ">Interactive",
            "ui:window.panel|Hyperlinks|{\"panel\": \"hyperlinks\"}",
            "ui:window.panel|Bookmarks|{\"panel\": \"bookmarks\"}",
            "ui:window.panel|Articles|{\"panel\": \"articles\"}",
            "ui:window.panel|Object States|{\"panel\": \"states\"}",
            "ui:window.panel|Buttons and Forms|{\"panel\": \"buttons\"}",
            "ui:window.panel|Media|{\"panel\": \"media\"}",
            "ui:window.panel|Page Transitions|{\"panel\": \"transitions\"}",
            "ui:window.panel|Track Changes|{\"panel\": \"trackChanges\"}",
            "ui:window.panel|Scripts|{\"panel\": \"scripts\"}",
            "ui:window.panel|Liquid Layout|{\"panel\": \"liquid\"}",
            "ui:window.panel|Tags|{\"panel\": \"tags\"}",
            "ui:window.panel|Book|{\"panel\": \"book\"}",
            "ui:window.panel|Publication|{\"panel\": \"publication\"}",
            "<",
            ">Utilities",
            "ui:window.panel|Data Merge|{\"panel\":\"dataMerge\"}",
            "<",
            ">Output",
            "ui:window.panel|Attributes|{\"panel\": \"attributes\"}",
            "cmd:preflight.run",
            "<",
            "-",
            "ui:window.brightness",
        ],
    ),
    (
        "Help",
        &[
            "ui:help.discord",
            "-",
            "ui:help.appPage",
            "ui:help.github",
            "ui:help.issues",
            "ui:help.website",
            "-",
            "cmd:file.newSample",
            "-",
            "ui:help.about",
        ],
    ),
];

pub fn ui_label(id: &str) -> Option<(&'static str, Option<&'static str>)> {
    UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| (c.1, c.2))
}

/// Run a UI command; `None` if `id` isn't one.
pub fn run_ui(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if p.as_object().is_none_or(|p| p.is_empty()) && app.session.active().is_some_and(|s| s.doc.publication.is_some()) {
        let format = match id {
            "app.exportPdf" => Some("pdf"),
            "app.exportEpub" => Some("epub"),
            "app.exportFixedEpub" => Some("fixedEpub"),
            "app.exportHtml" => Some("html"),
            _ => None,
        };
        if let Some(format) = format {
            return Some(crate::dialogs::publication::open(app, &json!({"mode":"export","format":format})));
        }
    }
    if id.starts_with("view.") {
        return view::run(app, id, p);
    }
    if id.starts_with("window.") {
        return workspace::run(app, id, p);
    }
    let flag = |b: &mut bool| {
        *b = !*b;
        Ok(json!(*b))
    };
    Some(match id {
        "app.newDocumentDialog" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("newDocument", json!({})));
            Ok(Value::Null)
        }
        "app.openDialog" => app.pick_and_open("open"),
        "app.close" => {
            let index = match p.get("index") {
                Some(v) => match v.as_u64().and_then(|i| usize::try_from(i).ok()) {
                    Some(i) => Some(i),
                    None => return Some(Err("index must be a non-negative document index".into())),
                },
                None => app.session.active_index(),
            };
            let Some(index) = index else { return Some(Err("no active document".into())) };
            let Some(st) = app.session.documents().get(index) else { return Some(Err("no such document".into())) };
            if st.is_dirty() {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("closeDocument", json!({"uid": st.uid, "title": st.title(), "action": "save"})));
                Ok(json!({"confirmationRequired": true}))
            } else {
                app.run("file.close", json!({"index": index}))
            }
        }
        "app.teiImport" => {
            if p.get("path").is_some() || p.get("base64").is_some() {
                crate::dialogs::tei::open(app, p.clone(), p["purpose"].as_str().unwrap_or("open"))
            } else {
                app.pick_and_open("tei")
            }
        }
        "app.teiReport" => match app.run("tei.report", p.clone()) {
            Ok(reports) => {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("teiReport", json!({"reports":reports})));
                Ok(Value::Null)
            }
            Err(e) => Err(e),
        },
        "app.teiSource" => crate::dialogs::tei::save_source(app, p),
        "app.publicationDialog" => crate::dialogs::publication::open(app, p),
        "app.publicationImport" => app.pick_and_open("publicationAdd"),
        "app.publicationManuscript" => app.pick_and_open("manuscript"),
        "app.publicationBook" => app.pick_and_open("publicationBook"),
        "app.browserRecovery" => {
            if app.services.save_recovery.is_some() {
                app.pick_and_open("browserRecovery")
            } else {
                Err("browser recovery is available in the browser app".into())
            }
        }
        "app.placeDialog" => app.pick_and_open("place"),
        "app.placeWithOptions" => {
            // Word/RTF Import Options: style mapping and conflicts before the text lands.
            let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("place")) else { return Some(Ok(Value::Null)) };
            let l = path.to_lowercase();
            if l.ends_with(".tei") || l.ends_with(".xml") {
                return Some(crate::dialogs::tei::open(app, json!({"path":path}), "place"));
            }
            if !(l.ends_with(".docx") || l.ends_with(".rtf")) {
                return Some(app.run("file.place", json!({"path": path})));
            }
            let info = match app.run("place.styles", json!({"path": path})) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "importOptions",
                json!({"path": path, "styles": info["paragraph"], "charStyles": info["character"], "conflicts": info["conflicts"], "styleConflicts": "useExisting", "removeStyles": false, "map": {}}),
            ));
            Ok(Value::Null)
        }
        "app.save" | "app.saveDialog" if app.services.download.is_some() => download_document(app),
        "app.save" => {
            if app.session.active().is_some_and(|d| d.path.is_some()) {
                return Some(app.run("file.save", json!({})));
            }
            return run_ui(app, "app.saveDialog", p);
        }
        "app.saveDialog" => {
            let name = app
                .session
                .active()
                .map(|d| format!("{}.designcraft", d.doc.publication.as_ref().map_or(d.doc.title.as_str(), |v| &v.root.title)))
                .unwrap_or_default();
            if let Some(path) = app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                return Some(app.run("file.saveAs", json!({"path": path})));
            }
            Ok(Value::Null)
        }
        "app.saveCopyDialog" => {
            let name = app
                .session
                .active()
                .map(|d| format!("{} copy.designcraft", d.doc.publication.as_ref().map_or(d.doc.title.as_str(), |v| &v.root.title)))
                .unwrap_or_default();
            if let Some(path) = app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                if app.services.download.is_some() {
                    return Some(download_copy(app, &path));
                }
                return Some(app.run("file.saveACopy", json!({"path": path})));
            }
            Ok(Value::Null)
        }
        "app.exportPng" => export_png(app, p),
        "app.exportIdml" => export_idml(app, p),
        "app.exportEpub" => {
            let mut q = if p.is_object() { p.clone() } else { json!({}) };
            if let Some(q) = q.as_object_mut() {
                q.entry("cover").or_insert(json!(true));
            }
            export_bytes(app, &q, "epub", "file.exportEpub")
        }
        "app.exportInteractivePdf" => {
            let mut q = if p.is_object() { p.clone() } else { json!({}) };
            for (k, v) in [("pageLayout", json!("single")), ("view", json!("fitPage")), ("bookmarksPanel", json!(true)), ("media", json!(true))] {
                q.as_object_mut().map(|o| o.entry(k).or_insert(v));
            }
            export_pdf(app, &q)
        }
        "app.exportFixedEpub" => export_bytes(app, p, "epub", "file.exportFixedEpub"),
        "app.exportHtml" => export_bytes(app, p, "html", "file.exportHtml"),
        "app.exportXml" => export_bytes(app, p, "xml", "file.exportXml"),
        "app.printBooklet" => export_bytes(app, p, "pdf", "file.printBooklet"),
        "app.menus" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("menus", json!({"query": ""})));
            Ok(Value::Null)
        }
        "app.placeAndLink" => {
            let target = app.session.active().and_then(|d| {
                let id = *d.selection.items.first()?;
                let it = d.doc.item(id)?;
                let loc = d.doc.find(id)?;
                Some((it.text_frame()?.story, it.bounds(), loc.spread))
            });
            let Some((sid, b, sr)) = target else { return Some(Err("select a text frame".into())) };
            Ok(app
                .run("story.placeAndLink", json!({"story": sid.0, "rect": [b.x0 + 24.0, b.y0 + 24.0, b.x1 + 24.0, b.y1 + 24.0], "spread": sr}))
                .unwrap_or(Value::Null))
        }
        "app.fittingOptions" => {
            let g = app.session.active().and_then(|d| d.selection.items.first().and_then(|id| d.doc.item(*id)).and_then(|it| it.graphic().cloned()));
            let Some(g) = g else { return Some(Err("select a frame with a placed graphic".into())) };
            let fitting = serde_json::to_value(g.auto_fit).unwrap_or(json!("none"));
            let c = |v: f64| json!(designcraft_geom::format_measure(v, designcraft_geom::Unit::Points));
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "fittingOptions",
                json!({"autoFit": g.auto_fit != designcraft_doc::Fitting::None, "fitting": fitting, "align": g.fit_align,
                    "cropTop": c(g.crop[0]), "cropLeft": c(g.crop[1]), "cropBottom": c(g.crop[2]), "cropRight": c(g.crop[3])}),
            ));
            Ok(Value::Null)
        }
        "app.userDictionary" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("userDictionary", json!({"word": ""})));
            Ok(Value::Null)
        }
        "app.qrCode" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("qrCode", json!({})));
            Ok(Value::Null)
        }
        "app.exportText" => export_bytes(app, p, "rtf", "file.exportText"),
        "app.packageDialog" => {
            let name = app.session.active().map(|d| format!("{} Folder", d.doc.title)).unwrap_or_default();
            match app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                Some(dir) => {
                    let r = app.run("file.package", json!({"dir": dir}));
                    if let Ok(v) = &r {
                        app.status(format!(
                            "Packaged {} files into {}",
                            v["files"].as_array().map_or(0, |f| f.len()),
                            v["dir"].as_str().unwrap_or("")
                        ));
                    }
                    r
                }
                None => Ok(Value::Null),
            }
        }
        "app.language" => {
            let v = p.get("lang").and_then(Value::as_str).unwrap_or("");
            if !crate::i18n::LANGUAGES.iter().any(|(c, _)| *c == v) {
                return Some(Err(format!("unknown language `{v}`")));
            }
            app.ui.language = v.to_string();
            Ok(json!(v))
        }
        "app.flattener" => {
            let v = p.get("preset").and_then(Value::as_str).unwrap_or("");
            if !["", "high", "medium", "low"].contains(&v) {
                return Some(Err(format!("unknown preset `{v}`")));
            }
            app.ui.flattener = v.to_string();
            Ok(json!(v))
        }
        "app.graphicCell" => {
            let Some(pick) = app.services.pick_open.as_mut() else { return Some(Err("needs a file picker".into())) };
            match pick("place") {
                Some(path) => app.run("table.placeGraphic", json!({"path": path})),
                None => Ok(Value::Null),
            }
        }
        "app.loadSwatches" => {
            if let Some(pick) = app.services.pick_open.as_mut() {
                return Some(match pick("swatches") {
                    Some(path) => app.run("swatch.load", json!({"path": path})),
                    None => Ok(Value::Null),
                });
            }
            if let Some(open) = app.services.open_async.as_mut() {
                open("swatches");
            }
            Ok(Value::Null)
        }
        "app.saveSwatches" => {
            let path = match p.get("path").and_then(Value::as_str) {
                Some(s) => Some(s.to_string()),
                None => app.services.pick_save.as_mut().and_then(|f| f("Swatches.ase")),
            };
            let Some(path) = path else { return Some(Ok(Value::Null)) };
            let r = match app.run("swatch.save", json!({})) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
            match app.services.write.as_mut() {
                Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
                None => Err("no writer".into()),
            }
        }
        "app.storyEditor" => {
            app.review_editor = None;
            if let Some(note) = p["note"].as_u64() {
                if let Err(error) = app.run("publication.note.select", json!({"id":note})) {
                    return Some(Err(error));
                }
                app.note_editor = Some(note);
                app.story_editor = app.session.active().and_then(|s| s.selection.text.map(|t| t.story));
                return Some(Ok(Value::Null));
            }
            let sid = p.get("story").and_then(Value::as_u64).map(designcraft_doc::StoryId).or_else(|| {
                let st = app.session.active()?;
                st.selection.text.map(|t| t.story).or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))
            });
            match sid {
                Some(s) => {
                    app.note_editor = app
                        .session
                        .active()
                        .and_then(|st| st.selection.text.filter(|t| t.story == s).and_then(|t| t.cell).and_then(|c| c.annotation_id()));
                    app.story_editor = if app.story_editor == Some(s) { None } else { Some(s) };
                    Ok(Value::Null)
                }
                None => Err("select a text frame or place the cursor in text".into()),
            }
        }
        "app.findChange" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("findChange", json!({})));
            Ok(Value::Null)
        }
        "app.insertTableDialog" => {
            if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
                return Some(Err("place the insertion point in a text frame to create a table".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("insertTable", p.clone()));
            Ok(Value::Null)
        }
        "app.rubyDialog" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let cur = st.selection.text.and_then(|t| {
                let story = st.doc.text_story(t.story, t.cell)?;
                let at = if t.range().is_empty() { t.range().start } else { t.range().start + 1 };
                story.char_format_at(at).over.ruby.clone()
            });
            app.ui.dialog = Some(crate::dialogs::Dialog::new("ruby", json!({"text": cur.unwrap_or_default()})));
            Ok(Value::Null)
        }
        "app.footnoteOptionsDialog" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let o = serde_json::to_value(&st.doc.footnote_options).unwrap_or_default();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("footnoteOptions", o));
            Ok(Value::Null)
        }
        "app.findFontDialog" => {
            if app.session.active().is_none() {
                return Some(Err("no document open".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("findFont", json!({})));
            Ok(Value::Null)
        }
        "app.insertXrefDialog" => {
            if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
                return Some(Err("place the insertion point in text to insert a cross-reference".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("insertXref", json!({})));
            Ok(Value::Null)
        }
        "app.deleteAllGuides" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let spread = if st.editing_parents { json!({"kind": "parent", "index": 0}) } else { json!(st.doc.page_loc(page).map_or(0, |l| l.0)) };
            return Some(app.run("guide.deleteAll", json!({"spread": spread})));
        }
        "app.deletePage" | "app.duplicateSpread" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let r = if id == "app.deletePage" {
                app.run("layout.pages.delete", json!({"pages": [page]}))
            } else {
                let spread = st.doc.page_loc(page).map_or(0, |l| l.0);
                app.run("layout.pages.duplicateSpread", json!({"spread": spread}))
            };
            return Some(r);
        }
        "app.tablePanel" => {
            app.ui.open_panel = if app.ui.open_panel.as_deref() == Some("table") { None } else { Some("table".into()) };
            Ok(Value::Null)
        }
        "app.exportPdf" => export_pdf(app, p),
        "app.palette" => {
            app.ui.palette = Some(String::new());
            Ok(Value::Null)
        }
        "app.preferences" => {
            let mut f = app.session.execute("prefs.set", &json!({})).unwrap_or_default();
            if app.session.active().is_some() {
                let doc = app.session.execute("document.preferences", &json!({})).unwrap_or_default();
                f["horizontalUnits"] = doc["horizontalUnits"].clone();
                f["overprintBlack"] = doc["overprintBlack"].clone();
                f["glyphFallback"] = doc["glyphFallback"].clone();
                for k in ["superscriptSize", "superscriptPosition", "subscriptSize", "subscriptPosition"] {
                    f[format!("adv.{k}")] = json!(format!("{}", doc["advancedType"][k].as_f64().unwrap_or(0.0)));
                }
                f["verticalUnits"] = doc["verticalUnits"].clone();
                let words = app.session.execute("spelling.words", &json!({})).unwrap_or_default();
                f["userWords"] =
                    json!(words.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default());
                let inc = doc["keyboardIncrement"].as_f64().unwrap_or(1.0);
                f["keyboardIncrement"] = json!(designcraft_geom::format_measure(inc, designcraft_geom::Unit::Points));
                let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or_default();
                let m = |v: &Value| json!(designcraft_geom::format_measure(v.as_f64().unwrap_or(0.0), units));
                let (bg, g) = (&doc["baselineGrid"], &doc["grid"]);
                f["bg.start"] = m(&bg["start"]);
                f["bg.increment"] = m(&bg["increment"]);
                f["bg.relativeTo"] = bg["relativeTo"].clone();
                f["bg.viewThreshold"] = json!((bg["viewThreshold"].as_f64().unwrap_or(0.75) * 100.0).round());
                f["bg.color"] = bg["color"].clone();
                f["grid.horizontal"] = m(&g["horizontal"]);
                f["grid.vertical"] = m(&g["vertical"]);
                f["grid.subdivisions"] = g["subdivisions"].clone();
                f["grid.inBack"] = g["inBack"].clone();
                f["grid.color"] = g["color"].clone();
                for k in ["marginColor", "columnColor", "bleedColor", "slugColor"] {
                    f[k] = doc[k].clone();
                }
                f["pasteboard.h"] = m(&doc["pasteboard"][0]);
                f["pasteboard.v"] = m(&doc["pasteboard"][1]);
            }
            f["snap.alignEdges"] = json!(app.ui.align_edges);
            f["snap.alignCenters"] = json!(app.ui.align_centers);
            f["snap.dimensions"] = json!(app.ui.smart_dimensions);
            f["snap.spacing"] = json!(app.ui.smart_spacing);
            f["snap.zone"] = json!(app.ui.snap_zone);
            f["displayQuality"] = json!(match app.ui.display_quality {
                designcraft_render::DisplayQuality::Fast => "fast",
                designcraft_render::DisplayQuality::Typical => "typical",
                designcraft_render::DisplayQuality::High => "high",
            });
            f["uiScale"] = json!((app.ui.ui_scale * 100.0).round());
            f["richBlack"] = json!(app.ui.rich_black);
            f["dynamicSpelling"] = json!(app.ui.dynamic_spelling);
            f["storyEditorSize"] = json!(app.ui.story_editor_size);
            let list = app.session.prefs.autocorrect_list.iter().map(|(a, b)| format!("{a} → {b}")).collect::<Vec<_>>().join("\n");
            f["autocorrectText"] = json!(list);
            f["section"] = json!("general");
            app.ui.dialog = Some(crate::dialogs::Dialog::new("preferences", f));
            Ok(Value::Null)
        }
        "app.layerOptions" => {
            let layers = match app.session.execute("object.pdfLayers", &json!({})) {
                Ok(v) => v,
                Err(e) => return Some(Err(e.to_string())),
            };
            app.ui.dialog = Some(crate::dialogs::Dialog::new("layerOptions", json!({"layers": layers})));
            Ok(Value::Null)
        }
        "app.colorSettings" => {
            let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("colorSettings", f));
            Ok(Value::Null)
        }
        "app.keyboardShortcuts" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("keyboardShortcuts", json!({"query": "", "recording": ""})));
            Ok(Value::Null)
        }
        "edit.dynamicSpelling" => flag(&mut app.ui.dynamic_spelling),
        "help.about" => {
            if let Some(tab) = p.get("tab").and_then(Value::as_str) {
                let Some(i) = crate::about::ABOUT_TABS.iter().position(|t| t.eq_ignore_ascii_case(tab)) else {
                    return Some(Err(format!("help.about: unknown tab `{tab}` (about, contributors, models)")));
                };
                app.ui.about_tab = u8::try_from(i).unwrap_or(0);
            }
            app.ui.about = p.get("open").and_then(Value::as_bool).unwrap_or(true);
            Ok(json!(app.ui.about))
        }
        "help.app" => match p.get("app").and_then(Value::as_str) {
            Some(slug) if !slug.is_empty() && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                let url = format!("{}/apps/{slug}", designcraft_engine::links::WEBSITE);
                app.ui.pending_urls.push(url.clone());
                Ok(json!({"url": url}))
            }
            _ => Err("help.app: give `app` (e.g. \"photocraft\")".into()),
        },
        h if h.starts_with("help.") && designcraft_engine::links::get(&h[5..]).is_some() => {
            let url = designcraft_engine::links::get(&h[5..]).unwrap_or_default().to_string();
            app.ui.pending_urls.push(url.clone());
            Ok(json!({"url": url}))
        }
        _ => return None,
    })
}

/// Is a command enabled (for menu greying)?
pub fn enabled(app: &DesignApp, id: &str) -> bool {
    match designcraft_engine::find_command(id) {
        Some(c) => (c.enabled)(&app.session).is_ok(),
        None => true,
    }
}

/// Check state of a toggle command (None = not a toggle).
pub fn checked(app: &DesignApp, id: &str, params: &Value) -> Option<bool> {
    Some(match id {
        "view.rulers" => app.ui.rulers,
        "view.frameEdges" => app.ui.frame_edges,
        "view.overprintPreview" => app.ui.overprint_preview,
        "view.fastDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::Fast,
        "view.typicalDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::Typical,
        "view.highQualityDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::High,
        "view.guides" => app.ui.guides,
        "view.snapToGuides" => app.ui.snap_to_guides,
        "view.snapToDocumentGrid" => app.ui.snap_to_document_grid,
        "view.smartGuides" => app.ui.smart_guides,
        "view.baselineGrid" => app.ui.baseline_grid,
        "view.textThreads" => app.ui.text_threads,
        "view.hiddenCharacters" => app.ui.hidden_characters,
        "view.taggedFrames" => app.ui.tagged_frames,
        "changes.track" => app.session.active().is_some_and(|d| d.doc.settings.track_changes),
        "window.controlBar" => app.ui.control_bar,
        "window.split" => app.split,
        "window.newWindow" => app.second_window,
        "view.proofColors" => app.ui.proof_colors,
        "view.flattenerPreview" => app.ui.flattener_preview,
        "view.tagMarkers" => app.ui.tag_markers,
        "view.rotateSpread" => match params.get("angle").and_then(Value::as_i64) {
            Some(0) => app.session.active().is_some_and(|st| st.doc.spreads.iter().all(|sp| sp.pages.first().is_none_or(|p| p.view_rotation == 0))),
            _ => return None,
        },
        "view.proofSetup" => match (params.get("target").and_then(Value::as_str), params.get("simulatePaper")) {
            (Some(t), _) => app.ui.proof_setup.target.id() == t,
            (None, Some(_)) => app.ui.proof_setup.simulate_paper,
            _ => return None,
        },
        "edit.dynamicSpelling" => app.ui.dynamic_spelling,
        "app.language" => app.ui.language == params.get("lang").and_then(Value::as_str).unwrap_or(""),
        "app.flattener" => app.ui.flattener == params.get("preset").and_then(Value::as_str).unwrap_or(""),
        "window.taskBar" => app.ui.task_bar,
        "window.toolsDoubleColumn" => app.ui.tools_double_column,
        "view.togglePreview" => app.ui.screen_mode == crate::ScreenMode::Preview,
        "window.brightness" => params.get("brightness").and_then(Value::as_str) == Some(app.ui.brightness.id()),
        "type.storyDirection" => {
            let st = app.session.active()?;
            let sid = st
                .selection
                .text
                .map(|t| t.story)
                .or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))?;
            st.doc.story(sid)?.vertical == params.get("vertical").and_then(Value::as_bool)?
        }
        _ => return None,
    })
}

/// Enablement for menu display (UI commands need a document unless they're app/window-level).
pub fn menu_enabled(app: &DesignApp, id: &str) -> bool {
    if ui_label(id).is_some() {
        return app.session.active().is_some() || id.starts_with("app.") || id.starts_with("window.");
    }
    enabled(app, id)
}

/// A menu item was chosen. An engine command whose label ends in "…" and that takes parameters
/// opens a dialog built from its parameter documentation (see [`crate::dialogs::command_fields`]).
pub fn activate(app: &mut DesignApp, id: &str, params: &Value) {
    if params.is_null() && id == "layout.documentSetup" {
        crate::dialogs::open_document_setup(app);
        return;
    }
    if params.is_null() && id == "file.print" {
        crate::dialogs::open_print(app);
        return;
    }
    if params.is_null() && id == "object.textFrameOptions" {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
        return;
    }
    if params.is_null()
        && ui_label(id).is_none()
        && let Some(c) = designcraft_engine::find_command(id)
        && !crate::dialogs::command_fields(c.params).is_empty()
        && (c.label.ends_with('…') || crate::dialogs::command_fields(c.params).iter().any(|f| !f.optional))
    {
        app.ui.dialog = Some(crate::dialogs::Dialog::new(&format!("cmd:{id}"), json!({})));
        return;
    }
    let p = if params.is_null() { json!({}) } else { params.clone() };
    let _ = app.run(id, p);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirty_app(services: crate::Services) -> crate::DesignApp {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), services);
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Unsaved text"})).unwrap();
        app
    }

    fn close_action(app: &mut crate::DesignApp, action: &str) -> Result<Value, String> {
        app.ui.dialog.as_mut().unwrap().fields.insert("action".into(), json!(action));
        crate::dialogs::confirm(app)
    }

    #[test]
    fn regression_close_dialog_cancel_save_and_discard() {
        let mut app = dirty_app(crate::Services { pick_save: Some(Box::new(|_| None)), ..Default::default() });
        let uid = app.session.doc().unwrap().uid;
        app.run("app.close", json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "closeDocument");
        close_action(&mut app, "cancel").unwrap();
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.session.doc().unwrap().uid, uid);
        app.run("app.close", json!({})).unwrap();
        assert!(close_action(&mut app, "save").is_err(), "cancelling the file picker must keep the document open");
        assert!(app.session.doc().unwrap().is_dirty());
        assert!(app.ui.dialog.is_some());
        close_action(&mut app, "discard").unwrap();
        assert!(app.session.documents().is_empty());
    }

    #[test]
    fn regression_close_confirmation_follows_document_identity() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Clean"})).unwrap();
        app.run("file.new", json!({"title": "Close this one"})).unwrap();
        app.run("frame.create", json!({"rect": [0, 0, 100, 100]})).unwrap();
        let target = app.session.doc().unwrap().uid;
        app.run("app.close", json!({"index": 1})).unwrap();
        app.run("file.new", json!({"title": "Keep this one"})).unwrap();
        let keep = app.session.doc().unwrap().uid;
        app.run("file.close", json!({"index": 0})).unwrap();
        close_action(&mut app, "discard").unwrap();
        assert!(!app.session.documents().iter().any(|st| st.uid == target));
        assert_eq!(app.session.doc().unwrap().uid, keep);
    }

    #[test]
    fn regression_close_saves_before_removing_the_document() {
        let dir = std::env::temp_dir().join(format!("dc-close-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("saved.designcraft");
        let chosen = path.to_string_lossy().into_owned();
        let mut app = dirty_app(crate::Services { pick_save: Some(Box::new(move |_| Some(chosen.clone()))), ..Default::default() });
        app.run("app.close", json!({})).unwrap();
        close_action(&mut app, "save").unwrap();
        assert!(app.session.documents().is_empty());
        assert!(app.ui.dialog.is_none());
        let mut reopened = designcraft_engine::Session::new();
        reopened.execute("file.open", &json!({"path": path})).unwrap();
        assert!(reopened.doc().unwrap().doc.stories.values().any(|st| st.text == "Unsaved text"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn regression_close_rejects_invalid_indices_and_uses_the_ui_shortcut() {
        let mut app = dirty_app(crate::Services::default());
        for index in [json!(-1), json!("0"), json!(5)] {
            assert!(app.run("app.close", json!({"index": index})).is_err());
            assert!(app.ui.dialog.is_none());
            assert_eq!(app.session.documents().len(), 1);
        }
        let close = effective_shortcuts(&app).into_iter().find(|(_, shortcut)| shortcut == "Cmd+W");
        assert_eq!(close.unwrap().0, "app.close");
    }

    #[test]
    fn regression_browser_copy_downloads_without_marking_saved() {
        let deliveries = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let output = deliveries.clone();
        let services = crate::Services {
            pick_save: Some(Box::new(|_| Some("copy.designcraft".into()))),
            download: Some(Box::new(|_, _| panic!("use the fallible writer when available"))),
            write: Some(Box::new(move |name, bytes| {
                output.borrow_mut().push((name.to_string(), bytes.to_vec()));
                Ok(())
            })),
            ..Default::default()
        };
        let mut app = dirty_app(services);
        let path = app.session.doc().unwrap().path.clone();
        let saved = app.session.doc().unwrap().saved_doc.clone();
        let result = app.run("app.saveCopyDialog", json!({})).unwrap();
        assert_eq!(result["path"], "copy.designcraft");
        assert_eq!(deliveries.borrow().len(), 1);
        let bytes = &deliveries.borrow()[0].1;
        assert_eq!(result["bytes"], bytes.len());
        let mut reopened = designcraft_engine::Session::new();
        reopened.execute("file.openBytes", &json!({"base64": designcraft_engine::cmd::base64_encode(bytes)})).unwrap();
        assert!(reopened.doc().unwrap().doc.stories.values().any(|st| st.text == "Unsaved text"));
        assert_eq!(app.session.doc().unwrap().path, path);
        assert!(std::sync::Arc::ptr_eq(&app.session.doc().unwrap().saved_doc, &saved));
        assert!(app.session.doc().unwrap().is_dirty());
    }

    #[test]
    fn regression_failed_browser_save_keeps_close_dialog_and_unsaved_work() {
        let mut app = dirty_app(crate::Services {
            pick_save: Some(Box::new(|_| Some("copy.designcraft".into()))),
            download: Some(Box::new(|_, _| {})),
            write: Some(Box::new(|_, _| Err("download failed".into()))),
            ..Default::default()
        });
        assert_eq!(app.run("app.saveCopyDialog", json!({})).unwrap_err(), "download failed");
        app.run("app.close", json!({})).unwrap();
        assert_eq!(close_action(&mut app, "save").unwrap_err(), "download failed");
        assert!(app.session.doc().unwrap().is_dirty());
        assert!(app.ui.dialog.is_some());
        assert!(app.session.doc().unwrap().path.is_none());
    }

    fn walk(items: &[Item], out: &mut Vec<(String, String, Value)>) {
        for it in items {
            match it {
                Item::Sub(_, c) => walk(c, out),
                Item::Cmd { label, id, params, .. } => out.push((label.clone(), id.clone(), params.clone())),
                Item::Sep => {}
            }
        }
    }

    #[test]
    fn transform_menu_items_open_their_dialogs() {
        for id in ["transform.move", "transform.scale", "transform.rotate", "transform.shear"] {
            let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            activate(&mut app, id, &Value::Null);
            let dialog = app.ui.dialog.as_ref().map(|d| d.id.clone());
            assert_eq!(dialog.as_deref(), Some(format!("cmd:{id}").as_str()), "{id} opens its dialog");
        }
    }

    #[test]
    fn snap_preferences_are_a_command() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let r = run_ui(&mut app, "view.snapPreferences", &json!({"smartSpacing": false, "zone": 8})).unwrap().unwrap();
        assert_eq!(r["smartSpacing"], false);
        assert_eq!(r["alignEdges"], true, "unnamed switches keep their value");
        assert_eq!(r["zone"], 8.0);
        assert!(!app.ui.smart_spacing && (app.ui.snap_zone - 8.0).abs() < 1e-9);
        let r = run_ui(&mut app, "view.snapPreferences", &json!({"zone": -3})).unwrap().unwrap();
        assert_eq!(r["zone"], 0.0, "a negative zone turns snapping off");
    }

    #[test]
    fn custom_shortcuts_override_defaults() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert_eq!(shortcut_of(&app, "app.preferences").as_deref(), Some("Cmd+K"));
        let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "app.preferences", "shortcut": "Cmd+Alt+Shift+9"})).unwrap().unwrap();
        assert_eq!(r["shortcut"], "Cmd+Alt+Shift+9");
        assert!(effective_shortcuts(&app).contains(&("app.preferences".into(), "Cmd+Alt+Shift+9".into())));
        // Taking another command's keys reports the conflict.
        let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Alt+Shift+9"})).unwrap().unwrap();
        assert_eq!(r["conflicts"], json!(["app.preferences"]));
        run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": ""})).unwrap().unwrap();
        assert_eq!(shortcut_of(&app, "app.palette"), None);
        assert!(run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Nope"})).unwrap().is_err());
        run_ui(&mut app, "window.resetShortcuts", &json!({})).unwrap().unwrap();
        // ⌘N is the New Document dialog, not a bare file.new.
        let first_cmd_n = effective_shortcuts(&app).into_iter().find(|(_, s)| s == "Cmd+N").map(|(i, _)| i);
        assert_eq!(first_cmd_n.as_deref(), Some("app.newDocumentDialog"));
        assert_eq!(shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Return"));
        assert_eq!(shortcut_string(egui::Modifiers { command: true, shift: true, ..Default::default() }, egui::Key::K), "Cmd+Shift+K");
        if cfg!(target_os = "macos") {
            assert_eq!(shortcut_text("Cmd+Alt+3"), "⌥⌘3");
            assert_eq!(shortcut_text("Cmd+Shift+."), "⇧⌘.");
        } else {
            assert_eq!(shortcut_text("Cmd+Alt+3"), "Alt+Ctrl+3");
        }
    }

    #[test]
    fn split_window_panes_have_their_own_views() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        assert_eq!(run_ui(&mut app, "window.split", &json!({})).unwrap().unwrap(), json!(true));
        frame(&mut app);
        frame(&mut app);
        // Both panes have a canvas, each half as wide.
        let r0 = app.canvas_rect.unwrap();
        let r1 = app.other_pane.as_ref().and_then(|o| o.1).unwrap();
        assert!(r0.width() < 800.0 && r1.width() < 800.0 && (r0.center().x - r1.center().x).abs() > 300.0, "{r0:?} {r1:?}");
        // Zooming the right pane leaves the left alone.
        app.switch_pane(1);
        let z0 = app.view().unwrap().zoom;
        crate::canvas::set_zoom(&mut app, z0 * 3.0);
        app.switch_pane(0);
        assert!((app.view().unwrap().zoom - z0).abs() < 1e-9);
        app.switch_pane(1);
        assert!((app.view().unwrap().zoom - z0 * 3.0).abs() < 1e-6);
        assert_eq!(run_ui(&mut app, "window.split", &json!({})).unwrap().unwrap(), json!(false));
        assert_eq!(app.pane, 0);
        frame(&mut app);
    }

    #[test]
    fn interface_language_switches_and_validates() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        run_ui(&mut app, "app.language", &json!({"lang": "de"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "de");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "de"})), Some(true));
        assert_eq!(crate::i18n::tr(&app.ui.language, "Window"), "Fenster");
        assert!(run_ui(&mut app, "app.language", &json!({"lang": "xx"})).unwrap().is_err());

        run_ui(&mut app, "app.language", &json!({"lang": "zh"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "zh");
        assert_eq!(crate::i18n::tr(&app.ui.language, "File"), "文件");

        app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
        let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
        run_ui(&mut app, "app.language", &json!({"lang": "ar"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "ar");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "ar"})), Some(true));
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap());
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("ar", title), title, "{title}");
        }
        // Every menu title has a translation (Japanese never matches the English).
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("ja", title), title, "{title}");
        }

        run_ui(&mut app, "app.language", &json!({"lang": "pt-br"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "pt-br");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "pt-br"})), Some(true));
        for (title, _) in menu_tree() {
            // "Layout" is the natural Brazilian Portuguese word, so it stays identical
            // to English; every other title must be translated.
            if title != "Layout" {
                assert_ne!(crate::i18n::tr("pt-br", title), title, "{title}");
            }
        }
    }

    #[test]
    fn arabic_dialog_and_language_switch_keep_finite_widget_geometry() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            for _ in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                        max_texture_side: Some(8192),
                        ..Default::default()
                    },
                    |ui| {
                        app.logic(&ui.ctx().clone());
                        app.ui(ui);
                    },
                );
                output.textures_delta.clear();
            }
        };
        app.run("app.language", json!({"lang": "ar"})).unwrap();
        frame(&mut app);
        app.run("app.newDocumentDialog", json!({})).unwrap();
        frame(&mut app); // egui asserts that all allocated rectangles are finite.
        crate::dialogs::confirm(&mut app).unwrap();
        frame(&mut app);
        assert_eq!(app.session.documents().len(), 1);
        for lang in ["zh", "", "ar", "pt-br"] {
            app.run("app.language", json!({"lang": lang})).unwrap();
            frame(&mut app);
        }
    }

    #[test]
    fn story_direction_is_checked() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "縦", "vertical": true})).unwrap();
        assert_eq!(checked(&app, "type.storyDirection", &json!({"vertical": true})), Some(true));
        assert_eq!(checked(&app, "type.storyDirection", &json!({"vertical": false})), Some(false));
    }

    #[test]
    fn proof_setup_and_colors() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert_eq!(checked(&app, "view.proofColors", &Value::Null), Some(false));
        let r = run_ui(&mut app, "view.proofSetup", &json!({"target": "deuteranopia"})).unwrap().unwrap();
        assert_eq!(r["on"], true);
        assert_eq!(checked(&app, "view.proofSetup", &json!({"target": "deuteranopia"})), Some(true));
        run_ui(&mut app, "view.proofSetup", &json!({"simulatePaper": "toggle"})).unwrap().unwrap();
        assert!(app.ui.proof_setup.simulate_paper);
        run_ui(&mut app, "view.proofColors", &json!({})).unwrap().unwrap();
        assert!(!app.ui.proof_colors);
        assert!(run_ui(&mut app, "view.proofSetup", &json!({"target": "nope"})).unwrap().is_err());
    }

    #[test]
    fn tab_hides_panels_and_documents_cycle() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert_eq!(shortcut_of(&app, "window.hidePanels").as_deref(), Some("Tab"));
        run_ui(&mut app, "window.hidePanels", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.hidden_panels, 1);
        run_ui(&mut app, "window.hidePanelsExceptTools", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.hidden_panels, 2);
        run_ui(&mut app, "window.hidePanelsExceptTools", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.hidden_panels, 0);
        for _ in 0..3 {
            app.session.execute("file.new", &json!({})).unwrap();
        }
        assert_eq!(app.session.active_index(), Some(2));
        run_ui(&mut app, "window.nextDocument", &json!({})).unwrap().unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        run_ui(&mut app, "window.previousDocument", &json!({})).unwrap().unwrap();
        assert_eq!(app.session.active_index(), Some(2));
    }

    #[test]
    fn rotate_spread_view() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({"pages": 3})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        frame(&mut app);
        assert_eq!(run_ui(&mut app, "view.rotateSpread", &json!({"angle": 90})).unwrap().unwrap(), json!(90));
        frame(&mut app);
        // Only the spread in view turns: its canvas footprint swaps width and height.
        let d = app.session.active().unwrap().doc.clone();
        let turned: Vec<usize> = d.spreads.iter().enumerate().filter(|(_, sp)| sp.pages[0].view_rotation == 1).map(|(i, _)| i).collect();
        assert_eq!(turned.len(), 1);
        let layout = designcraft_tools::CanvasLayout::new(&d, false);
        let slot = layout.slots[turned[0]];
        let b = d.spreads[turned[0]].bounds();
        assert!((slot.bounds.width() - b.height()).abs() < 1e-6 && (slot.bounds.height() - b.width()).abs() < 1e-6);
        assert_eq!(checked(&app, "view.rotateSpread", &json!({"angle": 0})), Some(false));
        run_ui(&mut app, "view.rotateSpread", &json!({"angle": 0})).unwrap().unwrap();
        assert_eq!(checked(&app, "view.rotateSpread", &json!({"angle": 0})), Some(true));
        frame(&mut app);
    }

    #[test]
    fn hand_tool_hold_starts_power_zoom() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, t: f64, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                time: Some(t),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 0.05, vec![]);
        app.select_tool("hand");
        let r = app.canvas_rect.unwrap();
        let p = r.center();
        let z0 = app.view().unwrap().zoom;
        frame(&mut app, 0.1, vec![egui::Event::PointerMoved(p)]);
        frame(
            &mut app,
            0.2,
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }],
        );
        frame(&mut app, 0.4, vec![]);
        assert!(app.power_zoom.is_none(), "not yet");
        frame(&mut app, 0.8, vec![]);
        assert!(app.power_zoom.is_some(), "holding still starts Power Zoom");
        frame(
            &mut app,
            0.9,
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
        );
        assert!(app.power_zoom.is_none());
        assert!((app.view().unwrap().zoom - z0).abs() < 1e-9, "back at the zoom it started from");
    }

    #[test]
    fn middle_drag_pans_with_any_tool() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        // A drawing tool: a primary drag would make a frame, a middle drag must not.
        app.select_tool("rectangleFrame");
        let p = app.canvas_rect.unwrap().center();
        let v0 = *app.view().unwrap();
        let items = |app: &crate::DesignApp| app.session.active().unwrap().doc.spreads.iter().map(|s| s.items.len()).sum::<usize>();
        let items0 = items(&app);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: Default::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(p), button(p, true)]);
        for i in 1..=8 {
            frame(&mut app, vec![egui::Event::PointerMoved(p + egui::vec2(10.0 * i as f32, 5.0 * i as f32))]);
        }
        frame(&mut app, vec![button(p + egui::vec2(80.0, 40.0), false)]);
        let v = *app.view().unwrap();
        assert!((v.zoom - v0.zoom).abs() < 1e-9, "panning keeps the zoom");
        assert!(v.origin.x < v0.origin.x && v.origin.y < v0.origin.y, "dragging right/down moves the view: {v0:?} -> {v:?}");
        assert_eq!(items(&app), items0, "the tool saw nothing");
    }

    #[test]
    fn tool_shortcuts_work_after_drawing_on_the_canvas() {
        // Clicking the canvas gives it keyboard focus; single-key tool shortcuts must still switch
        // tools without pressing Esc first (#1).
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let key = |k: egui::Key| {
            vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() },
            ]
        };
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, key(egui::Key::F));
        assert_eq!(app.session.tool_id(), "rectangleFrame");
        // Draw a frame.
        let a = app.canvas_rect.unwrap().center();
        let b = a + egui::vec2(120.0, 80.0);
        frame(&mut app, vec![egui::Event::PointerMoved(a), button(a, true)]);
        for i in 1..=4 {
            frame(&mut app, vec![egui::Event::PointerMoved(a + (b - a) * (i as f32 / 4.0))]);
        }
        frame(&mut app, vec![button(b, false)]);
        frame(&mut app, vec![]);
        assert!(app.session.active().is_some_and(|d| !d.selection.items.is_empty()), "drew a frame");
        frame(&mut app, key(egui::Key::V));
        assert_eq!(app.session.tool_id(), "selection", "V switches tools straight after drawing");
        frame(&mut app, key(egui::Key::F));
        assert_eq!(app.session.tool_id(), "rectangleFrame");
    }

    #[test]
    fn sample_scripts_run() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        for (name, text) in app.ui.scripts.clone() {
            let r = app.session.execute("script.run", &json!({"text": text}));
            assert!(r.is_ok(), "{name}: {r:?}");
        }
    }

    #[test]
    fn new_window_shows_a_second_view() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        assert_eq!(run_ui(&mut app, "window.newWindow", &json!({})).unwrap().unwrap(), json!(true));
        assert!(!app.split);
        frame(&mut app);
        frame(&mut app);
        // Pane 1 was drawn (its rect is held for it) and has its own view.
        assert!(app.other_pane.as_ref().and_then(|o| o.1).is_some() || app.pane == 1);
        app.switch_pane(1);
        assert!(app.view().is_some(), "the second window has a view");
        app.switch_pane(0);
        assert_eq!(checked(&app, "window.newWindow", &Value::Null), Some(true));
        run_ui(&mut app, "window.newWindow", &json!({"on": false})).unwrap().unwrap();
        frame(&mut app);
    }

    #[test]
    fn panels_float_and_dock() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let r = run_ui(&mut app, "window.floatPanel", &json!({"panel": "swatches", "x": 50, "y": 60})).unwrap().unwrap();
        assert_eq!(r["floating"], json!(["swatches"]));
        assert_eq!(app.ui.floating[0].1, [50.0, 60.0]);
        // Showing a floating panel doesn't also open its flyout.
        run_ui(&mut app, "window.panel", &json!({"panel": "swatches"})).unwrap().unwrap();
        assert_eq!(app.ui.open_panel, None);
        run_ui(&mut app, "window.dockPanel", &json!({"panel": "swatches"})).unwrap().unwrap();
        assert!(app.ui.floating.is_empty());
        assert!(run_ui(&mut app, "window.floatPanel", &json!({"panel": "nope"})).unwrap().is_err());
    }

    #[test]
    fn quick_apply_lists_styles_then_commands() {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body Copy"})).unwrap();
        let items = quick_apply_items(&s, "body");
        assert_eq!(items[0].id, "style.paragraph.apply");
        assert_eq!(items[0].params, json!({"name": "Body Copy"}));
        assert_eq!(items[0].kind, "Paragraph Style");
        assert!(quick_apply_items(&s, "").iter().all(|i| i.kind.is_empty()));
        assert!(quick_apply_items(&s, "preferences").iter().any(|i| i.id == "app.preferences"));
    }

    #[test]
    fn every_menu_entry_is_a_command() {
        let mut all = Vec::new();
        for (_, items) in menu_tree() {
            walk(&items, &mut all);
        }
        assert!(all.len() > 80, "{}", all.len());
        for (label, id, params) in &all {
            assert!(ui_label(id).is_some() || designcraft_engine::find_command(id).is_some(), "menu entry {label}: unknown command {id}");
            assert!(!label.is_empty() && label != id, "menu entry {id} has no label");
            if let Some(o) = params.as_object() {
                assert!(!o.is_empty(), "{id}: empty fixed params");
            }
        }
        // Nested submenus parse (Type › Insert Special Character › Symbols › …).
        let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
        let special = ty.iter().find_map(|i| match i {
            Item::Sub(n, c) if n == "Insert Special Character" => Some(c.clone()),
            _ => None,
        });
        assert!(special.is_some_and(|c| c.iter().any(|i| matches!(i, Item::Sub(n, _) if n == "Symbols"))));
        // Fixed-parameter variants keep their own labels.
        assert!(all.iter().any(|(l, id, p)| l == "Fill Frame Proportionally" && id == "object.fit" && p["mode"] == "fillProportionally"));
    }

    #[test]
    fn custom_workspaces_save_apply_and_delete() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.dock_tab = "swatches".into();
        app.ui.control_bar = true;
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
        run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
        app.ui.dock_tab = "properties".into();
        run_ui(&mut app, "window.workspace", &json!({"name": "Mine"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.control_bar, app.ui.workspace.as_str()), ("swatches", true, "Mine"));
        app.ui.dock_tab = "layers".into();
        run_ui(&mut app, "window.resetWorkspace", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.dock_tab, "swatches", "reset to the saved arrangement");
        run_ui(&mut app, "window.deleteWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
        assert!(app.ui.custom_workspaces.is_empty());
        assert_eq!(app.ui.workspace, "Essentials");
    }

    #[test]
    fn hide_and_show_menu_items() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let items = all_menu_items();
        let (menu, label) = items.iter().find(|(m, _)| m == "Edit").cloned().expect("Edit items");
        let key = menu_key(&menu, &label);
        run_ui(&mut app, "window.hideMenuItem", &json!({"item": key})).unwrap().unwrap();
        assert!(app.ui.hidden_menu_items.contains(&key));
        run_ui(&mut app, "window.hideMenuItem", &json!({"item": key, "hidden": false})).unwrap().unwrap();
        assert!(app.ui.hidden_menu_items.is_empty());
        assert!(items.len() > 100, "every menu's items are listed: {}", items.len());
    }
}
