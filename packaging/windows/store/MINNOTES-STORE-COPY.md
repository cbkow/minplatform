# minNotes 3.0 — Microsoft Store listing and release notes (draft)

Drafted 2026-11-14 for the owner to edit and paste (canon row 379). English only. Store ID 9P2S2W9TLL9D; the package
is `minNotes-3.0.0-win64.msix` from `packaging/windows/make-msix-minnotes.ps1` (identity `cbkow.minNotes`, the same
as 2.0.6, so the Store installs it as an update).

## Short description

Notes for people who work with footage: text, tables, pictures, PDFs and videos you can play, write timed notes on
and draw on, in one file your team can share.

## Description

minNotes is a notes application made for film, animation and post-production work.

A collection is one file that holds notes, folders and canvases in a sidebar tree. A note is made of blocks: text,
lists and tasks, tables, code, pictures, PDFs, files and videos. A video plays in its block; you write notes on
frames and draw on them without leaving the page, and those notes are shared with QCView. A PDF is paged in its
block, its text can be selected, and you can draw on its pages. A canvas is a free surface for pictures, videos,
PDFs and notes as cards, with frames, a pen and shapes.

Collections can live on shared storage. Your work is kept on your machine as you type; Save publishes it to the
document and merges what a colleague saved in the meantime.

Hand work to others as HTML, Markdown, Word or PDF, as a web folder with playing videos and an optional password,
or as one package file with its media.

minNotes is free software under the GNU General Public License. It has no accounts and sends no usage data.

## What's new in this version

minNotes 3 is a new application under the same name, written again from the start.

- IMPORTANT for minNotes 2 users: a document is now a collection of notes, folders and canvases. minNotes 3 does
  not open a minNotes 2 document as it is; it imports its pages into a new collection. Open the old document and
  choose Import. The old file is not changed. Things placed directly on the old canvas, outside a page, are not
  imported, and the message after the import counts them.
- Folders in the sidebar, and rows you can drag to reorder.
- A save that merges, for collections on shared storage.
- Video notes and drawings made in the note; a large view for PDFs and pictures with drawing, text selection and
  find.
- Tables with choice, check and timecode columns; a table can be shown as a board.
- Web Folder export: notes as pages for a browser, with videos that play and an optional password.
- Collect Media and a one-file package.
- Import of Markdown, web pages, text, CSV, Word, Excel and OpenDocument files.
- Reading mode, Always on Top, and the User Guide inside the application (Help > User Guide).

Gone from minNotes 2: the path bar, the history timeline, the built-in spelling checker, streams, and Evernote and
Notion import. See "Coming from minNotes 2" in the User Guide.

## Product features (up to 20, 200 characters each)

- Notes, folders and canvases in one file
- Videos that play in the note, with timed notes and drawings shared with QCView
- PDFs paged in the note, with text selection, drawing and find
- Tables with choice, check and timecode columns, shown as a table or a board
- An infinite canvas for pictures, videos, PDFs and notes
- Collections on shared storage, with a save that merges
- Export as HTML, Markdown, Word, PDF or a web folder with an optional password
- Collect Media and one-file packages
- Import of Markdown, HTML, text, CSV, Word, Excel and OpenDocument
- Import of minNotes 2 documents
- Reading mode and Always on Top
- No accounts, no usage data; free software (GPL)

## Search terms (7 at most, 30 characters each)

notes, video review, post-production, storyboard notes, shot list, canvas, markdown

## Screenshot captions

(Pictures: the manual's, retaken on Windows at 1920 x 1080 - `tools/mn-shots`; list to settle with the owner.)

1. A note with text, tasks and a picture
2. A video with timed notes and drawings
3. A table shown as a board
4. A canvas of pictures and notes
5. A PDF in the large view
6. A web folder in the browser

## Certification notes (for the submission)

- `runFullTrust` justification: "minNotes is a desktop notes editor that opens and saves user-chosen documents and
  media anywhere on disk or on network shares; full trust is the standard entry point for a Win32 application."
- No sign-in. To test: start minNotes, choose New Collection, type; File > Save asks where the file goes.
- This version replaces 2.0.6 under the same identity. Documents made with 2.0.6 are imported, not opened in place.

## Release notes (GitHub release `v3.0.0`, and the macOS feed's entry)

```html
<ul>
  <li><b>minNotes 3 is a new application under the same name.</b> A document is now a collection of notes, folders
      and canvases. A minNotes 2 document is imported, not opened in place: open it and choose Import. The old file
      is not changed; things placed directly on its canvas, outside a page, are not imported.</li>
  <li>Folders, a merging save for shared storage, video notes and drawings in the note, a large view for PDFs and
      pictures, tables as boards, Web Folder export with an optional password, packages, and a wider import.</li>
  <li>Windows: minNotes comes from the Microsoft Store only, which also delivers its updates.</li>
</ul>
<p>Read "Coming from minNotes 2" first: https://minnotes.app/from-2/</p>
```
