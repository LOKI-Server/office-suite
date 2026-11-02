# Office Suite

An office suite for [LOKI](https://github.com/LOKI-Server/LOKI), packaged as one
LOKI app (ID `office`). Every tool shares one address, one sign-in, one database
and one look; each tool lives under its own path.

| Tool | Path | What it does |
| --- | --- | --- |
| Docs | `/docs` | Rich-text documents with tables, images and Markdown/HTML export |
| Sheets | `/sheets` | Spreadsheets with tabs, formulas, sorting, filters and CSV import/export |

The home page (`/`) has a **New** button per tool, your recent files, and search
across every tool. Files are private to the person who makes them, save as you
work, and go to a shared trash (`/trash`) that keeps them for 30 days.

**Version history** (`/files/{id}/versions`, also in each file's menu): while
you work, the content being replaced is kept at most every ten minutes, and
always before a restore. Each file keeps its latest 30 versions, dropping the
oldest once they pass 4 MB together. You can open any version read-only and
restore it; restoring keeps the current content as a version too.

## Layout

```
src/lib.rs        router, health check, home page, static files
src/files.rs      the shared files table: lists, search, trash, rename, copy,
                  versioned saves and version history
src/schema.rs     tables, created by the health check
src/ui/           page chrome (Shell, Tool), security headers, helpers
src/docs/         Docs: model, renderer, pages
src/sheets/       Sheets: model, number formatting and CSV, pages
templates/        base, home, files (tool lists), trash, icons/, docs/, sheets/
static/           office.css, autosave.js (shared); docs.css, editor.js; sheets.css, formula.js, sheet.js
tests/            formula engine tests (Node)
```

`loki-sdk` 0.3 comes from crates.io, so the app runs on LOKI 0.3 and later. It
uses LOKI's `database`, `forms` and `uploads` (Docs images) capabilities.

## Building

Needs Rust, the `wasm32-wasip2` target and `cargo-loki` 0.3, plus Node 20+ for
the formula tests:

```powershell
rustup target add wasm32-wasip2
cargo install cargo-loki --locked
```

```powershell
cargo test                                    # Rust unit tests
node --test tests/formula.test.js             # formula engine tests
cargo loki dev --server PATH_TO_LOKI_SERVER   # local LOKI with live rebuilds
cargo loki package                            # writes target/loki/office-0.2.0.loki-app
```

Import the package in LOKI's **Applications** page, grant access and enable it.

## Docs

The editor posts a validated block model (paragraphs, headings, quotes, code,
nested lists, tables, images, dividers; bold, italic, underline, strikethrough,
inline code, links, alignment), never HTML, so stored documents cannot carry
scripts. Links must be `http`, `https` or `mailto`. Paste from web pages, Word
or Google Docs is converted to the same model, tables and nested lists included.

- **Lists** nest up to six levels: Tab and Shift+Tab in a list item.
- **Tables:** insert from the toolbar; a **Table** menu appears inside one for
  rows, columns, the heading row and deleting. Tab moves between cells and adds
  a row at the end. Up to 500 rows and 20 columns.
- **Images:** the toolbar button, or paste or drop PNG, JPEG, GIF or WebP files
  (5 MB each). They upload to the owner's own folder and only they can load
  them. Click an image to select it (Delete removes it); double-click to describe
  it. Exports link to images on the server, so opening them needs a sign-in.
- **Find and replace:** Ctrl+F (or Ctrl+H), with match case and replace all.
- **Links:** Ctrl+K opens a small editor beside the text, to add, change, open
  or remove a link.

Shortcuts: Ctrl+B/I/U, Ctrl+E inline code, Ctrl+K link, Ctrl+F find, Ctrl+\
clear formatting, Ctrl+Alt+0–3 normal/headings, Ctrl+Shift+7/8 lists,
Ctrl+Shift+L/E/R/J alignment, Ctrl+Z/Ctrl+Y undo/redo.

## Sheets

Up to 20 tabs, each up to 2,000 rows × 52 columns (A–AZ), and 50,000 filled
cells in all. Formulas are evaluated in the browser as you work
(`static/formula.js`); each save sends every cell's input with its calculated
value, which the server validates and uses for the read-only view, CSV export
and search. Spreadsheets saved by 0.2 open as a single tab.

- **Tabs** along the bottom: add, rename (double-click), duplicate, reorder,
  delete; Ctrl+Page Up/Down switches. Renaming a tab updates formulas that use
  it; deleting one turns them into `#REF!`.
- **Formulas:** `+ - * / ^ & %`, comparisons, references (`A1`, `$A$1`,
  `Sheet2!A1`, `'Q1 data'!B2:B9`) and ranges (`B2:B9`). While typing a formula,
  click or drag over cells to insert their references. Functions: `SUM`, `AVERAGE`, `MIN`, `MAX`, `MEDIAN`,
  `COUNT`, `COUNTA`, `PRODUCT`, `ROUND`, `ABS`, `INT`, `SQRT`, `POWER`, `MOD`,
  `IF`, `IFERROR`, `AND`, `OR`, `NOT`, `CONCAT`/`CONCATENATE`, `LEN`, `UPPER`,
  `LOWER`, `TRIM`, `LEFT`, `RIGHT`. Errors: `#DIV/0!`, `#VALUE!`, `#REF!`,
  `#NAME?`, `#NUM!`, `#ERROR!` (bad formula), `#CYCLE!` (circular reference).
- **Editing:** type to replace a cell, Enter or F2 to edit it, Alt+Enter for a
  line break, Tab/Enter to move on, Delete to clear. Start text with `'` to keep
  it from being read as a number or formula.
- **Rows and columns:** insert and delete; formulas on every tab follow, and
  references to deleted cells become `#REF!`.
- **Data menu:** sort A→Z or Z→A by the active column (the selected rows, or
  every row below the frozen ones); filter rows by text in a column (the header
  row always shows; the filter is saved); freeze rows and columns up to the
  active cell (up to 10 rows and 5 columns).
- **Fill:** drag the square at the selection's corner down or right. Evenly
  stepping numbers continue their series; anything else repeats, with formulas
  following. Ctrl+D and Ctrl+R fill down and right.
- **Formatting:** bold, italic, alignment, number formats (number, whole,
  percent, currency), column widths (drag a header edge).
- **Clipboard:** copy and paste within a spreadsheet moves relative references;
  tab-separated text from Excel or Google Sheets pastes as cells; one copied
  cell fills a whole selection.
- **CSV:** import replaces the current tab; export writes the current tab's
  calculated values, with formula-like text escaped so it can't run elsewhere.
- Shortcuts: Ctrl+B/I, Ctrl+D/R, Ctrl+Z/Ctrl+Y, Ctrl+A, Ctrl+arrows to the
  edges, Home/End, Page Up/Down, Ctrl+Page Up/Down for tabs.

## Adding a tool

1. Add `src/NAME/mod.rs` with a `pub static TOOL: Tool` and a
   `routes(Router) -> Router` registering its pages under `/NAME`, including
   `POST /NAME/new` and `GET /NAME/{id}/view`.
2. Add a `Kind` for it in `src/files.rs`, mount its routes in `router()` and list
   it in `TOOLS` in `src/lib.rs`.
3. Store files through `files::create`, `Stored::load` and `files::save`, which
   take care of ownership, conflict versions, history, trash and search. Add the
   tool's `content` (validate and summarize a body) and `show` (read-only page,
   also used for earlier versions) to `Kind` in `src/files.rs`.
4. Put templates under `templates/NAME/` extending `base.html`, wrap pages in
   `ui::page`, reuse `static/autosave.js` for saving, and add new static files
   to `ASSETS` in `src/lib.rs`.
