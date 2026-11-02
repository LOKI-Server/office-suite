# Office Suite

An office suite for [LOKI](https://github.com/LOKI-Server/LOKI), packaged as one
LOKI app (ID `office`). Every tool shares one address, one sign-in, one database
and one look; each tool lives under its own path.

| Tool | Path | What it does |
| --- | --- | --- |
| Docs | `/docs` | Rich-text documents with Markdown/HTML export |
| Sheets | `/sheets` | Spreadsheets with formulas, number formats and CSV import/export |

The home page (`/`) has a **New** button per tool, your recent files, and search
across every tool. Files are private to the person who makes them, save as you
work, and go to a shared trash (`/trash`) that keeps them for 30 days.

## Layout

```
src/lib.rs        router, health check, home page, static files
src/files.rs      the shared files table: lists, search, trash, rename, copy, versioned saves
src/schema.rs     tables, created by the health check
src/ui/           page chrome (Shell, Tool), security headers, helpers
src/docs/         Docs: model, renderer, pages
src/sheets/       Sheets: model, number formatting and CSV, pages
templates/        base, home, files (tool lists), trash, icons/, docs/, sheets/
static/           office.css, autosave.js (shared); docs.css, editor.js; sheets.css, formula.js, sheet.js
tests/            formula engine tests (Node)
```

`loki-sdk` comes from a sibling checkout at `../LOKI/crates/loki-sdk` (see
`Cargo.toml`). Drop the `path` to build against crates.io instead.

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
lists, dividers; bold, italic, underline, strikethrough, inline code, links,
alignment), never HTML, so stored documents cannot carry scripts. Links must be
`http`, `https` or `mailto`. Paste from web pages, Word or Google Docs is
converted to the same model.

Shortcuts: Ctrl+B/I/U, Ctrl+E inline code, Ctrl+K link, Ctrl+\ clear
formatting, Ctrl+Alt+0–3 normal/headings, Ctrl+Shift+7/8 lists,
Ctrl+Shift+L/E/R/J alignment, Ctrl+Z/Ctrl+Y undo/redo.

## Sheets

Up to 2,000 rows × 52 columns (A–AZ) and 50,000 filled cells. Formulas are
evaluated in the browser as you work (`static/formula.js`); each save sends
every cell's input with its calculated value, which the server validates and
uses for the read-only view and CSV export.

- **Formulas:** `+ - * / ^ & %`, comparisons, references (`A1`, `$A$1`) and
  ranges (`B2:B9`). Functions: `SUM`, `AVERAGE`, `MIN`, `MAX`, `MEDIAN`,
  `COUNT`, `COUNTA`, `PRODUCT`, `ROUND`, `ABS`, `INT`, `SQRT`, `POWER`, `MOD`,
  `IF`, `IFERROR`, `AND`, `OR`, `NOT`, `CONCAT`/`CONCATENATE`, `LEN`, `UPPER`,
  `LOWER`, `TRIM`, `LEFT`, `RIGHT`. Errors: `#DIV/0!`, `#VALUE!`, `#REF!`,
  `#NAME?`, `#NUM!`, `#ERROR!` (bad formula), `#CYCLE!` (circular reference).
- **Editing:** type to replace a cell, Enter or F2 to edit it, Alt+Enter for a
  line break, Tab/Enter to move on, Delete to clear. Start text with `'` to keep
  it from being read as a number or formula.
- **Rows and columns:** insert and delete; formulas follow, and references to
  deleted cells become `#REF!`.
- **Formatting:** bold, italic, alignment, number formats (number, whole,
  percent, currency), column widths (drag a header edge).
- **Clipboard:** copy and paste within a spreadsheet moves relative references;
  tab-separated text from Excel or Google Sheets pastes as cells; one copied
  cell fills a whole selection.
- **CSV:** import replaces the contents; export writes calculated values, with
  formula-like text escaped so it can't run elsewhere.
- Shortcuts: Ctrl+B/I, Ctrl+Z/Ctrl+Y, Ctrl+A, Ctrl+arrows to the edges,
  Home/End, Page Up/Down.

## Adding a tool

1. Add `src/NAME/mod.rs` with a `pub static TOOL: Tool` and a
   `routes(Router) -> Router` registering its pages under `/NAME`, including
   `POST /NAME/new` and `GET /NAME/{id}/view`.
2. Add a `Kind` for it in `src/files.rs`, mount its routes in `router()` and list
   it in `TOOLS` in `src/lib.rs`.
3. Store files through `files::create`, `Stored::load` and `files::save`, which
   take care of ownership, versions, trash and search.
4. Put templates under `templates/NAME/` extending `base.html`, wrap pages in
   `ui::page`, reuse `static/autosave.js` for saving, and add new static files
   to `ASSETS` in `src/lib.rs`.
