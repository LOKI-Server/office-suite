# LOKI Office

An office suite for [LOKI](https://github.com/LOKI-Server/LOKI), packaged as one
LOKI app (ID `office`). Every tool shares one address, one sign-in, one database
and one look; each tool lives under its own path.

| Tool | Path | What it does |
| --- | --- | --- |
| Docs | `/docs` | Private rich-text documents with autosave, trash, and Markdown/HTML export |

The home page (`/`) lists the tools and your recently edited files.

## Layout

```
src/lib.rs        router, health check, suite home page
src/schema.rs     every tool's tables, created by the health check
src/ui/           page chrome (Shell, Tool), security headers, helpers
src/docs/         Docs: routes, model, renderer, handlers
templates/        base.html, home.html, icons/, docs/
static/           office.css (suite), docs.css, editor.js, app.js
```

`loki-sdk` comes from a sibling checkout at `../LOKI/crates/loki-sdk` (see
`Cargo.toml`). Drop the `path` to build against crates.io instead.

## Building

Needs Rust, the `wasm32-wasip2` target and `cargo-loki` 0.3:

```powershell
rustup target add wasm32-wasip2
cargo install cargo-loki --locked
```

```powershell
cargo test                                    # native unit tests
cargo loki dev --server PATH_TO_LOKI_SERVER   # local LOKI with live rebuilds
cargo loki package                            # writes target/loki/office-0.1.0.loki-app
```

Import the package in LOKI's **Applications** page, grant access and enable it.

## Docs

Documents are private to the person who writes them. The editor posts a
validated block model (paragraphs, headings, quotes, code, lists, dividers;
bold, italic, underline, strikethrough, inline code, links, alignment), never
HTML, so stored documents cannot carry scripts. Links must be `http`, `https` or
`mailto`.

- Saves about a second after you stop typing, and on Ctrl+S. Each save carries
  the version it was based on; if the document changed elsewhere, the editor
  asks whether to load the latest version or keep yours.
- Paste from web pages, Word or Google Docs is converted to the same model.
- Trashed documents can be restored for 30 days.
- Shortcuts: Ctrl+B/I/U, Ctrl+E inline code, Ctrl+K link, Ctrl+\ clear
  formatting, Ctrl+Alt+0–3 normal/headings, Ctrl+Shift+7/8 lists,
  Ctrl+Shift+L/E/R/J alignment, Ctrl+Z/Ctrl+Y undo/redo.

## Adding a tool

1. Add `src/NAME/mod.rs` with a `pub static TOOL: Tool` and a
   `routes(Router) -> Router` that registers everything under `/NAME`, plus
   `POST /NAME/new` (the home page's **New** button posts there).
2. Mount it in `router()` and list it in `TOOLS` in `src/lib.rs`.
3. Add its tables to `src/schema.rs`; statements must be idempotent.
4. Put templates under `templates/NAME/` extending `base.html` with
   `Shell::new(&request, Some(&TOOL))`, wrap pages in `ui::page`, and serve any
   new static files from `router()`.
