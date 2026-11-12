# minNotes links, file chips and inline choice chips — what the Qt app did (Phase 3, links / chips step)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`), read only. Nothing in it was
built or run; every statement is read from code, from the app's own manual or from its regression test, and anything
deduced rather than read is marked **(inference)**. "Not found" means the code was searched and has no such thing.
References are `alias:line` relative to the minNotes repository.
`BM` = `app/core/BlockModel.cpp`, `BMh` = `app/core/BlockModel.h`, `IT` = `app/core/InlineText.cpp`,
`ITh` = `app/core/InlineText.h`, `Hl` = `app/core/InlineMarkdownHighlighter.cpp`, `Ed` = `app/qml/MinNotes/App/Editor.qml`,
`BV` = `app/qml/MinNotes/App/BlockView.qml`, `MB` = `app/qml/MinNotes/App/MediaBlock.qml`,
`ChP` = `app/qml/MinNotes/App/ChoicePicker.qml`, `ChE` = `app/qml/MinNotes/App/ChoiceColumnEditor.qml`,
`Tb` = `app/qml/MinNotes/App/ChromeToolbar.qml`, `Mn` = `app/qml/MinNotes/App/Main.qml`, `MS` = `app/core/MediaStore.cpp`,
`PMh` = `app/core/PathMap.h`, `FL` = `app/core/FileLinks.cpp`, `CP` = `app/core/ClipboardPaster.cpp`,
`BC` = `app/core/BlockClipboard.cpp`, `Ex` = `app/core/Exporter.cpp`, `Im` = `app/core/Importer.cpp`,
`Dx` = `app/core/DocxReader.cpp`, `Od` = `app/core/OdfReader.cpp`, `CD` = `app/core/CanvasDocument.cpp`,
`Cv` = `app/qml/MinNotes/App/CanvasView.qml`, `CvM` = `app/qml/MinNotes/App/CanvasContextMenu.qml`,
`CFc` = `app/qml/MinNotes/App/CanvasFileCard.qml`, `ManE` = `docs/editing.md`, `ManM` = `docs/media.md`,
`ManX` = `docs/export.md`, `Sh` = `docs/shortcuts.md`, `T` = `tests/regression_test.cpp`.

Sibling specs this one leans on and does not repeat: `S1` = `step1-core-spec.md` (§1.4 span offsets, §1.5 the two
payload-span rules, §1.6 the chip functions), `Md` = `step-media-spec.md` (§1 the media descriptor, §2.4 a missing
file, §6.3 reveal), `TC` = `step-tables-columns-spec.md` (choice / check columns, the picker in a table, the options
editor). The new side (minplatform at `50174cc`): `inl` = `crates/mn-doc/src/inline.rs`, `ed` = `crates/mn-edit/src/lib.rs`,
`md` = `crates/mn-edit/src/markdown.rs`, `tb` = `apps/mn/src/toolbar.rs`, `pt` = `apps/mn/src/paint.rs`,
`ps` = `apps/mn/src/paste.rs`, `cl` = `apps/mn/src/clip.rs`, `lay` = `crates/mn-layout/src/lib.rs`,
`mda` = `apps/mn/src/media.rs`, `mp` = `crates/min-paths/src/`.

Vocabulary of the old code: a **span** is `[s, e)` over one block's text in UTF-16 units with a kind and, for four
kinds, a payload string called `href`; a **link** is a span of kind `link` whose payload is the target; a **chip**
(inline choice chip) is a span of kind `choice` whose text is the chosen option's label; a **file chip** (also
"attachment chip") is not a span at all but a whole *media block* of kind `file`; a **bookmark** is a canvas element
holding a web address.

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| Three unrelated mechanisms | A link and a choice chip are spans in a text block. A file chip is a media block. Nothing in the page editor refers to a file from *inside* a line of text | `ITh:27-35`, `MS:61-69` |
| "FileLinks" is not a feature | `FileLinks.cpp` is four helpers — reveal in the file manager, open a folder, open with the default app, open in the sibling file browser — and the `ufb:///` link builder. It stores nothing | `FL:22-59` |
| No inline file link | No span kind, no toolbar button and no command makes a reference to a file or folder inside text. A typed or pasted path is plain text. **Not found** | `ITh:27-35`, `Tb:142-200` |
| No attach button | The old toolbar has no paperclip. A file chip is made only by drop, by pasting copied files, by an import that falls back to "attach", or by the Evernote importer | §2.2 |
| Span kinds | 1 bold, 2 italic, 3 code, 4 strike, 5 underline, **6 link**, 7 text colour, 8 highlight, 9 retired (comment), **10 choice**. Numbers are stable; the file stores names | `ITh:27-35`, `IT:17-34` |
| One block, one line of spans | A span never crosses a block. A selection over several blocks is applied block by block | `Ed:2813-2817` |

---

## 1. Links in text

### 1.1 Storage

| Item | Rule | Ref |
|---|---|---|
| Record | In the block's attrs: `spans: [{"s", "e", "k": "link", "u": "<target>"}]`. `u` is written only for payload kinds | `BM:2515-2520`, `BC:48-56`, `IT:236-247` |
| Target | An opaque string. Nothing validates, trims (beyond the dialog's own trim), normalises or parses it at store time | `BM:4940-4942`, `Ed:2803-2804` |
| Label | The covered text. A link has no separate title | `ITh:20-22` |
| Overlap | `setLink` drops **whole** every link span that overlaps the range, then appends the new one. Two adjacent links with the same target are not merged | `BM:4922-4939`; `S1` §1.5 |
| Remove | `setLink` with an empty target: every link span overlapping the range goes, whole | `BM:4931-4933` |
| Lookup | `linkAt(row, col)` / `linkRangeAt(row, col)`: the first link span with `s <= col < e`. A caret at a link's end is **not** in it | `BM:5092-5104` |

### 1.2 Making, editing, removing

| Gesture | What happens | Ref |
|---|---|---|
| Toolbar link button, `⌘K` (Ctrl+K on Windows) | `applyLink()`: opens the URL popup. The target range is snapshotted first, because the popup takes focus | `Tb:164-166`, `Ed:3495`, `Ed:2774-2802` |
| … with a selection | Range = the selection (any number of blocks). The field is prefilled with the link at the selection's **start** only | `Ed:2782-2786` |
| … caret inside a link | Range = that whole link span. Prefilled with its target (edit mode) | `Ed:2788-2793` |
| … bare caret | Insert mode: empty field | `Ed:2794-2799` |
| Popup | 320 px wide, under the caret (centred near the top when the caret's block has no text item), one text field, hint line `↵ apply · esc cancel`, placeholder `https://…  (blank removes the link)`. Enter commits, Esc or a press outside cancels. No label field, no "open", no "remove" button | `Ed:8630-8688` |
| Commit, insert mode | Non-empty: the URL itself is inserted as text at the caret (no typing marks) and linked to itself. Empty: nothing | `Ed:2807-2811` |
| Commit, range mode | For each block of the range `setLink(row, s, e, url)`; an empty URL removes. One undo step (a group over the rows) | `Ed:2803-2820` |
| Clear formatting (`⌘\`) | Removes link spans over the range along with every style span; chips survive | `IT:212-217`, `Ed:3237-3248`, `T:3853` |
| Typing Markdown `[label](url)` | Not live. Converted when the block is **left** (`commitMarkdown`, called on every caret move to another block, before structural commands, before a drop or paste of files) and when a document is **loaded**. Only in paragraph, quote, bullet / numbered and task items | `BM:5193-5206`, `BM:5228-5245`, `BM:2526-2534`, `Ed:1692`, `Ed:6686` |
| … the scanner | `[`, then the first `]`, which must be followed by `(`, then the first `)`. Label = text between the brackets; target = everything between the parentheses, untrimmed. No title syntax, no nested brackets, no escaped parentheses, no `<url>` autolink | `BM:5193-5206` |
| … while still typing | The markers stay literal and unstyled: the live highlighter dims only `` ` ``, `**`, `*` markers | `Hl:66-90` |
| Auto-link of a typed or pasted bare URL | **Not found** in the page editor. (The canvas does turn a dropped / pasted one-line `http(s)` text into a bookmark element, §2.6) | `BM:5674-5800`, `Cv:251-260` |
| Paste of a URL over a selection | No special case: the text route deletes the selection and pastes the text | `Ed:3167-3172`, `Ed:3097-3103` |
| Paste of plain text holding `[label](url)` | Each pasted line goes through the same converter: links are made | `BM:5786-5791` |
| Paste of HTML with `<a href>` | Anchors become link spans with the raw `href`. A single pasted paragraph is spliced inline at the caret with its spans | `Im:718-719`, `BM:5914-5961` |

### 1.3 Drawing, hover, opening

| Item | Rule | Ref |
|---|---|---|
| Look | Underline + the accent colour (`#0189f1`). Under inline code the code colour wins; an explicit text-colour span wins over both; inside a highlight the auto-contrast colour wins; in a table cell with a text colour the cell colour wins. The underline always stays | `Hl:52`, `Hl:123-126`, `Hl:128-140`, `BV:671-675` |
| Where drawn | Paragraph, heading, quote, bullet, task, numbered. A code block does not draw spans | `BV:668-669` |
| Pointer | Stays the I-beam over a link. (The pointing hand is for task boxes, code-block tools, chips and typed cells) | `Ed:6740-6751` |
| Hover | A pill appears above the pointer: `↗` + the full target (middle-elided, at most 380 px wide). Anchored once where the pointer entered the link; stays 500 ms after the pointer leaves so it can be reached | `Ed:6770-6785`, `Ed:8010-8034` |
| Open, way 1 | A click on the pill: `Qt.openUrlExternally(target)` | `Ed:8038-8047` |
| Open, way 2 | Right-click on a link (outside a selection): first menu row `Open <target>` — target shown without `http(s)://` and `www.`, cut to 30 characters | `Ed:6583-6585`, `Ed:8326-8328`, `Ed:3218-3221` |
| Click on link text | Places the caret. Never opens; any press hides the pill | `Ed:6680-6682` |
| `⌘`-click / Ctrl-click | **Not found.** A header comment names it; no handler exists | `BMh:666-668` vs `Ed:6680-6681` |
| Keyboard open (Return on a link) | **Not found** | `Ed:3510` |
| Schemes | Whatever the string is goes to `Qt.openUrlExternally`. No `https://` is added to a bare `www.` or host; no scheme list; `mailto:` and `file:` are neither helped nor refused | `Ed:8046`, `Ed:8328` |

### 1.4 Text edits around a link

| Edit | Rule | Ref |
|---|---|---|
| Typing strictly inside | The link grows | `IT:99-104` |
| Typing at its start | The link shifts right; the new text is not linked | `IT:101` |
| Typing at its exact end | Not linked (no span is extended at its end) | `IT:102`, `ITh:61-62` |
| Deleting across an edge | The link shrinks; an emptied link disappears | `IT:106-118` |
| Replacing exactly the linked run (a spelling fix) | The link keeps covering the replacement | `ITh:71-74`, `T:3713-3719` |
| Return inside a link | Both halves keep a link span with the same target | `BM:6224-6240`, `BM:5807-5811` |
| Toolbar state | The link button is lit when the caret's position is inside a link; with a selection, when its focus end is | `Ed:2652-2654`, `Tb:165` |
| Caret keys | No special handling in or at a link | `Ed:1991-2017` |

---

## 2. File chips

### 2.1 Storage

`Md` §1 has the full media descriptor; this is the file chip's part of it.

| Item | Rule | Ref |
|---|---|---|
| Record | A block of type `media` whose content is JSON `{"src", "kind": "file", "name", "ext"}`. `name` = the file name at insert time, `ext` = its lower-cased suffix | `MS:61-69` |
| `src` | `toRef(absolute path)`: an object `{"vol": <mapping id>, "rel": [segment, …]}` when the path lies under an enabled path mapping's root for this OS, else the absolute path as a string | `MS:64`, `PMh:36-39` |
| Copy policy | Never copied: always referenced where it is, including from a temporary location | `BMh:779`; `Md` §3 |
| Exception | An Evernote attachment's bytes are written into the document's sidecar and `src` is the relative sidecar path | `Im:1004-1020` |
| Folder | No check for a directory: a dropped folder becomes the same record with `ext` usually empty **(inference: neither `insertFileFromUrl` nor `fileDescriptorJson` tests for one)** | `BM:4442-4447`, `MS:61-69` |
| Legacy | A surviving 1.x `sketch` block is laid out and drawn as a file chip | `BM:2678` |

### 2.2 How one is made

| Route | Rule | Ref |
|---|---|---|
| Drop on a page | Each dropped URL goes through `insertMediaFromUrl`: video → PDF → previewable format → loadable image → *importable document or package (raises the import flow, inserts nothing)* → **file chip**. Lands at the drop gap, beside a block (making a column), or in a table cell | `BM:4449-4475`, `Ed:1552-1589` |
| Paste of copied files | The paste router's `urls` action (after our own blocks and HTML, before raster and text): the same function, after the caret's block | `CP:33-47`, `Ed:3061-3070` |
| Import over the table cap | The dialog's "Attach file" appends the file as a chip at the document's end | `Mn:214-223`, `Im:467-469` |
| Evernote import | Non-image resources become file chips | `Im:1004-1020` |
| Toolbar, menu, shortcut | **Not found.** No "insert file" command | `Tb:142-200`, `Mn:1255-1270` |
| Paste of a path as text | Plain text; no chip | `CP:33-47` |

### 2.3 Drawing and actions (page editor)

| Item | Rule | Ref |
|---|---|---|
| Size | Fixed 56 px high; width `min(360, lane width)`, left-aligned in the block | `BM:67`, `BM:3381`, `MB:108` |
| Look | Raised fill, 1 px border, the app radius; a generic `file` icon (24, muted) for every type; two lines: the name (body size, middle-elided; the word `file` when empty) and the resolved local path (small, subtle, middle-elided) | `MB:105-133` |
| Name | The record's `name`; else the last segment of `rel` or of the string `src` | `BM:4600-4613` |
| Click | Selects the block (a media block is "opaque": the caret sits on it, Backspace / Delete remove it, typing starts a paragraph after it) | `Ed:1730-1732`, `Ed:1763-1766` |
| Double-click | Reveal in Finder / Explorer | `Ed:6818-6822`, `ManM:37-38` |
| Return on it | Nothing specific to a file chip **(not found)** | `Ed:3510` |
| Context menu | The media rows: `Reveal in Finder` (`Show in Explorer` on Windows) · `Open in ufb` · `Copy path` (hidden when the source is remote), then the generic block rows (§4.2) | `Ed:8341-8344`, `Ed:8231` |
| Open with the default app | **Not found** on a page. The helper exists and only the canvas calls it | `FL:56-59`, `CD:895` |
| Reveal | No-op when the path is empty or the file does not exist. macOS `open -R`, Windows `explorer /select,` | `FL:26-37` |
| Open in ufb | Builds `ufb:///<win or mac>/<percent-encoded path>` and hands it to the OS; no existence check | `FL:22-24`, `FL:51-54` |
| Copy path | The resolved absolute path of this machine, as text | `Ed:8344` |
| Drag, move, copy | As any media block (`step-block-moves-spec.md`); in the block clipboard the descriptor travels whole | `BM:5970-5975` |

### 2.4 Missing file, unmapped volume

| Case | What shows | Ref |
|---|---|---|
| File gone, path resolvable | The chip is unchanged: name and path, no warning **(inference: the chip binds only the name and the resolved path)** | `MB:43-44`, `MB:122-131` |
| `{vol, rel}` whose volume is unknown or disabled here | The path line is empty; the name still shows (from `name` or `rel`) | `MS:660-667`, `BM:4547-4550`, `BM:4610-4612` |
| Reveal / double-click then | Nothing happens, silently | `FL:26-27` |
| Relink, "locate…", a missing mark | **Not found** | `Md` §2.4 |
| Export | Meta line reads `(unavailable)` in place of the size | `Ex:509-511` |

### 2.5 Cross-OS path mapping

| Item | Rule | Ref |
|---|---|---|
| Mappings | App-wide list (not per document): id, label, a root per OS (Windows, macOS, Linux), enabled. Edited in `Document ▸ Path Mappings…` | `PMh:19-26`, `Mn:1269` |
| Write | `toRef`: under an enabled mapping's current-OS root → `{vol, rel[]}` (separator-free segments); else the absolute string | `PMh:36-39` |
| Read | `{vol, rel}` → this OS's root + native join; empty when the volume is unknown or disabled. A legacy absolute **string** that matches any OS's root of a mapping is rebuilt for this OS; otherwise used as is | `PMh:41-48`, `MS:660-670` |
| One chokepoint | Every reader goes through `MediaStore::resolveUrl(QJsonValue)` | `MS:660-670` |
| Since commit `e545be4` | The classifiers and link builders are calls into the shared `min-paths` crate (`mp_*`) | `FL:17-24`, `MS:312-315` |

### 2.6 The canvas's file, folder and web-link cards (context; not page content)

| Element | Rule | Ref |
|---|---|---|
| File card | Same record; the card shows name + path in the mono face, and the text `(volume not mapped on this machine)` when unresolved; the path as a tooltip | `CFc:1-38` |
| Folder card | A `folder` kind exists only on the canvas (`Open folder`, `Spread files here`, `Refresh`) | `Cv:208`, `Ed:5189` |
| Bookmark card | `{kind: "bookmark", src, host, title, icon, fetched}`; only `http` / `https` accepted; title and favicon fetched in the background | `CD:1181-1194`, `CD:1221-1259` |
| Making a bookmark | `Add link…` on the empty canvas (a URL prompt), or a dropped / pasted one-line `http(s)` text. The prompt prefixes `https://` to a bare `www.` and refuses anything else | `CvM:85`, `Cv:251-260`, `Cv:278-282` |
| Opening | Double-click / Return = open: the default app for a file, the browser for a bookmark. Menu: `Open link`, `Copy link`; for local media `Reveal…`, `Open in ufb`, `Copy path` | `Cv:351-353`, `CvM:65-69`, `Ed:3461` |

---

## 3. Inline choice chips

`S1` §1.6 lists the engine functions with their exact rules; `TC` covers chips as table-cell values. This section is
the chip in running text.

### 3.1 Storage

| Item | Rule | Ref |
|---|---|---|
| Record | A span `{"s", "e", "k": "choice", "u": "<payload>"}`; the covered **text is the chosen option's label** | `ITh:98-101`, `BMh:670-672` |
| Payload | Compact JSON `{"o": [{"id", "l", "c"?}, …], "v": "<chosen id>"}`: `l` label, `c` colour hex (absent = none) | `IT:281-283`, `IT:327-338` |
| Where the options live | **In each chip.** There is no document-level or shared option set: every inline chip carries its own full list, and two chips are unrelated even when made by copy and paste | `BM:4966-4986`, `ChP:49-58` |
| Default set | `To do #8A8A8A`, `Doing #0189F1`, `Done #58A65C`, the first chosen. The three ids are freshly minted for every inserted chip | `BM:4964-4986` |
| No empty state | A chip always has a chosen option; "clearing" one removes it | `ChP:126-129`, `BMh:680` |
| Labels | Stripped of `` ` * _ ~ [ ] \ `` and line breaks, trimmed; empty becomes `Option`. Reason given: the Markdown converter scans a block's raw text and would eat marker characters inside a label | `IT:252-260`, `BM:4956-4960` |
| Address | A chip is addressed by `(row, span start)`; label changes keep the start | `ChP:13-17`, `IT:302-317` |

### 3.2 Inserting

| Route | Rule | Ref |
|---|---|---|
| Toolbar | `Insert block` flyout ▸ `Choice chip  (⌥⌘C)` (icon: a caret in a circle; there is no "tag" icon) | `Tb:196` |
| Shortcut | `⌥⌘C` (the handler also accepts the Option-layer `ç`); Ctrl+Alt+C on Windows | `Ed:3374-3383`, `Mn:1262-1264` |
| Menu bar | `Document ▸ Insert Choice Chip`, enabled when a document is open | `Mn:1262-1264`, `Mn:1410-1411` |
| Context menu | `Insert choice chip` (not on media, not when the click is inside a selection). The caret goes to the clicked block — at its **end** unless the caret was already in that block — then the same command | `Ed:8363-8365` |
| The command | Refused in a full-frame tab (except a table's grid tab) and in a typed table cell. A selection is deleted first. `insertChoiceAt(row, col)`; the caret parks after the chip; the picker opens under the chip at once | `Ed:2977-2995` |
| Where allowed | Any text block except code; not on media or divider. A position strictly inside another chip moves to that chip's end | `BM:4998-5006`, `IT:291-295` |
| Typed trigger | **Not found** (no `/`, no `@`, no `[ ]` shortcut makes one) | — |
| Undo | One step per call | `BM:5008-5018` |

### 3.3 The picker and the options editor

| Item | Rule | Ref |
|---|---|---|
| Open | A click on a chip (the press never places the caret inside; the caret parks after the chip), or right after insertion. Anchored at the click point / under the chip. The pointer is a hand over a chip | `Ed:6669-6679`, `Ed:6746-6750`, `Ed:8116-8121` |
| Keyboard open | **Not found** for an inline chip. (Space opens a typed table cell's picker only) | `Ed:3512-3517` |
| Rows | One per option: colour dot, label, a check on the chosen one. A click chooses it — the chip's text becomes that label — and closes | `ChP:76-117`, `BM:5041-5045` |
| `Remove` | Always shown for an inline chip: deletes the chip and its text | `ChP:119-141`, `BM:5076-5080` |
| `+ New option` field | Enter adds the option with the next palette colour (seven fixed colours, by position), **chooses it**, and closes. In span mode the field is neither a filter nor focused on open (unless there are no options); Up / Down do nothing | `ChP:46`, `ChP:64-65`, `ChP:144-181`, `IT:327-338` |
| `Edit options…` | Opens the modal options editor on this chip's own list (rename, recolour, reorder, delete; `TC` describes it). Commit = `setChoiceOptions`: ids kept, new ids minted, labels sanitised; a deleted chosen option falls back to the first; an empty list removes the chip | `ChP:191-203`, `Ed:8142-8145`, `ChE:65`, `ChE:90`, `BM:5065-5074` |
| Close | Esc or a press outside | `ChP:45` |

### 3.4 Drawing

The label is ordinary block text. Behind it the block draws one rounded rectangle per visual line of the chip's range:
the chosen option's colour at 0.28 alpha, a 1 px border at 0.55 alpha, the small radius, 4 px wider than the text on
each side. The pill reserves no width, so the padding rides under adjacent glyphs. No colour → the muted text colour
is used as the tint. Style spans may overlap a chip and style its label (`BV:390-426`).

### 3.5 Caret, selection and edits around a chip

| Edit | Rule | Ref |
|---|---|---|
| Right / Left | Stepping into a chip hops to its far edge (with Shift, the selection takes the whole chip) | `Ed:1991-2012` |
| Backspace just after, or anywhere in, a chip | Removes the whole chip and its text; the caret goes to its start | `Ed:1734-1741` |
| Delete at its left edge or inside | The same | `Ed:1751-1758` |
| Typing with the caret strictly inside | The caret first hops to the chip's end; the text lands after it | `Ed:1768-1771` |
| Typing at either edge | Outside the chip (start: the chip shifts; end: not extended) | `IT:99-104` |
| Return strictly inside | The split point moves to the chip's end | `BM:6232-6235` |
| Paste of blocks strictly inside | The split point moves to the chip's end | `BM:6787-6789` |
| Clear formatting | The chip survives | `IT:212-217`, `T:3881` |
| Where atomicity lives | In these callers only; the span engine itself will grow or shrink a chip (`S1` §1.6) | `IT:99-118` |
| Mouse drag, Up / Down or a hit-test landing inside a chip | Not specially handled in the code read; the next key applies the rules above | `Ed:1928` |

### 3.6 Copy, cut, paste

| Flavour | Rule | Ref |
|---|---|---|
| The app's block flavour | Spans travel with their payload (`"u"`), so a wholly selected chip arrives as a chip with its own option list. A chip the selection cuts through is dropped as a chip — its selected text travels as plain text | `BC:48-56`, `BM:6557-6565`, `T:3140-3147` |
| Plain text | The label | `BM:6573` |
| HTML to other apps | Written only when the range holds a table or a columns row (the page walker's fragment, chip as a tinted `span`). A plain text range puts **no HTML** on the clipboard — a link's target is lost to other apps, too | `Ed:2958-2972` |
| Paste from outside | Nothing recreates a chip from HTML or Markdown. **Not found** | `Im:700-730` |
| Tab merge | Rows are copied as specs with spans and payloads; chips survive | `BM:5963-5987`, `T:2733-2781` |

---

## 4. Toolbar, menus, shortcuts

### 4.1 Toolbar (page mode)

| Button | Label / tooltip | Enabled | Lit | Ref |
|---|---|---|---|---|
| Link | `Link  (⌘K)` | Always (page mode) | Caret inside a link | `Tb:164-166` |
| Choice chip | In the `Insert block` flyout: `Choice chip  (⌥⌘C)` | Always; the command itself refuses in a typed cell or a frame tab | Never | `Tb:186-199`, `Ed:2977-2982` |
| Clear formatting | `Clear formatting  (⌘\)` — removes links | Always | — | `Tb:167-168` |
| Attach / file | **Not found** | — | — | `Tb:142-200` |

The link button calls `applyLink` directly rather than through the bar's refocus wrapper, so the popup's field keeps
focus; the chip entry is flagged `noRefocus` for the same reason (`Tb:166`, `Tb:196`).

### 4.2 Context menu (page)

| Row | Shown when | Action | Ref |
|---|---|---|---|
| `Open <target>` | The right-click is on a link and not inside a selection. First row of the menu (the "primary" section) | Opens externally | `Ed:8326-8328` |
| `Reveal in Finder` / `Show in Explorer` | On any media block, file chips included | Reveal | `Ed:8341-8342` |
| `Open in ufb` | On any media block | The `ufb:///` deep link | `Ed:8343` |
| `Copy path` | On a media block whose source is not `http(s)` | Path as text | `Ed:8344`, `Ed:8231` |
| `Insert choice chip` | Not a frame tab, not media, not inside a selection | §3.2 | `Ed:8363-8365` |
| Edit link, Remove link, Copy link | **Not found** in the page menu (`Copy link` exists for canvas bookmarks) | — | `CvM:69` |
| A chip's own menu | **Not found**: a right-click on a chip gives the block's menu; the chip's actions are all in its picker | `Ed:6577-6589` |

### 4.3 Shortcuts

| Keys | Command | Ref |
|---|---|---|
| `⌘K` | Link popup | `Ed:3495`, `Sh:92` |
| `⌥⌘C` | Insert a choice chip | `Ed:3374-3383`, `Sh:77`, `ManE:115-119` |
| `⌘\` | Clear formatting | `Tb:167` |
| Enter / Esc in the link popup | Apply / cancel | `Ed:8671-8672` |
| Enter in the picker's field | Add and choose the option | `ChP:157-181` |

---

## 5. Export and import mapping

| | Link | Inline choice chip | File chip |
|---|---|---|---|
| **Markdown out** | `[label](target)`; the target in `<…>` when it holds a space or a parenthesis. Outermost of nested marks (`Ex:160-190`, `Ex:107-113`) | The label as plain text; colour and options dropped (`Ex:219-220`) | A fenced block: info string `file`, then name, path, size or `(unavailable)`; with the option on, a `<ufb:///…>` autolink line after it (`Ex:509-515`) |
| **HTML out** | `<a href>` (`Ex:921-922`) | `<span class="chip">` with the colour at 0.28 alpha, a neutral ground when colourless (`Ex:933-941`, `Ex:1422`) | A `figure` with name, path, `kind · meta`, optional `a.ufblink` (`Ex:1166-1172`) |
| **DOCX out** | A real `w:hyperlink` with an external relationship; blue `#0563C1`, underlined (`Ex:2259-2270`, `Ex:2288`, `Ex:2904`) | A shaded run: the colour mixed 28 % over white, `#EFEFEF` when colourless (`Ex:97-105`, `Ex:2291-2292`) | Bold name, mono path, optional `ufb` hyperlink, mono meta (`Ex:2405-2433`) |
| **PDF out** | An anchor (clickable annotation), accent colour, underlined (`Ex:3085-3089`) | The same shaded run (`Ex:3096-3097`) | The same reference lines; the `ufb` line clickable (`Ex:3187-3207`) |
| **ODT out** | No ODT exporter (**not found**) | — | — |
| **Copy as Markdown** | As Markdown out | Label | As Markdown out |
| **Markdown in** | Through Qt's Markdown reader (GitHub dialect) → anchors → link spans (`Im:179`, `Im:718-719`); bare URLs presumably auto-linked by that dialect **(inference; not in this repo)** | None | None |
| **HTML in / HTML paste** | `<a href>` → link span, `href` kept as written (`Im:718-719`) | None (a `span.chip` comes back as plain text) | None |
| **DOCX in** | `w:hyperlink` resolved through the relationships → link span (`Dx:170-231`) | None | None |
| **ODT in** | `text:a` `xlink:href` → link span (`Od:84-145`) | None | None |
| **Evernote in** | As HTML | None | Attachments → file chips copied into the sidecar (`Im:1004-1020`) |
| **Spreadsheet in** | Cell hyperlinks: **not found** | Choice / check **columns** only (`TC` §7) | Over the cell cap: "Attach file" (`Im:467-469`) |

The `ufb` link line is a persisted export preference, off by default (`Mn:902-904`, `ManX:71-73`).

---

## 6. What the new app already has

| Area | State | Ref |
|---|---|---|
| Marks | `Mark::Link(String)` and `Mark::Choice(String)` (the payload JSON, verbatim) exist beside the style marks; value-carrying kinds never overlap themselves; equal neighbours join | `inl:6-33` |
| Link span rule | `put` **clips** older links around the new range (the old `setLink` dropped them whole) | `inl:68-73`; `S1` §1.5 |
| Chip rule in the engine | A chip is whole or not a chip: an edit that reaches into one removes its chip-ness (the text stays). `chip_at` (`start <= at < end`), `snap_out_of_chip` (strictly inside → its end). A split never cuts one | `inl:92-95`, `inl:99-122`, `inl:199-211` |
| Editing | Every caret position, split and insertion is snapped out of a chip; a deletion that touches a chip takes it whole; `⌘\` clears links and keeps chips | `ed:93-98`, `ed:125`, `ed:293`, `ed:348`, `ed:442-443` |
| Markdown as you type | `[label](url)` converts when its closing `)` is typed, as its own undo step (undo gives the characters back). Not on paste | `ed:305-312`, `md:76`, `ed:606-609` |
| Storage | `spans` are read and written with kinds `link` and `choice` and their values, UTF-16 offsets as in the old file | `inl:25-30`, `crates/mn-store/src/codec.rs:47`, `:83-86` |
| Painting a link | Underline + the link colour on the text run (layout), and an underline stroke in the link colour (paint) | `lay:232`, `pt:21`, `pt:286-290` |
| Painting a chip | A filled rounded pill behind the label in the chosen option's colour — in a choice column the colour comes from the column's list, else from the chip's own payload | `pt:263-277` |
| Table chips | Built: choice / check cells, the picker with filter and options editor (`apps/mn/src/choice.rs`), the column's list as the truth with a copy in every chip | `crates/mn-edit/src/typed.rs:2-3`, `TC` |
| Toolbar | `Link` (`⌘K` in its tooltip), `File` (paperclip) and `Chip` (tag, tooltip `Choice`) are in the band but **disabled**: `built()` is false for exactly these three | `tb:29`, `tb:37`, `tb:42`, `tb:128` |
| Paste in | HTML `<a href>` → `Mark::Link` (empty, `#…` and `javascript:` targets refused; the anchor's own underline dropped). Markdown `[label](url)` and `<http(s)://…>` → links. Nothing makes a chip | `ps:82`, `ps:108-109`, `ps:243-254` |
| Copy out | HTML flavour for every text range: links as `<a href>`; a chip contributes no markup (its label is plain text) | `cl:50-58` |
| File chips | A dropped or picked file that is not a readable picture or preview format becomes a `file` media record, referenced in place; a folder is refused with a notice. The picker is reached through the **Picture** tool; the paperclip does nothing yet | `mda:394-399`, `apps/mn/src/main.rs:821-822` |
| `min-paths` | `map` (`Ref::{Vol, Abs}`, `to_ref`, `resolve_ref`, `translate`), `classify` (network / volatile source), `links` (`ufb_link`, `minnotes_link`, QCView links, with parsers), `url` (file URLs), `clipboard` (copy / paste paths, `looks_like_path`), `volume::external_path`, `names::web_link_file_name`. No "FileLinks" type; no reveal / open-with helper in the crate | `mp/map.rs:31-155`, `mp/links.rs:10-59`, `mp/clipboard.rs:32-70`, `mp/volume.rs:89` |

Stubs and gaps on the new side: no link dialog, no hover or open gesture, no opening of any URL or file anywhere in
`apps/mn` (searched: no opener call); no inline-chip insertion, no picker for a chip outside a table; no file-chip
actions (reveal, open, copy path), and the chip's drawing is the media step's placeholder (`mda:25`).

---

## 7. Open questions for the redesign

| # | Question | Old behaviour | Ref |
|---|---|---|---|
| Q1 | What opens a link? | Only the hover pill and the context-menu row; a click edits, no modifier-click, no key. Reachable by mouse only | §1.3 |
| Q2 | Do typed or pasted bare URLs become links? | No on a page; yes (as a bookmark) on the canvas | §1.2, §2.6 |
| Q3 | Is a target normalised or checked (bare `www.`, missing scheme, `mailto:`, `file:`, a filesystem path, the family's `minnotes:` / `ufb:` / `qcview:` schemes)? | Stored and opened as typed; the canvas prompt alone adds `https://` to `www.` | §1.3, §2.6 |
| Q4 | Does pasting a URL over a selection link the selection? | No: it replaces the text | §1.2 |
| Q5 | Does the link editor edit the label, and does it offer Open / Copy / Remove? | One field; blank = remove; a bare caret inserts the URL as its own label | §1.2 |
| Q6 | A link over part of an older link: clip the old one (new engine) or drop it whole (old)? | Dropped whole | §1.1, §6 |
| Q7 | Links in headings and code blocks | Stored anywhere `⌘K` is pressed, drawn everywhere but code blocks; Markdown conversion only in four block kinds | §1.2, §1.3, §8 #5 |
| Q8 | Is a "file chip" in this step still a **block**, or does the step add an inline file reference (a span or an inline atom with a `{vol, rel}` target)? | Block only; no inline form exists to port | §0, §2.1 |
| Q9 | What does the paperclip do: a file dialog making block chips, and does it take folders? | No button existed; a folder dropped on a page became a file chip by accident, the canvas has a real folder card, the new app refuses folders | §2.1, §2.2, §6 |
| Q10 | Chip gestures: is double-click reveal or open? Is there an Open (default app) row on a page? | Double-click reveals; open-with exists only on the canvas | §2.3, §2.6 |
| Q11 | The missing / unmapped state of a file chip, and relink | No state, an empty path line, silent no-op actions | §2.4 |
| Q12 | One icon for every file type, or per type? Show the size? | One generic icon, name + path only | §2.3 |
| Q13 | Where do an inline chip's options live: in each chip (old), in named sets per document, or shared with a table column? | Per chip; copies drift apart; ids are new per insert | §3.1 |
| Q14 | What is the default set, and is an inline To do / Doing / Done chip the same thing as a check cell or a task item's state? | Three fixed options with fresh ids; unrelated to both | §3.1 |
| Q15 | Chip atomicity on a partial edit: remove the chip with its text (old editor) or keep the text and drop the chip-ness (new engine's fallback)? | Whole removal by Backspace / Delete; typing hops out | §3.5, §6 |
| Q16 | Keyboard access to a chip: select it as an object, open its picker (Space / Return), arrow through options, type-to-filter | None for inline chips | §3.3 |
| Q17 | Do chip labels still need sanitising once Markdown converts only at typing time? | Marker characters stripped because load and commit rescan raw text | §3.1 |
| Q18 | Insertion entry points: toolbar tag button only, or also a typed trigger and the context menu? Where does the menu's chip land? | Flyout entry, shortcut, menu bar, context menu (lands at the block's end) | §3.2 |
| Q19 | Should a chip survive a round trip through HTML or Markdown (copy to another app and back, export and re-import)? | No: it flattens to its label everywhere but the app's own clipboard flavour | §3.6, §5 |
| Q20 | How does a link read inside a coloured cell, a text-colour span or a highlight? | Colour is overridden; only the underline says "link" | §1.3 |
| Q21 | The `ufb` deep link: stays an export option and a menu row, or becomes a link scheme the editor understands? | Menu row on every media block; export option off by default | §2.3, §5 |

---

## 8. Oddities and accidents

| # | What | Why it looks accidental | Ref |
|---|---|---|---|
| 1 | A header comment promises "Cmd/Ctrl-click open + hover" | The editor says the opposite and has no such handler | `BMh:666-668`, `Ed:6680-6681` |
| 2 | The page link popup stores `www.example` as typed; the canvas prompt fixes it | Two URL entry points, two rules; the page one produces targets the OS may not open | `Ed:2803-2811`, `Cv:278-282` |
| 3 | With a selection, the popup is prefilled from the selection's start only | A selection that starts on plain text and runs into a link opens empty, and Enter on the empty field removes that link | `Ed:2786`, `Ed:2816` |
| 4 | A caret at a link's end is not "in" it | `⌘K` there inserts a second link instead of editing the one just made | `BM:5095`, `Ed:2788-2799` |
| 5 | `⌘K` over a multi-block selection links every row, code blocks included | A code block stores the span and never draws it | `Ed:2813-2817`, `BM:4924`, `BV:668` |
| 6 | Markdown links convert on leaving the block and on every load | Literal `[a](b)` text cannot be kept in a paragraph; the first `)` ends the target, so a URL with parentheses is cut | `BM:5193-5206`, `BM:2526-2534` |
| 7 | The link's Markdown markers are not dimmed while typing, the style markers are | The highlighter never learnt links | `Hl:66-90` |
| 8 | Copying a linked phrase out of a plain paragraph loses the target | No HTML flavour unless the range holds a table or columns | `Ed:2958-2972` |
| 9 | The hover pill freezes where the pointer entered the link and lives 500 ms on a timer | A workaround for the central mouse layer; three handlers keep it alive | `Ed:6776-6784`, `Ed:8040-8045` |
| 10 | `setLink` drops overlapping links whole; the typing-colour path clips | Two payload rules in one model | `S1` §1.5 |
| 11 | Every inline chip repeats its whole option list and mints new ids | The table shape reused without a place to hold a list | `BM:4966-4986` |
| 12 | A new option's colour comes from a seven-colour palette unrelated to the default set's colours | Two colour sources | `ChP:64-65`, `BM:4969-4971` |
| 13 | The chip's pill is an overlay 8 px wider than its text | "Overlays can't reserve layout width": it rides under neighbouring glyphs | `BV:390-426` |
| 14 | Chip atomicity is enforced by the editor's key handlers, not the engine | Any other writer (replace, spell fix, a selection ending inside) can leave text that no longer equals the label | `S1` §1.6, `Ed:1734-1771` |
| 15 | The inline picker has an add field but no filter, no arrow keys and no focus | Those were added for table cells only | `ChP:26-29`, `ChP:46`, `ChP:155-156` |
| 16 | The context menu's `Insert choice chip` puts the chip at the end of the clicked block | The menu does not know the click's column | `Ed:8365` |
| 17 | A chip cannot go in a code block but can go in a heading | The gate lists media, divider, code | `BM:5001` |
| 18 | The page file chip shows an empty path line for an unmapped volume; the canvas card says so in words | Two drawings of one record | `MB:128`, `CFc:28` |
| 19 | `Open in ufb` is offered on every media block, remote ones too, with no check | Only `Copy path` is gated on a local source | `Ed:8343-8344` |
| 20 | A dropped folder becomes a file chip | No directory test on the fallback | `BM:4442-4447` |
| 21 | Double-click on a file chip reveals; on the canvas it opens | The page gesture predates the "double-click = open" ruling | `Ed:6818-6822`, `Cv:351-353` |
| 22 | A surviving `sketch` block draws as a file chip | A stand-in after sketches were retired | `BM:2678` |

---

## 9. Not read

- `ChoiceColumnEditor.qml` beyond its span-mode entry and commit (`ChE:47-90`); `TC` describes the editor.
- The mouse drag-selection path and Up / Down navigation with respect to a chip's interior; `deleteSelection` over a
  partly covered chip.
- Search and spell-check behaviour inside links and chips (`SearchEngine.cpp`, `app/spell/`).
- The canvas exporter's treatment of bookmark, file and folder cards (`CanvasExporter.cpp`); `CanvasFolderCard.qml`;
  `CanvasBookmarkCard.qml` beyond its layout.
- `PathMap.cpp`, `PathMapController.cpp` and the mappings dialog (only `PathMap.h`'s contract was read).
- `MediaStore::resolvePath` for string sources (existence checks, sidecar-relative paths); `Md` §2 covers it.
- The package exporter and Collect Media with respect to file chips.
- Qt's own Markdown and HTML readers (what they do with autolinks, titles, relative targets).
- On the new side: `apps/mn/src/main.rs` beyond the lines cited, `crates/mn-edit/src/markdown.rs` beyond the link rule,
  and `crates/mn-doc/src/legacy.rs`.
- Local notes of the old project (plans, design records): not consulted for this spec.
