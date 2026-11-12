# minNotes media blocks — what the Qt app did (Phase 3, media step)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`, the tree as checked out),
read only. Nothing in it was built or run; every statement is read from code or from the app's own manual, and anything
deduced rather than read is marked **(inference)**. `main` (`adbb0d6`) differs from this tree in the media code in one
respect only: on `main` the source classifiers and the path-mapping rules are C++ inside `MediaStore.cpp` /
`PathMap.cpp`; on `min-paths` the same rules are calls into the `min-paths` crate. The rules are the same; §3.2 quotes
both. Line numbers are those of `59d1129`.

References are `alias:line` relative to the minNotes repository.
`MS` = `app/core/MediaStore.cpp`, `MSh` = `app/core/MediaStore.h`, `BM` = `app/core/BlockModel.cpp`,
`BMh` = `app/core/BlockModel.h`, `PR` = `app/core/PreviewRender.cpp`, `PRh` = `app/core/PreviewRender.h`,
`PP` = `app/core/PreviewPsd.cpp`, `PM` = `app/core/PreviewRender_mac.mm`, `AT` = `app/core/AssetTransfer.cpp`,
`ATh` = `app/core/AssetTransfer.h`, `PE` = `app/core/PackageExporter.cpp`, `PEh` = `app/core/PackageExporter.h`,
`PMh` = `app/core/PathMap.h`, `PMc` = `app/core/PathMap.cpp`, `Canvas` = `app/core/CanvasDocument.cpp`,
`Imp` = `app/core/Importer.cpp`, `CP` = `app/core/ClipboardPaster.cpp`, `Cb` = `app/core/Clipboard.cpp`,
`MB` = `app/qml/MinNotes/App/MediaBlock.qml`, `BV` = `app/qml/MinNotes/App/BlockView.qml`,
`Ed` = `app/qml/MinNotes/App/Editor.qml`, `Main` = `app/qml/MinNotes/App/Main.qml`,
`LB` = `app/qml/MinNotes/App/Lightbox.qml`, `Man` = `docs/media.md`.
New-side references are relative to the minplatform repository: `map.rs` = `crates/min-paths/src/map.rs`,
`classify.rs` = `crates/min-paths/src/classify.rs`, `min-image` = `crates/min-image/src/lib.rs`.

Scope: the media block inside a **page**. The canvas places the same descriptors as elements; it is quoted only where
it shows the same rule with a different constant. Section 5.2 of `step1-core-spec.md` lists the descriptor kinds; this
document supersedes it for media and settles two of its "not determined" notes (§1.5, §2.2).

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| Where a media block's data lives | The block's `content` column holds one compact JSON object, the **descriptor**. Bytes are never in the database | `BMh:748-750`, `MSh:10-16` |
| Block attrs | A media block's `attrs` carries only `cell` (its lane). Width, kind, ink and everything else are in the descriptor | `step1-core-spec.md` §5.1 |
| One parser | `fillMediaMeta` is the only place a row's media geometry is derived from the descriptor; load, insert, undo and redo all pass through it | `BM:2667-2686`, `BMh:1059-1063` |
| Known geometry | A media block's height is a pure function of the descriptor's `w` / `h` / `dw` and the lane width. It is reserved before anything loads and is never measured back from the rendered item | `BM:3380-3396`, `MB:7-8`, `BV:467-471` |
| Opaque | A media block holds no text; a selection takes it whole or not at all | `BMh:1050`, `step-block-moves-spec.md` §1.3 |
| Sidecar folder | `<document folder>/.minnotes/`, created on first use | `MS:300-304` |
| Anchor | The store is anchored to the folder of the **original** file, not to the working copy the app edits. An untitled document and an opened package are anchored to a scratch folder; Save As copies `.minnotes/` beside the new file and re-anchors | `Canvas:236`, `Canvas:288`, `Canvas:436-475`, `Canvas:50-54` |
| New-side state | `Kind::Media { descriptor: String }` carries the JSON untouched in both the legacy reader and the store codec | minplatform `crates/mn-doc/src/tree.rs:18-19`, `crates/mn-doc/src/legacy.rs:38`, `crates/mn-store/src/codec.rs:22`, `:69` |

---

## 1. The descriptor as stored

### 1.1 Keys

The kind is decided by `kind`; an object **without** `kind` is an image (`BM:4773-4779`).

| Key | JSON type | Meaning | Kinds | Ref |
|---|---|---|---|---|
| `src` | string **or** object (§2) | Where the bytes are | all | `MS:43`, `:54`, `:64`, `:72` |
| `w` | integer | Intrinsic width. Image: pixels. Video: coded width. PDF: page 0 width in PDF points, rounded | image, preview, video, pdf | `MS:44`, `:55`, `:73`, `:259-260`, `:281` |
| `h` | integer | Intrinsic height, same units | image, preview, video, pdf | as above |
| `kind` | string | `"video"`, `"pdf"`, `"file"`, `"audio"`, `"folder"`, `"bookmark"`, `"sketch"`; absent = image | all but image | `MS:46`, `:57`, `:65`, `:88`, `:159` |
| `name` | string | Display name. Image: the source file's name, written only when the image came from a file (a copied image's `src` is a hash). Preview: the original's file name. File / audio / folder: the file or folder name | image (optional), preview, file, audio, folder | `MS:75`, `:200`, `:66`, `:160`, `:89`, `MSh:19-21` |
| `ext` | string | Lower-case suffix without the dot | file | `MS:67` |
| `durMs` | integer | Duration in milliseconds | video, audio | `MS:47`, `:161` |
| `frames` | integer | Frame count (the container's, else `fps × duration` rounded) | video | `MS:48`, `:290-293` |
| `fps` | number | Average frame rate, else the real base rate; 0 when unknown | video | `MS:49`, `:283-285` |
| `pages` | integer | Page count (≥ 1) | pdf | `MS:58`, `:255-256` |
| `sampleRate`, `channels` | integer | Audio stream properties | audio | `MS:162-163` |
| `count`, `countCapped`, `entries`, `scanned` | integer, `true`, array of ≤ 6 strings, integer ms | A folder snapshot; `count` stops at 10000 | folder | `MS:83-93` |
| `preview` | object | Marks a **preview** image (§4.3): `{"src": <ref of the original>, "format": string, "mtime": number (ms since epoch), "size": number (bytes)}` | preview | `MS:201-206`, `MSh:42-45` |
| `dw` | integer 80…65535 | Per-block display width in page px; absent = default width (§5) | written for images only; honoured for any kind | `BM:5380-5385`, `BM:2680` |
| `ink` | object | Annotation ink per page: `{"<page>": {"version": "2.0", "coordinate_system": "normalized", "shapes": […]?, "texts": […]?}}`; page `"0"` for an image. A page with neither shapes nor texts loses its key; an empty `ink` leaves the root | image, preview, pdf | `BM:4622-4659`, `BMh:757-764` |
| `images` | array of objects with `src` | A 1.x sketch's embedded pictures | sketch | `AT:46-52`, `BM:4500-4509` |

A reader must tolerate unknown keys: `refreshedPreviewJson` and `setMediaWidth` both rewrite the object by inserting
into the parsed whole, so keys they do not know survive (`MS:232-235`, `BM:5382-5386`). The exception is §7.4.

### 1.2 Which kinds can be a page block

| Kind | Made by the page model | Ref |
|---|---|---|
| image, preview, video, pdf, file | Yes — `insertMediaFromUrl` and the paste and import paths | `BM:4449-4475` |
| remote image | Yes — HTML paste and HTML / Markdown import | `Imp:703-708`, `BM:6181-6209` |
| sketch | Not made any more. A 1.x file's sketch blocks are converted to canvas elements when the file is opened; one that survives is laid out and drawn as a file chip | `BM:2678`, `app/core/Converter.cpp:121-164` |
| audio, folder, bookmark | No builder is called from `BlockModel.cpp`; they are made only by the canvas (`Canvas:992-1037`). A page block with one of these kinds would be treated as an image with `w` = `h` = 0 **(inference from `BM:2676-2679`: only `video`, `file`, `sketch`, `pdf` are tested)** | `BM:2667-2686` |

### 1.3 Examples (synthetic)

Image, copied into the sidecar, resized by the user:

```json
{"src":".minnotes/3f786850e387550fdab836ed7e6dc881de23001b.jpg","w":2400,"h":1600,"name":"storefront-wide.jpg","dw":520}
```

Image, pasted raster (no `name`):

```json
{"src":".minnotes/89e6c98d92887913cadf06b2adb97f26cde4849b.png","w":1280,"h":720}
```

Image, referenced in place on a mapped share:

```json
{"src":{"vol":"01JB2M7Q4T8XK3N5R9W6YHZC0D","rel":["reference","boards","mood-board-03.png"]},"w":3840,"h":2160,"name":"mood-board-03.png"}
```

Image, referenced in place on an unmapped share (machine-local absolute path):

```json
{"src":"/Volumes/shared/reference/mood-board-03.png","w":3840,"h":2160,"name":"mood-board-03.png"}
```

Remote image, before the download lands (placeholder size):

```json
{"src":"https://example.com/pictures/diagram.png","w":480,"h":300}
```

Preview (a Photoshop file on a mapped share; the picture is the rendered PNG in the sidecar):

```json
{"src":".minnotes/a94a8fe5ccb19ba61c4c0873d391e987982fbbd3.png","w":2048,"h":1152,"name":"layout-v4.psd","preview":{"src":{"vol":"01JB2M7Q4T8XK3N5R9W6YHZC0D","rel":["design","layout-v4.psd"]},"format":"PSD","mtime":1761600000000,"size":48211200}}
```

Video (referenced in place):

```json
{"src":{"vol":"01JB2M7Q4T8XK3N5R9W6YHZC0D","rel":["renders","shot-010-v003.mov"]},"w":1920,"h":1080,"kind":"video","durMs":8342,"frames":200,"fps":23.976}
```

PDF, with ink on its second page:

```json
{"src":"/Volumes/shared/docs/schedule.pdf","w":612,"h":792,"kind":"pdf","pages":14,"ink":{"1":{"version":"2.0","coordinate_system":"normalized","shapes":[{"…":"…"}]}}}
```

File chip:

```json
{"src":"/Volumes/shared/delivery/archive.zip","kind":"file","name":"archive.zip","ext":"zip"}
```

Sketch (1.x leftover; only the keys the 2.0 code reads are shown — the full 1.x key set was not read):

```json
{"kind":"sketch","images":[{"src":".minnotes/0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33.png"}]}
```

### 1.4 Key order and number forms

Descriptors are written by `QJsonDocument::toJson(Compact)` from a `QJsonObject`, which emits keys in **sorted** order,
so a stored descriptor reads `{"h":…,"src":…,"w":…}` (the new side's own fixture has this order,
minplatform `crates/mn-doc/src/legacy.rs:230`). The examples above are in reading order, not stored order. `durMs`,
`preview.mtime` and `preview.size` are written as doubles holding integers (`MS:161`, `:204-205`).

### 1.5 Settled from step 1

`step1-core-spec.md` §5.2 left the `{vol, rel}` decision "not determined": it is in §2.2 below, read from
minplatform's own `min-paths` crate, which the Qt tree now calls. Its second open note (which kinds are page blocks) is
§1.2.

---

## 2. Source reference forms

### 2.1 The four forms of `src`

| Form | Written when | Ref |
|---|---|---|
| String starting `.minnotes/` | The bytes were copied or rendered into the sidecar. Relative to the document's folder. File name is `<sha1 hex>.<ext>` (§3.3) or, after Collect Media / a cross-document paste, a readable basename (§3.5, §3.6) | `MS:342`, `:363`, `:384`, `PE:66-77` |
| Object `{"vol": "<mapping id>", "rel": ["segment", …]}` | The file is referenced in place and its absolute path lies under an enabled path mapping's root for the writing OS. `rel` = the path below the root as separator-free segments | `PMh:6-15`, `PMc:47-50`, `map.rs:66-72` |
| String holding an absolute path | Referenced in place, under no mapping. In the writing machine's form | `PMc:47-50`, `map.rs:71` |
| String starting `http://` or `https://` | A remote image not yet downloaded | `MS:96-102`, `MS:655-656` |

Every descriptor builder passes its path through `toRef`; a `.minnotes/…` or `http` string matches no root and is
returned unchanged (`BM:4350-4353`).

### 2.2 `toRef` and `resolveRef` (the mapping rules)

A mapping row is `{id, label, win, mac, lin, enabled}`; the list is application-wide, not per document (`PMh:19-33`).

| Rule | Ref |
|---|---|
| `toRef`: the **first** enabled mapping, in list order, whose root for the current OS contains the path wins; none → the absolute string | `map.rs:66-72` |
| A root matches only on a component boundary (`/Vol/share` never matches `/Vol/share-extra`) | `map.rs:48-56` |
| Comparison treats `\` as `/` on every OS and is case-insensitive on Windows only; the stored path is never rewritten | `map.rs:36-37` |
| Segmenting: Windows splits on both `\` and `/`; POSIX on `/` only (a literal `\` stays inside its segment). Empty segments are dropped | `map.rs:41-46` |
| `resolveRef` of an object: the mapping with that id; unknown id, disabled row or empty root for this OS → no path. Otherwise root (trailing separators trimmed, `\` → `/`) + `/` + segments joined by `/`, on every OS | `map.rs:83-90`, `map.rs:74-78` |
| `resolveRef` of a string: every enabled mapping × every **other** OS's root is tried; a match is rebuilt under this OS's root; no match → the string unchanged. This gives old absolute-path documents cross-OS resolution | `map.rs:91-104`, `PMh:42-47` |

### 2.3 How `resolveUrl` decides

`MediaStore::resolveUrl(QJsonValue)` is the one chokepoint from a stored `src` to something loadable (`MS:660-670`).

| Step | Input | Result | Ref |
|---|---|---|---|
| 1 | object | `resolveRef`; empty → **empty URL**; else a `file://` URL of the path. The file's existence is not checked | `MS:661-666` |
| 2 | string | first through `resolveRef` (the legacy cross-OS swap), then: | `MS:669` |
| 2a | empty | empty URL | `MS:654` |
| 2b | `http://` / `https://` | returned as is (loaded directly until localized) | `MS:655-656` |
| 2c | starts `.minnotes/` | `<document folder>/<src>` as a `file://` URL | `MS:565-566`, `:657` |
| 2d | anything else | treated as an absolute path → `file://` URL | `MS:578`, `:657` |

There are two variants. The **blocking** one (`resolveUrl`) is for file operations: for a document opened from a
package whose file is still inside the archive it extracts the entry before returning (`MS:564-579`). The
**non-blocking** one (`resolveUrlAsync`) is for display: in the same case it starts a background extraction and returns
empty (`MS:581-595`, `:637-651`). Display bindings use the non-blocking one (`BM:4539-4545`, `MB:39-45`). Packages are
out of scope for this step; the split matters because it is the only "pending" state the image path has.

`anchorPathFor` returns the path a `src` would occupy with no IO at all; it is the stable key for per-file session
state (a PDF's current page, a video's playhead) and for the `.qcview` annotation folder (`MS:630-635`, `MSh:121-124`).

### 2.4 A missing file

| Case | What the user sees | Ref |
|---|---|---|
| `{vol, rel}` whose volume is unknown or disabled | Empty URL → the placeholder with the words "image unavailable" (§6.4) | `MS:662-665`, `MB:217-228` |
| A path that resolves but the file is gone | The image loader reports an error → the same placeholder, same words | `MB:224-225` |
| A preview whose **original** is gone | The rendered PNG still shows. The pill does not read "changed" (a missing source is "unavailable, not stale") | `MS:217-225` |
| A preview whose rendered PNG is gone | "image unavailable"; "Refresh preview" re-renders from the original if that still exists | `MS:226-236`, `Ed:8335` |
| Remote image whose download fails | The block keeps the remote `src` and the 480 × 300 placeholder size; no retry is scheduled | `BM:6198-6199` |

There is no relink command, no "locate file" dialog, and no marker in the descriptor that a file was found missing.
The block keeps its reserved size (from `w` / `h`) whether or not the file loads. Changing the path mappings calls
`refreshMedia`, which makes every media binding re-resolve (`BM:4584-4588`, `BMh:827-829`).

---

## 3. The copy policy

### 3.1 The rule for an image file (`MediaStore::importFile`, `MS:318-354`)

```
copy  ⇔  forceCopy  ∨  ¬isNetworkSource(path)  ∨  isVolatileSource(path)
```

| Source | Outcome | Ref |
|---|---|---|
| Any local file | **Copied** into `.minnotes/`, raw bytes, no re-encode (a JPEG stays a JPEG) | `MS:326-351` |
| A file on a network share | **Referenced in place** (`src` = absolute path → `toRef`) | `MS:353` |
| A file on a share, but under a volatile root | Copied | `MS:334` |
| A file on a share, pasted with `forceCopy` | Copied | `MS:334`, `MSh:62-65` |
| The source cannot be opened for reading, or the copy cannot be written | Falls back to a reference in place | `MS:336`, `:348`, `:353` |

Ruling dated 2026-08-19 in the code: shares are "the shared source of truth, can be huge, travel cross-OS via
`{vol,rel}`"; they are the only sources still referenced in place (`MS:326-333`). The manual states the same to the
user (`Man:46-52`).

### 3.2 The classifiers

| Classifier | Rule | Ref |
|---|---|---|
| Network (Windows) | A UNC path (`//` or `\\` prefix), or a drive letter whose `GetDriveTypeW` is `DRIVE_REMOTE` | `classify.rs:62-74`; `main`: `MS` `isNetworkSource` |
| Network (macOS / Linux) | The mount's filesystem type name contains one of `smb`, `afp`, `nfs`, `cifs`, `dav`, `sshfs`, `ftp` | `classify.rs:75-77`; `main`: `QStorageInfo::fileSystemType` |
| Network, cannot tell | "Not a share" — the safe side is copying | `MS:309-310` |
| Volatile roots (all OSes) | The system temp dir, `/tmp`, `/var/tmp`, and the app's own scratch root | `classify.rs:82-93`, `MS:311-313` |
| Volatile roots (macOS) | `~/Library`, `/private/var/folders`, `/var/folders`, `/private/tmp`, `/private/var/tmp` | `classify.rs:85-88` |
| Volatile roots (Windows) | `%TEMP%`, `%TMP%`, `%LOCALAPPDATA%`, `%APPDATA%` | `classify.rs:89-91` |
| Volatile match | Path equals a root or starts with `root/`; tested on both the absolute and the canonical (symlink-resolved) path; case-insensitive on Windows | `classify.rs:99-116` |

Why volatile exists (comment on `main`): an image referenced in place from a capture app's store or a temp folder
keeps rendering from the image cache while the app is open, then the file is deleted and an export emits a dead link.

### 3.3 Content addressing and dedupe

| Entry | Hash input | File name | Ref |
|---|---|---|---|
| `importFile` (a file) | SHA-1 of the file's raw bytes | `.minnotes/<40 hex>.<source suffix, lower case>`; suffix `img` when the source has none | `MS:337-342` |
| `importImage` (a decoded raster) | SHA-1 of the image **encoded as PNG** | `.minnotes/<40 hex>.png` | `MS:375-392` |
| `importBytes` (importer attachments) | SHA-1 of the bytes | `.minnotes/<40 hex>[.<suffix of the suggested name>]` | `MS:356-373` |

Dedupe = "a file of that name already exists → skip the write" (`MS:344`, `:366`, `:386`). Bytes are not compared.
The whole file is read into memory to hash it (`MS:337`). The write is a plain open-write-close, not temp + rename
(`MS:346-347`).

### 3.4 What each kind does

| Kind | Policy | Ref |
|---|---|---|
| Image file | §3.1 | `MS:318-354` |
| Pasted / embedded raster | Always written to the sidecar as PNG | `MS:375-397` |
| Preview (PSD, HEIC, RAW, HDR) | The **rendered PNG** is written to the sidecar; the **original is always referenced in place**, local or not, never copied, never modified | `MS:180-194`, `PRh:2-4` |
| Video | Always referenced in place. `forceCopy` does not apply | `MS:280`, `MSh:22-25`, `BMh:788-791` |
| PDF | Always referenced in place | `MS:259`, `MSh:29-30` |
| File chip | Always referenced in place | `MS:61-69`, `BMh:779` |
| Remote image | Loaded from the URL; then downloaded, decoded, re-encoded as PNG into the sidecar, and the descriptor swapped | `BM:6181-6209` |

### 3.5 Collect Media (`MediaCollector`, `PE:410-565`, `PEh:140-199`)

| Item | Rule | Ref |
|---|---|---|
| Purpose | Copy every externally referenced media file into `.minnotes/` and rewrite the descriptors, so the document is whole if the originals move | `PEh:140-146`, `Man:54-57` |
| What is external | A source whose resolved path is not inside the document's folder | `PE:447-448` |
| Videos | Counted apart and **opt-in** (default off, for size); left-out videos stay references | `PE:449-454`, `Main:611-637` |
| Names | The source's **readable basename**, made unique against the plan and what `.minnotes/` already holds by a `-2`, `-3`… suffix before the extension (case-insensitive). Not a hash | `PE:66-77`, `PE:433-437`, `:457` |
| Copy | On a worker thread, in 8 MiB chunks, with progress and cancel; a failure or cancel removes everything copied so far | `PE:479-537`, `PEh:148-151` |
| Rewrite | After every copy has finished: each descriptor whose `src` **resolves** to a copied path gets `src` = `.minnotes/<name>` — absolute strings and `{vol, rel}` both match. One undo entry per page | `BM:4477-4534`, `PE:419-424` |
| Video annotation folder | A video's `.qcview/<name>/` tree is copied to `.minnotes/.qcview/<name>/` | `PE:516-531` |
| After | The document saves itself (the copies are on disk, so an unsaved rewrite is the worst half-state); an untitled document does not | `Main:571-582` |
| Offered | Document menu; and automatically after Save As when external references remain (silent when none) | `Main:1266`, `Main:107-112` |
| Refused | For a document opened from a package | `PE:431-432`, `Main:556-559` |
| Not collected | A preview's **original** (`preview.src`): the walkers read the root `src` and a sketch's `images[].src` only. No occurrence of `preview` exists in `PackageExporter.cpp` or `AssetTransfer.cpp` | `AT:59-73`, `BM:4497-4516` |

### 3.6 Media crossing documents (`AssetTransfer`, `ATh:1-12`)

Used by rich paste between documents and by tab merge.

| Source `src` | Outcome | Ref |
|---|---|---|
| `.minnotes/…` | Copied into the destination's sidecar, keeping its basename. A same-name, same-size file already there is reused without a copy; any other collision gets a unique basename | `AT:98-112`, `ATh:8-12` |
| Absolute, `{vol, rel}`, `http` | Passed through untouched | `AT:109-110`, `ATh:8-10` |

### 3.7 Lifetime of sidecar files

Files are written the moment of the paste or drop, before any save, beside the original file. No code removes a
sidecar file when its block is deleted, when the insert is undone, or when the document is closed unsaved: a search of
`app/core` for an orphan sweep found none. **Not found.**

---

## 4. Formats and routing

### 4.1 Routing order (`BlockModel::insertMediaFromUrl`, `BM:4449-4475`)

Each step is tried only if the one before did not land a block.

| # | Test | On success | On failure |
|---|---|---|---|
| 1 | Extension is a video extension **and** the container probe finds a video stream with `w`, `h` > 0 | video block | next |
| 2 | Extension is `pdf` or `ai` **and** Qt PDF loads it with ≥ 1 page | pdf block | next |
| 3 | Extension is a preview extension on this platform **and** the render returns a picture | preview (image) block | next |
| 4 | `QImageReader(path).size()` is valid and non-empty | image block (§3.1) | next |
| 5 | The path is an importable document format or a package | **no block**; `importFileRequested` is emitted and the import flow takes the file | — |
| 6 | Anything else | file chip | — |

Consequences: a video extension whose probe fails can still land as an image or a chip; an `.ai` saved without PDF
compatibility falls to a chip; a PSD without a composite falls through to step 4 and then normally to a chip
(`MSh:38-39`, `PP:1-3`, `Man:29-31`).

### 4.2 Extension lists

| Class | Extensions (lower case, compared case-insensitively) | Ref |
|---|---|---|
| Video | `mp4 mov m4v mkv webm avi mxf m2ts mts ts wmv flv mpg mpeg 3gp` | `MS:104-115` |
| PDF | `pdf ai` | `MS:167-172` |
| Preview — HDR | `hdr rgbe` | `PR:19` |
| Preview — Photoshop | `psd psb` | `PR:20` |
| Preview — HEIF (macOS only) | `heic heif hif` | `PR:21`, `PR:48-55` |
| Preview — camera RAW (macOS only) | `cr2 cr3 nef arw dng raf orf rw2 pef srw` | `PR:22-28`, `PR:48-55` |
| Audio (canvas only) | `wav aif aiff mp3 flac m4a aac ogg opus wma` | `MS:117-126` |
| Image | **No list.** Whatever `QImageReader` can size from the file | `MS:322-324` |

### 4.3 Image decoders

| Format | Decoder | Preview? | Pill | Ref |
|---|---|---|---|---|
| Whatever the Qt image plugins installed with the app read | Qt (`QImageReader` for the probe, QML `Image` for display) | no | — | `MS:322`, `MB:175-191` |
| Radiance HDR | `stb_image` (HDR only compiled in). Linear float → clamp to 0…1 → 1/2.2 gamma → 8-bit; alpha 255. Refused above 64 megapixels | yes | `HDR` | `PR:3-7`, `PR:30-35`, `PR:76-98` |
| PSD / PSB | `psd_sdk`, the **merged composite** only (no layer compositing). Accepts 8 / 16-bit RGB(A), 8 / 16-bit grayscale (+alpha), 8-bit CMYK (inverted-ink formula, no ICC). Declines 32-bit float, Lab, duotone, indexed, multichannel, a file with no composite, and above 64 megapixels. 16-bit → 8-bit by `>> 8` | yes | `PSD` / `PSB` (the extension upper-cased) | `PP:1-6`, `PP:66-112`, `PR:60` |
| PSD the SDK declined; HEIC / HEIF; RAW (macOS) | ImageIO thumbnail (`CGImageSourceCreateThumbnailAtIndex`, always-from-image, **with transform**, max pixel size = the cap), then QuickLook thumbnailing with a 10-second timeout | yes | `HEIC`, `RAW` | `PM:36-70`, `PR:72` |
| HEIC / RAW on Windows | No backend; the file routes onward (normally a chip). The code's comment calls this "tier 3 (libheif)", not built | — | — | `PR:48-55`, `PRh:9` |
| EXR | Deliberately not a preview format; lands as a file chip | — | — | `PRh:5-6`, `Man:32-34` |

Preview facts:

| Item | Rule | Ref |
|---|---|---|
| Render size | Long edge capped at **2048 px** (`kMaxEdge`), aspect kept, smooth scaling | `PRh:17-18`, `PR:39-44` |
| Stored as | A flat 8-bit RGBA PNG in the sidecar, through `importImage` (so it is content-hashed) | `MS:185-188` |
| Descriptor `w` / `h` | The **rendered** picture's size, not the original's | `MS:198-199` |
| Stale test | The original exists **and** (its mtime in ms ≠ stored `mtime` **or** its size ≠ stored `size`). Evaluated by a file stat whenever the block's bindings re-evaluate | `MS:217-225`, `MB:37` |
| Pill | Top-right of the picture, inset 6 px, height 18 px, mono bold 10 px; text = the format, or `<format> · changed` when stale. Shown only once the image is ready | `MB:192-206` |
| Refresh | Menu command on a preview block: re-render, keep the block's other keys, replace `src` / `w` / `h` / `name` / `preview`. One undo step | `MS:226-236`, `BM:4426-4440`, `Ed:8335` |
| Reveal / Open in ufb / Copy path | Point at the **original**, not the PNG | `BM:4419-4425`, `BM:4589`, `Ed:8341-8344` |
| Copy image | Copies the PNG's pixels | `Ed:8333-8334` |

### 4.4 SVG, GIF, WebP

| Format | Finding |
|---|---|
| SVG | No SVG code path in the page editor. The Qt Svg module is not among the linked Qt components (`CMakeLists.txt:51-55`, `app/CMakeLists.txt:354-364`). Whether the shipped app's image plugins include an SVG reader, and so whether a dropped SVG becomes an image or a chip, was **not determined** |
| GIF | No GIF-specific code and no `AnimatedImage` anywhere in the QML. If the installed plugins read GIF, a GIF is an image block showing a still **(inference: QML `Image` does not animate)**. **Not determined** which plugins ship |
| WebP | The same as GIF. The video player has animated-WebP handling (`app/player/seek_compat.h`), but `webp` is not in the video extension list, so a dropped WebP never reaches it through this route |
| The set of raster formats in general | It is whatever plugins the deploy tool copies (`packaging/windows/minNotes.iss:88` ships the whole `imageformats` folder). The list was **not determined** from source |

What the new side has: `min-image` reads `exr png tif tiff jpg jpeg dpx tga bmp` (`min-image` `Format::of`), with
`dimensions()` for a header probe and `load_thumbnail()` for a reduced decode. It has no GIF, WebP, SVG, HEIC, PSD or
HDR reader.

---

## 5. Dimensions, width, resize

### 5.1 Probing

| Kind | Probe | Thread | Ref |
|---|---|---|---|
| Image file | `QImageReader::size()` — header only, no decode. Invalid or empty → not an image | caller's (the GUI thread on drop / paste) | `MS:322-324` |
| Pasted raster | The decoded image's own size | GUI | `MS:391` |
| Preview | The rendered picture's size (the render itself runs here) | GUI | `MS:185-188` |
| Video | Container header: best video stream's coded width / height; no decode | GUI | `MS:269-297` |
| PDF | Page 0's size in points, rounded to integers | GUI | `MS:253-260` |
| Remote image | None: `w` = 480, `h` = 300 until the download lands | — | `MS:99-100` |

No EXIF orientation handling exists in the image path: nothing reads or applies it, the probe reports the stored
size, and the display item sets no auto-transform (`MB:175-191`). The macOS preview backend is the one place that
applies orientation (`PM:39`).

### 5.2 Displayed width (`mediaDisplayWidth`, `BM:2693-2698`)

`laneW` = the block's lane width; inside a table cell it is the lane width minus 16 (an 8 px inset each side,
`BM:2727-2731`).

| Case | Width |
|---|---|
| `dw` > 0 | `dw`, verbatim. It may exceed the lane and the page |
| PDF, no `dw` | `laneW` (fit the lane) |
| `w` > 0, no `dw` | `min(laneW, w)` — never upscaled by default |
| `w` unknown | `laneW` |

A `dw` wider than the page also widens the page: the row publishes it as its measured width, the same channel wide
tables use, and the page scrolls sideways (`BM:2681-2685`).

### 5.3 Height (`mediaFrameHeight`, `BM:3380-3386`; `estimatedHeight`, `BM:3391-3395`)

| Item | Value |
|---|---|
| Frame height | `floor(width × h / w + 0.5)` |
| Frame height when `w` or `h` is missing | `laneW × param / 100`, where `param` = `clamp(round(100 × h / w), 1, 1000)`; **56** (16:9) when dims are unknown at insert | 
| File chip | Fixed **56** px (`kFileChip`, `BM:67`) |
| Block height | `12 + frame` (6 px above, 6 below; the frame sits at `y = 6`, `BV:460`) `+ 22` for a video (`kVideoBar`, `BM:66`) or `+ 40` for a PDF (`kPdfNav`, `BM:68`) |
| Limits | `w`, `h`, `dw` are held as 16-bit values, clamped to 0…65535 (`BM:2672-2673`, `:2680`) |

Refs for `param`: `BM:2674-2675`, `BM:4363-4367`.

### 5.4 Resize

| Item | Rule | Ref |
|---|---|---|
| Which blocks | Kind `image` only (previews included). Video, PDF and chips have no resize | `Ed:831-834` |
| When the affordance shows | After an explicit click **on the picture** (inside its frame, not merely on its row), while that block keeps the caret. Never on hover; gone on any press elsewhere | `Ed:806-820`, `Ed:7818-7824`, `Ed:6594` |
| Affordance | A 1 px accent outline and four 9 px accent squares at the corners (19 px hit areas), drawn on whole pixels | `Ed:7836-7862` |
| Drag | Any corner; proportional (width drives, aspect fixed). The picture stays anchored at the lane's left edge: a left-corner drag outward grows it exactly as a right-corner drag does | `Ed:7802-7806`, `Ed:7867-7884` |
| During the drag | A ghost outline at the target size; the document does not reflow until release | `Ed:7898-7905`, `Ed:805-806` |
| Minimum | **80** px (in the drag and again in the setter) | `Ed:7882`, `BM:5384` |
| Maximum | None short of 65535: the drag may pass the page width ("the reachable screen is the practical limit") | `Ed:7879-7881`, `BM:5384-5386` |
| Commit | On release: `setMediaWidth(row, round(width))` → `dw`. Cancel discards | `Ed:7885-7891` |
| Zoom | The drag distance is divided by the page zoom, so `dw` is in unzoomed page px | `Ed:7882-7883` |
| Fit width | Menu: `dw` = the lane's width (minus 16 in a table cell). A fixed number is stored, so it does not follow a later lane-width change | `Ed:294`, `Ed:8337-8338` |
| Original size | Menu: `dw` removed → back to `min(laneW, w)` | `Ed:8339-8340`, `BM:5383` |
| Undo | Each `setMediaWidth` is one undoable change; consecutive ones on the same block merge into one step (merge key `imgw:<block id>`) | `BM:5387-5399` |
| Keyboard resize | **Not found** |
| Corner double-click | Removed in the code (`Ed:7805-7806`); the manual still describes it (`Man:13-14`) |

### 5.5 Alignment, captions, crop

| Item | Finding | Ref |
|---|---|---|
| Alignment | None. A media block is placed at its lane's left edge. No alignment key exists in the descriptor | `BV:460`, `Ed:7803-7804` |
| Caption | None. No caption key, no caption UI | — (**not found**) |
| Alt text | None stored. `name` is the nearest thing | `MSh:19-21` |
| Crop, rotate, flip | None | — (**not found**) |
| Side by side | Done with columns, not with the image: dropping a file on a block's side edge makes a two-lane split with the picture in the new lane | `BM:3335-3347` |

### 5.6 Selection look

A media block has no selection affordance of its own: no tint, no outline. The row's focus fill says "you are here";
range membership shows through the row fill (`BV:483-488`, ruling dated 2026-08-19 in the code). The resize outline of
§5.4 is the only frame ever drawn on a picture.

---

## 6. Loading

### 6.1 Threads

| Work | Thread | Ref |
|---|---|---|
| Insert-time probe, sidecar copy (read whole file, SHA-1, write), preview render | **Synchronous on the GUI thread**, inside the drop / paste handler | `BM:4372-4405`, `MS:318-354`, `MS:180-194` |
| Display decode of an image | Qt's image reader thread (`asynchronous: true`) | `MB:182` |
| Video poster frame, PDF page | Image providers flagged force-asynchronous (decode off the GUI thread) | `app/core/VideoFrameProvider.h:12-15`, `app/core/PdfPageProvider.h:15-18` |
| Remote image download | The network stack's asynchronous reply; decode and PNG re-encode in the reply handler on the GUI thread | `BM:6193-6207` |
| Collect Media and cross-document copies | A worker thread | `PEh:148-151`, `ATh:3-6` |

### 6.2 Requested size and caches

| Item | Rule | Ref |
|---|---|---|
| Decode size | Width = `round(displayed width × screen device-pixel ratio)`; height follows the aspect. The page zoom is **not** in the formula | `MB:185` |
| Cache | Qt's own pixmap cache (`cache: true`), keyed by URL + requested size. The stated reason: sidecar names are content hashes, so a URL never changes its bytes, and without the cache every delegate recycle re-decoded the picture | `MB:180-182` |
| App-owned cache, byte budget, eviction policy | **None found.** No cache-size setting is made in `app/main.cpp` or `app/MinNotesApplication.cpp` |
| Lightbox | Decodes at the overlay's size × device-pixel ratio, with mipmaps, **uncached** for an image | `LB:140-142` |
| Thumbnails on disk | None. Every display decodes from the stored file |

### 6.3 Reveal

| Item | Rule | Ref |
|---|---|---|
| Fade in | Opacity 0 → 1 over **200 ms**, ease-out cubic, when the picture becomes visible | `MB:188-190` |
| Hide | Instant (no fade out) | `MB:187` |
| Space | Held at the final size throughout, from the descriptor | `MB:50-65`, `BV:467-471` |

### 6.4 Placeholder and failure states (image)

One rectangle the size of the frame — recessed fill, 1 px border, rounded — shown whenever the picture is not ready
(`MB:217-228`). It carries one centred line of muted body text:

| Condition | Text |
|---|---|
| The URL is empty **or** the loader reports an error, **and** no package extraction is in flight | `image unavailable` |
| Otherwise (still decoding, or a package extraction is in flight) | `loading…` |

There is no icon, no file name, no path and no action on the placeholder. The block's context menu still offers Reveal
and Copy path, which act on whatever path the `src` resolves to (`Ed:8341-8344`).

`MediaBlock` has a `suspended` property meant to swap the picture for a bare frame while the window resizes
(`MB:21-24`, `:67-72`). No host sets it for page blocks: a search of the QML found it set only on the kanban view's
own property. **Dead for images.**

---

## 7. Drop and paste entry points

### 7.1 Files dropped on a page (`Ed:1539-1594`)

| Item | Rule | Ref |
|---|---|---|
| Accepts | Drags carrying `text/uri-list` | `Ed:1541` |
| Target while dragging | A table cell wins; else a lane gap, a block's side edge, or a top-level gap. The two indicators never show together | `Ed:1525-1538` |
| Drop in a gap | Each URL in order → `insertMediaAt(gap, lane, url)` → `insertMediaFromUrl` (§4.1), then the block is moved to the lane the gap was in. Each file chains after the row the previous one actually took | `Ed:1571-1586`, `BM:3312-3333` |
| Drop on a block's side edge | The first file → `insertMediaBeside`: the block is split into two columns at 0.5 and the file takes the new lane; the rest follow down that lane | `Ed:1581-1582`, `BM:3335-3347` |
| Drop on a table cell | `tableInsertMedia`: each file becomes a media block appended in that cell. A typed body cell (one that holds a chip) does not accept files. One undo step | `Ed:1563-1569`, `BM:1907-1924` |
| Empty paragraph at the target | Consumed: the media block takes its place rather than landing after it | `BM:4322-4323`, `Ed:1575-1576` |
| Importable document or package among the files | No block; the import flow is raised for that file after the loop finishes | `BM:4466-4473`, `Main:270-285` |
| `forceCopy` | Never set on a drop | `BM:3317`, `BM:3342` |
| After | The caret parks on the last landed block and it is scrolled into view | `Ed:1588-1589` |
| Undo | One step per file (`insertMediaAt` opens its own transaction per call) **(inference from `BM:3316-3332`)** |
| A non-file URL (a web link dragged from a browser) | Not handled specially in a page: it goes down the same route and would end as a chip **(inference from `BM:4442-4447`)**. The canvas has its own rule for web URLs (`app/qml/MinNotes/App/CanvasView.qml:254`) |

### 7.2 Paste (`Ed:3014-3080`, router `CP:33-48`)

The router picks one flavour; one that yields nothing is masked and the router is asked again.

| Order | Clipboard holds | Result | Ref |
|---|---|---|---|
| 1 | minNotes' own blocks (not into a code block) | The blocks, media included; sidecar files travel per §3.6 | `CP:37` |
| 2 | Text, caret in a code block | Text verbatim | `CP:39` |
| 3 | HTML — unless it is a bare remote `<img>` wrapper **and** a raster is also present | Structured blocks. Each embedded image: decoded resource → `importImage`; local path → `importFile`; `http` → a remote-image block, downloaded afterwards | `CP:41`, `Imp:679-708`, `BM:5964` |
| 4 | File URLs (local files only) | Each → `insertMediaFromUrl(afterRow, url, forceCopy)` — the same routing as a drop | `CP:42`, `Cb:28-35`, `Ed:3061-3070` |
| 5 | A raster | `insertImageFromClipboard` → PNG in the sidecar → an image block with no `name` | `CP:43`, `Ed:3072-3078`, `BM:4379-4384` |

`forceCopy` on step 4 = "the clipboard also holds a raster". File URLs **plus** raster bytes on one clipboard is the
signature of a screen-capture app: the URL points at that app's temporary file, so the image is copied even if the path
looks like a share (a file manager's copy carries URLs only). It affects the image branch only; a video or PDF URL is
still referenced (`Ed:3015-3019`, `MSh:62-65`, `BMh:788-791`).

Because step 4 comes before step 5, a capture app's paste lands as a copy of the app's **file** (original encoding,
with a `name`), not as a re-encoded PNG.

### 7.3 Other entry points

| Entry | Produces | Ref |
|---|---|---|
| Importers (DOCX, ODF, XLSX, ENEX, HTML, Markdown, Notion) | Embedded pictures → `importImage` (PNG) or `importBytes` (original bytes, original suffix); linked local files → `importFile` (§3.1); remote links → remote-image blocks | `Imp:679-708`, `Imp:1011`, `app/core/DocxReader.cpp:273-277`, `app/core/OdfReader.cpp:382-386`, `app/core/XlsxReader.cpp:394-395` |
| Insert-image menu command or file dialog for a page | **Not found.** Images enter by drop, paste or import |
| Files dropped on the canvas | The same descriptors as elements; order video → pdf → audio → preview → image → another minNotes file → importable → file card. A dropped image is `min(480, w)` wide | `Canvas:990-1037`, `app/core/CanvasDocument.h:77` |

### 7.4 Remote images

`fetchRemoteMediaIn` scans the pasted range for blocks whose `src` starts with `http`, downloads each (redirects
followed under the no-less-safe policy), decodes with Qt, stores a PNG through `importImage`, and replaces the whole
descriptor with `{src, w, h}` (`BM:6181-6209`). The swap is keyed by block id, is **not** an undo step, and resizes the
block from 480 × 300 to the real aspect (`BM:6211-6225`). Because the descriptor is rebuilt rather than patched, a
`dw` or `ink` added while the download was in flight is lost **(inference from `BM:6203-6206`)**.

---

## 8. PDF blocks (later step)

Descriptor: `{src, w, h, kind: "pdf", pages}` — `w` / `h` are page 0 in PDF points; `ink` may hold strokes and text
chips per page (`MS:52-60`, `BMh:757-777`). `.ai` is accepted as a PDF when Qt PDF can load it (`MS:167-172`).

| Item | Rule | Ref |
|---|---|---|
| Shows | One page, fit to the lane width (or `dw`), on a white rounded rectangle with a 1 px border; the page's ink drawn over it, view-only | `MB:142-168`, `BM:2695` |
| Under it | A 40 px strip with previous / next | `Ed:1072`, `Ed:6532-6538` |
| Current page | Session state keyed by the file's path, not stored in the document; shared by every block and canvas element showing that file | `Ed:1103-1118` |
| Rendering | An image provider renders the page with Qt PDF at the requested width (default 760), off the GUI thread, cached by URL (path + page) | `app/core/PdfPageProvider.h:12-28`, `MS:238-247` |
| Opens | Full-frame in a tab (read, zoom, draw); in the lightbox on Space | `Ed:8329`, `Ed:1250-1262` |
| File | Always referenced in place; never modified | `MS:259`, `Man:21-23` |

---

## 9. Video blocks (later step)

Descriptor: `{src, w, h, kind: "video", durMs, frames, fps}` (`MS:41-51`), probed from the container header without
decoding (`MS:263-298`).

| Item | Rule | Ref |
|---|---|---|
| Shows at rest | A decoded frame as a poster — frame 0, or the remembered playhead after playback — with a duration badge (`m:ss`) bottom-right. No play button on the frame | `MB:230-267`, `MB:3-6` |
| Under it | A 22 px compact transport strip, the sole control | `Ed:1071`, `BM:66` |
| Poster decode | An image provider, software decode, off the GUI thread; cached by URL (path + frame number); source colour matrix honoured | `app/core/VideoFrameProvider.h:12-26`, `MS:399-490` |
| Playing | One decoder for the whole editor; its live surface is a root overlay that covers the poster | `Ed:1003-1006`, `BV:478-480` |
| Playhead | Session state keyed by the file's anchor path, not stored | `Ed:1095-1096` |
| File | Always referenced in place; Collect Media copies it only on request | `MS:280`, §3.5 |
| Annotations | In a `.qcview/<name>/` folder beside the media file, a format shared 1:1 with a sibling app; not in the descriptor | `MS:518-531`, `MSh:121-124` |
| Width | Follows §5.2 like an image; no resize affordance | `Ed:831-834` |

---

## 10. Open questions / things that looked accidental

Each item is something the Qt code does that reads as a consequence of Qt or of history rather than as intent. They
are for the user to rule on before the new design copies them.

| # | Behaviour | Why it looks accidental | Ref |
|---|---|---|---|
| 1 | Probe, whole-file read, SHA-1, sidecar write and preview render all run on the GUI thread inside the drop handler. A QuickLook render can wait 10 seconds | Simplicity of a synchronous `Q_INVOKABLE`, not a ruling. A large local file or a slow source freezes the window | `MS:318-354`, `PM:67` |
| 2 | Local videos and PDFs are referenced in place although "every local source copies" is the stated ruling; a video in a temp folder gets no volatile protection | The ruling's text covers images; video was excluded for size, PDF with no stated reason | `MS:326-333`, `MSh:22-32` |
| 3 | A preview's original is referenced in place even when local and volatile, and Collect Media never collects it | Follows from "the original is never copied"; for a local file it leaves a document that looks self-contained but cannot refresh | §3.4, §3.5 |
| 4 | `Fit width` stores today's lane width as a number, so the picture does not follow when the column or page width changes | There is no "fit" state in the descriptor, only `dw` | `Ed:294`, `Ed:8337-8338` |
| 5 | A picture inside columns keeps a `dw` wider than its lane | `dw` is "honoured verbatim" by a ruling made for the top level; whether it was meant inside a lane is not stated | `BM:2688-2694` |
| 6 | The decode size ignores the page zoom, so a zoomed-in page shows an upscaled texture **(inference)** | The formula uses display width × pixel ratio only | `MB:185` |
| 7 | EXIF orientation is ignored for ordinary images but applied for macOS previews | Qt's default; nothing in the code chooses it | `MB:175-191`, `PM:39` |
| 8 | "Is it an image" has no extension list: it is whatever the installed Qt plugins can size. The accepted set can differ between a macOS and a Windows install | Delegated to Qt | `MS:322-324` |
| 9 | Animated GIF / WebP show as stills (if they load at all) | No animated item is used | §4.4 |
| 10 | Two naming schemes share `.minnotes/`: content hashes (drop, paste) and readable basenames (Collect Media, cross-document paste) | Two programmes written at different times. Dedupe by hash does not see a collected copy of the same bytes | §3.3, §3.5, §3.6 |
| 11 | Dedupe trusts the file name: an existing `<sha>.<ext>` is never verified, and the write is not atomic, so a truncated file from an interrupted write would be reused for ever | Plain `QFile` write | `MS:344-347` |
| 12 | A sidecar file with no suffix is stored as `<sha>.img` | A fallback so later extension tests have something to read | `MS:340-341` |
| 13 | Sidecar files are never removed (undo of an insert, block delete, unsaved close) | No sweep exists | §3.7 |
| 14 | Sidecar files are written beside the **saved** file while the edit itself is in an unsaved working copy; closing without saving leaves the files behind | The store is anchored to the original folder so paths stay stable | `Canvas:288` |
| 15 | `name` is absent on pasted rasters and on images written before 2026-10-21; `mediaFileName` then derives a name from `src`, and for a `{vol, rel}` source it reads `rel` as a string although it is an array, which yields an empty name | A string read of an array value | `BM:4600-4614` |
| 16 | The remote-image swap rebuilds the descriptor as `{src, w, h}` and bypasses undo | "A background system update, not a user edit" — but it also drops any key added meanwhile | `BM:6203-6225` |
| 17 | The remote placeholder is a hard-coded 480 × 300, so the block changes height when the real picture lands | No size is known before the download | `MS:99-100` |
| 18 | The stale test for a preview is a file stat run from a display binding, on the GUI thread, for every visible preview block, including sources on a network share | It is bound to the block's revision counter rather than scheduled | `MB:37`, `MS:217-225` |
| 19 | The image cache is Qt's global pixmap cache with no budget set by the app; the code comment gives delegate recycling as the reason caching was turned on | The cache exists to survive Qt's recycled delegates | `MB:180-182` |
| 20 | PDF pages and video posters go through image providers with the file path base64url-encoded into a URL | Stated workaround: a PDF item bound to a recycled delegate blanked to white | `app/core/PdfPageProvider.h:6-11`, `MB:80-103` |
| 21 | The media item's width binding avoids one particular property chain "to avoid a latent loop the async poster decode wakes up", and dims are keyed on the row rather than on `active` to avoid a zero-width frame on recycle | QML binding-order workarounds | `BV:461-465`, `MB:50-56` |
| 22 | `suspended` (drop the picture during a window resize) is declared but never driven for page blocks | Left over from an earlier pass | §6.4 |
| 23 | The outline and handles are rounded to whole pixels because a fractional display width antialiased them grey | A rendering workaround; it also shows that display widths are often fractional | `Ed:7838-7840`, `Ed:7859` |
| 24 | `w`, `h` and `dw` are clamped to 65535 in the row, while the descriptor keeps the true numbers | 16-bit row fields | `BM:2672-2680` |
| 25 | The manual says double-clicking a corner fits the width; the code removed it | Manual not updated | `Man:13-14`, `Ed:7805-7806` |
| 26 | A video extension whose probe fails falls through to the image test and then to a chip, silently | The fall-through chain has no error path | `BM:4452-4474` |
| 27 | Drop of several files is one undo step per file **(inference)**; the cell drop is one step for all | Different entry functions | §7.1 |
| 28 | The "missing" state gives no file name, no path, and no way to relink; a document moved to a machine without the mapping shows a page of identical grey boxes | Nothing was built | §2.4, §6.4 |

## 11. Not determined

| Item | Why |
|---|---|
| The exact raster formats the shipped app reads (JPEG, PNG, TIFF, WebP, GIF, BMP, ICO, SVG …) | Decided by the Qt image plugins the deploy tools copy; no list exists in source (§4.4) |
| Whether a dropped SVG becomes an image block | Depends on the same plugin set |
| The full key set of a 1.x `sketch` descriptor | Only the keys the 2.0 code reads were traced (§1.3) |
| Qt's pixmap-cache budget and eviction as they apply here | Qt internals; nothing is configured by the app (§6.2) |
| How the exporters (HTML, Markdown, DOCX, PDF) and the package writer treat each `src` form | Not read for this step; they resolve through the same `resolveUrl` |
| The `ink` shape schema (`shapes`, `texts` element fields) | Belongs to the annotation step; only the envelope is given (§1.1) |
| Lightbox behaviour beyond its image decode size | Read only for §6.2 |
