// Sheets editor: a virtualised grid over the spreadsheet model.
//
// Cells hold what was typed; formula.js computes every value after each
// change. Saving posts each cell's input with its value, which the server
// uses for exports and the read-only view.
"use strict";
(() => {
  const F = window.SheetFormula;
  const data = JSON.parse(document.getElementById("doc-data").textContent);
  const wrap = document.getElementById("grid-wrap");
  const table = document.getElementById("grid");
  const editorEl = document.getElementById("cell-editor");
  const formulaInput = document.getElementById("formula");
  const cellName = document.getElementById("cell-name");
  const statsEl = document.getElementById("selection-stats");
  const formatSelect = document.getElementById("number-format");
  const importInput = document.getElementById("import-csv");

  const MAX_ROWS = 2000;
  const MAX_COLS = 52;
  const MAX_CELLS = 50000;
  const ROW_H = 25;
  const HEAD_W = 48;
  const DEFAULT_W = 100;
  const BUFFER = 15;
  // Formatting a selection bigger than this only touches cells that have content.
  const FORMAT_ALL_LIMIT = 10000;

  // ---------------------------------------------------------------- state

  let rows = data.body.rows;
  let cols = data.body.cols;
  let widths = Object.assign({}, data.body.widths || {});
  // Cell objects are never mutated in place, so snapshots can share them.
  let cells = new Map(Object.entries(data.body.cells || {}));
  let values = new Map();

  let anchor = { col: 0, row: 0 }; // the active cell
  let focus = { col: 0, row: 0 }; // the other corner of the selection
  let editing = null; // {col, row, mode: "enter" | "edit", source: "cell" | "bar"}
  let clip = null; // the last copy: {text, origin, grid}

  const width = (col) => widths[F.colName(col)] || DEFAULT_W;
  const cellAt = (col, row) => cells.get(F.keyOf(col, row));
  const selection = () => ({
    c1: Math.min(anchor.col, focus.col),
    c2: Math.max(anchor.col, focus.col),
    r1: Math.min(anchor.row, focus.row),
    r2: Math.max(anchor.row, focus.row),
  });

  function setCell(key, patch) {
    const next = Object.assign({}, cells.get(key) || {}, patch);
    for (const k of Object.keys(next)) {
      if (next[k] === "" || next[k] === false || next[k] === null || next[k] === undefined) delete next[k];
    }
    if (Object.keys(next).length) cells.set(key, next);
    else cells.delete(key);
  }

  // ---------------------------------------------------------------- history

  const undoStack = [];
  const redoStack = [];
  const snapshot = () => ({ cells: new Map(cells), rows, cols, widths: Object.assign({}, widths) });
  function restore(s) {
    cells = new Map(s.cells);
    rows = s.rows;
    cols = s.cols;
    widths = Object.assign({}, s.widths);
    anchor = clampPoint(anchor);
    focus = clampPoint(focus);
    changed(true);
  }
  // Runs `change` as one undoable step. Returns false if it was refused.
  function apply(change, structural) {
    const before = snapshot();
    if (change() === false) return false;
    if (cells.size > MAX_CELLS) {
      restore(before);
      window.alert(`Spreadsheets can have up to ${MAX_CELLS.toLocaleString()} filled cells.`);
      return false;
    }
    undoStack.push(before);
    if (undoStack.length > 100) undoStack.shift();
    redoStack.length = 0;
    changed(structural);
    return true;
  }
  function undo() {
    if (!undoStack.length) return;
    redoStack.push(snapshot());
    restore(undoStack.pop());
  }
  function redo() {
    if (!redoStack.length) return;
    undoStack.push(snapshot());
    restore(redoStack.pop());
  }

  // ---------------------------------------------------------------- rendering

  let colgroup, headRow, tbody, firstRow = -1, lastRow = -1;

  function buildTable() {
    table.replaceChildren();
    colgroup = document.createElement("colgroup");
    const thead = document.createElement("thead");
    headRow = document.createElement("tr");
    tbody = document.createElement("tbody");
    table.append(colgroup, thead, tbody);
    thead.appendChild(headRow);
    const headCol = document.createElement("col");
    headCol.style.width = HEAD_W + "px";
    colgroup.appendChild(headCol);
    const corner = document.createElement("th");
    corner.className = "corner";
    corner.title = "Select all";
    headRow.appendChild(corner);
    for (let c = 0; c < cols; c++) {
      const col = document.createElement("col");
      col.style.width = width(c) + "px";
      colgroup.appendChild(col);
      const th = document.createElement("th");
      th.scope = "col";
      th.dataset.col = c;
      th.textContent = F.colName(c);
      const handle = document.createElement("span");
      handle.className = "resize";
      handle.dataset.col = c;
      th.appendChild(handle);
      headRow.appendChild(th);
    }
    table.style.width = HEAD_W + Array.from({ length: cols }, (_, c) => width(c)).reduce((a, b) => a + b, 0) + "px";
    firstRow = lastRow = -1;
    renderRows(true);
  }

  function spacer(height) {
    const tr = document.createElement("tr");
    tr.className = "spacer";
    const td = document.createElement("td");
    td.colSpan = cols + 1;
    td.style.height = height + "px";
    tr.appendChild(td);
    return tr;
  }

  // Draws only the rows near the viewport.
  function renderRows(force) {
    const top = Math.max(0, wrap.scrollTop - ROW_H);
    const first = Math.max(0, Math.floor(top / ROW_H) - BUFFER);
    const last = Math.min(rows, Math.ceil((top + wrap.clientHeight) / ROW_H) + BUFFER);
    if (!force && first === firstRow && last === lastRow) return;
    firstRow = first;
    lastRow = last;
    const fragment = document.createDocumentFragment();
    if (first > 0) fragment.appendChild(spacer(first * ROW_H));
    for (let r = first; r < last; r++) {
      const tr = document.createElement("tr");
      tr.dataset.row = r;
      const th = document.createElement("th");
      th.scope = "row";
      th.textContent = r + 1;
      tr.appendChild(th);
      for (let c = 0; c < cols; c++) {
        const td = document.createElement("td");
        paintCell(td, c, r);
        tr.appendChild(td);
      }
      fragment.appendChild(tr);
    }
    if (last < rows) fragment.appendChild(spacer((rows - last) * ROW_H));
    tbody.replaceChildren(fragment);
    paintSelection();
    placeEditor();
  }

  function paintCell(td, col, row) {
    const key = F.keyOf(col, row);
    const cell = cells.get(key);
    const value = values.has(key) ? values.get(key) : null;
    td.textContent = F.display(value, cell && cell.fmt);
    let align = cell && cell.align;
    if (!align) {
      align = typeof value === "number" ? "right" : typeof value === "boolean" || F.isErr(value) ? "center" : "left";
    }
    const classes = [];
    if (align !== "left") classes.push("align-" + align);
    if (cell && cell.bold) classes.push("bold");
    if (cell && cell.italic) classes.push("italic");
    if (F.isErr(value)) classes.push("error");
    td.className = classes.join(" ");
  }

  function tdAt(col, row) {
    if (row < firstRow || row >= lastRow) return null;
    const tr = tbody.querySelector(`tr[data-row="${row}"]`);
    return tr ? tr.cells[col + 1] || null : null;
  }

  function repaintValues() {
    for (const tr of tbody.querySelectorAll("tr[data-row]")) {
      const r = Number(tr.dataset.row);
      for (let c = 0; c < cols; c++) paintCell(tr.cells[c + 1], c, r);
    }
    paintSelection();
  }

  let painted = [];
  function paintSelection() {
    for (const el of painted) el.classList.remove("selected", "active", "head-selected");
    painted = [];
    const { c1, c2, r1, r2 } = selection();
    const multi = c1 !== c2 || r1 !== r2;
    for (let r = Math.max(r1, firstRow); r <= Math.min(r2, lastRow - 1); r++) {
      const tr = tbody.querySelector(`tr[data-row="${r}"]`);
      if (!tr) continue;
      tr.cells[0].classList.add("head-selected");
      painted.push(tr.cells[0]);
      if (multi) {
        for (let c = c1; c <= c2; c++) {
          tr.cells[c + 1].classList.add("selected");
          painted.push(tr.cells[c + 1]);
        }
      }
    }
    for (let c = c1; c <= c2; c++) {
      headRow.cells[c + 1].classList.add("head-selected");
      painted.push(headRow.cells[c + 1]);
    }
    const active = tdAt(anchor.col, anchor.row);
    if (active) {
      active.classList.add("active");
      painted.push(active);
    }
    updateChrome();
  }

  // The formula bar, toolbar state and selection statistics.
  function updateChrome() {
    const key = F.keyOf(anchor.col, anchor.row);
    const cell = cells.get(key);
    const { c1, c2, r1, r2 } = selection();
    cellName.textContent = c1 === c2 && r1 === r2 ? key : `${F.keyOf(c1, r1)}:${F.keyOf(c2, r2)}`;
    if (!editing) formulaInput.value = cell ? cell.input || "" : "";
    formatSelect.value = (cell && cell.fmt) || "";
    for (const button of document.querySelectorAll(".toolbar [data-cmd=bold], .toolbar [data-cmd=italic]")) {
      button.setAttribute("aria-pressed", String(!!(cell && cell[button.dataset.cmd])));
    }
    for (const button of document.querySelectorAll(".toolbar [data-align]")) {
      button.setAttribute("aria-pressed", String(!!cell && cell.align === button.dataset.align));
    }
    let count = 0, sum = 0, filled = 0;
    if ((c2 - c1 + 1) * (r2 - r1 + 1) <= 100000) {
      for (let r = r1; r <= r2; r++) {
        for (let c = c1; c <= c2; c++) {
          const v = values.get(F.keyOf(c, r));
          if (v !== undefined && v !== null && v !== "") filled++;
          if (typeof v === "number") {
            count++;
            sum += v;
          }
        }
      }
    }
    statsEl.textContent = filled > 1 && count
      ? `Sum ${F.display(sum)} · Average ${F.display(sum / count)} · Count ${count}`
      : "";
  }

  function scrollIntoView(col, row) {
    const top = row * ROW_H;
    const headH = headRow.offsetHeight;
    if (top < wrap.scrollTop) wrap.scrollTop = top;
    else if (top + ROW_H + headH > wrap.scrollTop + wrap.clientHeight) wrap.scrollTop = top + ROW_H + headH - wrap.clientHeight;
    let left = 0;
    for (let c = 0; c < col; c++) left += width(c);
    if (left < wrap.scrollLeft) wrap.scrollLeft = left;
    else if (left + width(col) + HEAD_W > wrap.scrollLeft + wrap.clientWidth) {
      wrap.scrollLeft = left + width(col) + HEAD_W - wrap.clientWidth;
    }
    renderRows(false);
  }

  // Recomputes values after a change; `structural` rebuilds the grid.
  function changed(structural) {
    values = F.compute(cells, { rows, cols });
    if (structural) buildTable();
    else repaintValues();
    autosave.changed();
  }

  // ---------------------------------------------------------------- selection

  const clampPoint = (p) => ({ col: Math.min(Math.max(p.col, 0), cols - 1), row: Math.min(Math.max(p.row, 0), rows - 1) });

  function select(point, extend) {
    point = clampPoint(point);
    if (extend) focus = point;
    else anchor = focus = point;
    scrollIntoView(extend ? focus.col : anchor.col, extend ? focus.row : anchor.row);
    paintSelection();
  }

  // ---------------------------------------------------------------- editing

  function startEditing(mode, text) {
    const cell = cellAt(anchor.col, anchor.row);
    editing = { col: anchor.col, row: anchor.row, mode, source: "cell" };
    editorEl.value = text !== undefined ? text : cell ? cell.input || "" : "";
    formulaInput.value = editorEl.value;
    editorEl.hidden = false;
    placeEditor();
    editorEl.focus();
    const end = editorEl.value.length;
    editorEl.setSelectionRange(end, end);
  }

  function placeEditor() {
    if (!editing || editing.source !== "cell") return;
    const td = tdAt(editing.col, editing.row);
    if (!td) return;
    editorEl.style.left = td.offsetLeft + "px";
    editorEl.style.top = td.offsetTop + "px";
    editorEl.style.minWidth = td.offsetWidth + "px";
    editorEl.style.height = td.offsetHeight + "px";
  }

  // Saves the edit; `move` then moves the active cell by {col, row}.
  function commit(move) {
    if (!editing) return;
    const { col, row, source } = editing;
    const text = (source === "bar" ? formulaInput.value : editorEl.value).replace(/\r/g, "");
    if (new TextEncoder().encode(text).length > 8192) {
      cancel();
      window.alert("A cell can hold up to 8 KB of text, so that entry wasn't kept.");
      return;
    }
    editing = null;
    editorEl.hidden = true;
    const key = F.keyOf(col, row);
    const before = (cellAt(col, row) || {}).input || "";
    if (text !== before) apply(() => setCell(key, { input: text }));
    wrap.focus({ preventScroll: true });
    if (move) select({ col: col + move.col, row: row + move.row });
    else paintSelection();
  }

  function cancel() {
    if (!editing) return;
    editing = null;
    editorEl.hidden = true;
    wrap.focus({ preventScroll: true });
    paintSelection();
  }

  editorEl.addEventListener("input", () => {
    formulaInput.value = editorEl.value;
  });
  editorEl.addEventListener("keydown", (event) => {
    const arrows = { ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0] };
    if (event.key === "Enter" && event.altKey) {
      // Alt+Enter adds a line inside the cell.
      event.preventDefault();
      const { selectionStart: s, selectionEnd: e, value } = editorEl;
      editorEl.value = value.slice(0, s) + "\n" + value.slice(e);
      editorEl.setSelectionRange(s + 1, s + 1);
      formulaInput.value = editorEl.value;
    } else if (event.key === "Enter") {
      event.preventDefault();
      commit({ col: 0, row: event.shiftKey ? -1 : 1 });
    } else if (event.key === "Tab") {
      event.preventDefault();
      commit({ col: event.shiftKey ? -1 : 1, row: 0 });
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancel();
    } else if (editing && editing.mode === "enter" && arrows[event.key]) {
      // Typing over a cell: arrows finish the entry, as in other spreadsheets.
      event.preventDefault();
      const [col, row] = arrows[event.key];
      commit({ col, row });
    }
  });
  editorEl.addEventListener("blur", () => {
    if (editing && editing.source === "cell") commit(null);
  });

  formulaInput.addEventListener("focus", () => {
    if (editing && editing.source === "cell") commit(null);
    editing = { col: anchor.col, row: anchor.row, mode: "edit", source: "bar" };
  });
  formulaInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      commit({ col: 0, row: 1 });
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancel();
    }
  });
  formulaInput.addEventListener("blur", () => {
    if (editing && editing.source === "bar") commit(null);
  });

  // ---------------------------------------------------------------- commands

  // Calls `fn(key, col, row)` for every cell in the selection, or only the
  // filled ones when the selection is very large.
  function eachSelected(fn) {
    const { c1, c2, r1, r2 } = selection();
    if ((c2 - c1 + 1) * (r2 - r1 + 1) > FORMAT_ALL_LIMIT) {
      for (const key of Array.from(cells.keys())) {
        const p = F.parseKey(key);
        if (p && p.col >= c1 && p.col <= c2 && p.row >= r1 && p.row <= r2) fn(key, p.col, p.row);
      }
      return;
    }
    for (let r = r1; r <= r2; r++) for (let c = c1; c <= c2; c++) fn(F.keyOf(c, r), c, r);
  }

  function toggleMark(mark) {
    const on = !(cellAt(anchor.col, anchor.row) || {})[mark];
    apply(() => eachSelected((key) => setCell(key, { [mark]: on })));
  }
  function setAlign(align) {
    const current = (cellAt(anchor.col, anchor.row) || {}).align;
    const next = current === align ? null : align;
    apply(() => eachSelected((key) => setCell(key, { align: next })));
  }
  function setFormat(fmt) {
    apply(() => eachSelected((key) => setCell(key, { fmt: fmt || null })));
    wrap.focus({ preventScroll: true });
  }
  function clearSelection() {
    apply(() => eachSelected((key) => setCell(key, { input: "" })));
  }

  // Moves cells along one axis, rewriting every formula with `rewrite`.
  // `place(index)` returns the cell's new index, or null to drop it.
  function shiftCells(axis, place, rewrite) {
    const next = new Map();
    for (const [key, cell] of cells) {
      const p = F.parseKey(key);
      if (!p) continue;
      const index = place(axis === "row" ? p.row : p.col);
      if (index === null) continue;
      const moved = axis === "row" ? F.keyOf(p.col, index) : F.keyOf(index, p.row);
      next.set(moved, cell.input && F.isFormula(cell.input) ? Object.assign({}, cell, { input: rewrite(cell.input) }) : cell);
    }
    cells = next;
  }

  function insert(axis, at, count) {
    const limit = axis === "row" ? MAX_ROWS : MAX_COLS;
    const size = axis === "row" ? rows : cols;
    if (size + count > limit) {
      window.alert(`Spreadsheets can have up to ${limit} ${axis === "row" ? "rows" : "columns"}.`);
      return;
    }
    apply(() => {
      shiftCells(axis, (i) => (i >= at ? i + count : i), (input) => F.insertShift(input, axis, at, count));
      if (axis === "row") rows += count;
      else {
        cols += count;
        const next = {};
        for (const [name, w] of Object.entries(widths)) {
          const c = F.colIndex(name);
          next[F.colName(c >= at ? c + count : c)] = w;
        }
        widths = next;
      }
    }, true);
  }

  function remove(axis, at, count) {
    const size = axis === "row" ? rows : cols;
    if (count >= size) {
      window.alert(`A spreadsheet needs at least one ${axis === "row" ? "row" : "column"}.`);
      return;
    }
    apply(() => {
      shiftCells(
        axis,
        (i) => (i < at ? i : i < at + count ? null : i - count),
        (input) => F.deleteShift(input, axis, at, count),
      );
      if (axis === "row") rows -= count;
      else {
        cols -= count;
        const next = {};
        for (const [name, w] of Object.entries(widths)) {
          const c = F.colIndex(name);
          if (c < at) next[name] = w;
          else if (c >= at + count) next[F.colName(c - count)] = w;
        }
        widths = next;
      }
      anchor = clampPoint(anchor);
      focus = clampPoint(focus);
    }, true);
  }

  function command(name) {
    const { c1, c2, r1, r2 } = selection();
    switch (name) {
      case "undo": return undo();
      case "redo": return redo();
      case "bold": case "italic": return toggleMark(name);
      case "insertRowAbove": return insert("row", r1, r2 - r1 + 1);
      case "insertRowBelow": return insert("row", r2 + 1, r2 - r1 + 1);
      case "deleteRows": return remove("row", r1, r2 - r1 + 1);
      case "insertColLeft": return insert("col", c1, c2 - c1 + 1);
      case "insertColRight": return insert("col", c2 + 1, c2 - c1 + 1);
      case "deleteCols": return remove("col", c1, c2 - c1 + 1);
      case "addRows": {
        const count = Math.min(100, MAX_ROWS - rows);
        if (count > 0) apply(() => { rows += count; }, true);
        return;
      }
    }
  }

  // ---------------------------------------------------------------- clipboard

  function selectionGrid() {
    const { c1, c2, r1, r2 } = selection();
    const grid = [];
    for (let r = r1; r <= r2; r++) {
      const line = [];
      for (let c = c1; c <= c2; c++) line.push(cellAt(c, r) || null);
      grid.push(line);
    }
    return grid;
  }

  function copySelection(event) {
    const { c1, c2, r1, r2 } = selection();
    const text = F.toDelimited(
      selectionGrid().map((line, r) =>
        line.map((cell, c) => F.display(values.get(F.keyOf(c1 + c, r1 + r)) ?? null, cell && cell.fmt)),
      ),
      "\t",
    );
    clip = { text, origin: { col: c1, row: r1 }, grid: selectionGrid() };
    event.clipboardData.setData("text/plain", text);
    event.preventDefault();
    return { c1, c2, r1, r2 };
  }

  // Pastes a grid of cells (from this spreadsheet) or of typed text at the active cell.
  function paste(text) {
    const internal = clip && clip.text === text;
    const grid = internal ? clip.grid : F.parseDelimited(text.replace(/\r?\n$/, ""), "\t").map((line) => line.map((input) => ({ input })));
    if (!grid.length) return;
    const { c1, c2, r1, r2 } = selection();
    // One copied cell fills the whole selection.
    const fill = grid.length === 1 && grid[0].length === 1 && (c1 !== c2 || r1 !== r2);
    const height = fill ? r2 - r1 + 1 : grid.length;
    const widthCells = fill ? c2 - c1 + 1 : Math.max(...grid.map((line) => line.length));
    if (r1 + height > MAX_ROWS || c1 + widthCells > MAX_COLS) {
      window.alert("That doesn't fit: spreadsheets can have up to 2,000 rows and 52 columns.");
      return;
    }
    apply(() => {
      rows = Math.max(rows, r1 + height);
      cols = Math.max(cols, c1 + widthCells);
      for (let r = 0; r < height; r++) {
        for (let c = 0; c < widthCells; c++) {
          const source = fill ? grid[0][0] : (grid[r] || [])[c];
          const key = F.keyOf(c1 + c, r1 + r);
          if (!source) {
            if (internal) cells.delete(key);
            continue;
          }
          let input = source.input || "";
          if (internal && F.isFormula(input)) {
            const from = fill ? clip.origin : { col: clip.origin.col + c, row: clip.origin.row + r };
            input = F.moveFormula(input, c1 + c - from.col, r1 + r - from.row, { rows, cols });
          }
          if (internal) {
            const copy = Object.assign({}, source, { input });
            if (!copy.input) delete copy.input;
            cells.set(key, copy);
          } else {
            setCell(key, { input });
          }
        }
      }
      focus = { col: c1 + widthCells - 1, row: r1 + height - 1 };
      anchor = { col: c1, row: r1 };
    }, true);
  }

  const gridHasFocus = () => document.activeElement === wrap;
  document.addEventListener("copy", (event) => {
    if (gridHasFocus()) copySelection(event);
  });
  document.addEventListener("cut", (event) => {
    if (!gridHasFocus()) return;
    copySelection(event);
    clearSelection();
  });
  document.addEventListener("paste", (event) => {
    if (!gridHasFocus() || !event.clipboardData) return;
    event.preventDefault();
    paste(event.clipboardData.getData("text/plain"));
  });

  // ---------------------------------------------------------------- CSV import

  importInput.addEventListener("change", async () => {
    const file = importInput.files[0];
    importInput.value = "";
    if (!file) return;
    if (file.size > 4 * 1024 * 1024) {
      window.alert("That file is too big to import (4 MB at most).");
      return;
    }
    const text = await file.text();
    const tab = /\.tsv$/i.test(file.name) || (!text.split("\n", 1)[0].includes(",") && text.includes("\t"));
    const grid = F.parseDelimited(text, tab ? "\t" : ",");
    const height = grid.length;
    const widthCells = Math.max(0, ...grid.map((line) => line.length));
    if (!height) return;
    if (height > MAX_ROWS || widthCells > MAX_COLS) {
      window.alert(`That file has ${height} rows and ${widthCells} columns; spreadsheets can have up to ${MAX_ROWS} rows and ${MAX_COLS} columns.`);
      return;
    }
    if (cells.size && !window.confirm(`Replace everything in this spreadsheet with ${file.name}?`)) return;
    apply(() => {
      cells = new Map();
      rows = Math.max(rows, height);
      cols = Math.max(cols, widthCells);
      grid.forEach((line, r) => line.forEach((input, c) => {
        if (input) cells.set(F.keyOf(c, r), { input });
      }));
      anchor = focus = { col: 0, row: 0 };
    }, true);
  });

  // ---------------------------------------------------------------- mouse

  let dragging = false;
  function pointFromEvent(event) {
    const td = event.target.closest("td, th");
    if (!td || !table.contains(td)) return null;
    const tr = td.parentElement;
    if (tr.parentElement.tagName === "THEAD") {
      const col = Number(td.dataset.col);
      return td.classList.contains("corner") ? { all: true } : { col, header: "col" };
    }
    if (!tr.dataset.row) return null;
    const row = Number(tr.dataset.row);
    if (td.tagName === "TH") return { row, header: "row" };
    return { col: td.cellIndex - 1, row };
  }

  table.addEventListener("mousedown", (event) => {
    if (event.button !== 0 || event.target.classList.contains("resize")) return;
    const point = pointFromEvent(event);
    if (!point) return;
    if (editing) commit(null);
    event.preventDefault();
    wrap.focus({ preventScroll: true });
    if (point.all) {
      anchor = { col: 0, row: 0 };
      focus = { col: cols - 1, row: rows - 1 };
    } else if (point.header === "col") {
      if (!event.shiftKey) anchor = { col: point.col, row: 0 };
      focus = { col: point.col, row: rows - 1 };
    } else if (point.header === "row") {
      if (!event.shiftKey) anchor = { col: 0, row: point.row };
      focus = { col: cols - 1, row: point.row };
    } else {
      select(point, event.shiftKey);
      dragging = true;
      return;
    }
    paintSelection();
  });
  table.addEventListener("mouseover", (event) => {
    if (!dragging) return;
    const point = pointFromEvent(event);
    if (point && point.col !== undefined && point.row !== undefined) {
      focus = point;
      paintSelection();
    }
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
  });
  table.addEventListener("dblclick", (event) => {
    const point = pointFromEvent(event);
    if (point && point.col !== undefined && point.row !== undefined) startEditing("edit");
  });

  // Column resizing by dragging the edge of a column header.
  table.addEventListener("mousedown", (event) => {
    if (!event.target.classList.contains("resize")) return;
    event.preventDefault();
    const col = Number(event.target.dataset.col);
    const startX = event.clientX;
    const start = width(col);
    const colEl = colgroup.children[col + 1];
    let current = start;
    const move = (e) => {
      current = Math.min(800, Math.max(24, start + e.clientX - startX));
      colEl.style.width = current + "px";
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (current !== start) {
        apply(() => {
          const name = F.colName(col);
          if (current === DEFAULT_W) delete widths[name];
          else widths[name] = current;
        }, true);
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  });

  wrap.addEventListener("scroll", () => {
    window.requestAnimationFrame(() => renderRows(false));
  });

  // ---------------------------------------------------------------- keyboard

  wrap.addEventListener("keydown", (event) => {
    if (event.target !== wrap) return;
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key;
    const run = (fn) => {
      event.preventDefault();
      fn();
    };
    const from = event.shiftKey ? focus : anchor;
    const move = (dc, dr) => run(() => select({ col: from.col + dc, row: from.row + dr }, event.shiftKey));
    const jump = { ArrowUp: [0, -rows], ArrowDown: [0, rows], ArrowLeft: [-cols, 0], ArrowRight: [cols, 0] };
    if (mod && jump[key]) return move(...jump[key]);
    switch (key) {
      case "ArrowUp": return move(0, -1);
      case "ArrowDown": return move(0, 1);
      case "ArrowLeft": return move(-1, 0);
      case "ArrowRight": return move(1, 0);
      case "PageUp": return move(0, -20);
      case "PageDown": return move(0, 20);
      case "Home": return run(() => select({ col: 0, row: mod ? 0 : from.row }, event.shiftKey));
      case "End": return run(() => select({ col: cols - 1, row: mod ? rows - 1 : from.row }, event.shiftKey));
      case "Tab": return run(() => select({ col: anchor.col + (event.shiftKey ? -1 : 1), row: anchor.row }));
      case "Enter": return run(() => startEditing("edit"));
      case "F2": return run(() => startEditing("edit"));
      case "Delete":
      case "Backspace": return run(clearSelection);
    }
    if (mod && !event.altKey) {
      const lower = key.toLowerCase();
      if (lower === "z" && !event.shiftKey) return run(undo);
      if ((lower === "z" && event.shiftKey) || lower === "y") return run(redo);
      if (lower === "b") return run(() => toggleMark("bold"));
      if (lower === "i") return run(() => toggleMark("italic"));
      if (lower === "a") {
        return run(() => {
          anchor = { col: 0, row: 0 };
          focus = { col: cols - 1, row: rows - 1 };
          paintSelection();
        });
      }
      return;
    }
    if (key.length === 1 && !event.altKey) run(() => startEditing("enter", key));
  });

  // ---------------------------------------------------------------- toolbar

  for (const button of document.querySelectorAll(".toolbar [data-cmd]")) {
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", () => {
      if (editing) commit(null);
      const menu = button.closest("details");
      if (menu) menu.open = false;
      command(button.dataset.cmd);
      wrap.focus({ preventScroll: true });
    });
  }
  for (const button of document.querySelectorAll(".toolbar [data-align]")) {
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", () => {
      if (editing) commit(null);
      setAlign(button.dataset.align);
      wrap.focus({ preventScroll: true });
    });
  }
  formatSelect.addEventListener("change", () => {
    if (editing) commit(null);
    setFormat(formatSelect.value);
  });
  document.addEventListener("click", (event) => {
    for (const menu of document.querySelectorAll("details.menu[open]")) {
      if (!menu.contains(event.target)) menu.open = false;
    }
  });

  // ---------------------------------------------------------------- saving

  function body() {
    const out = {};
    for (const [key, cell] of cells) {
      const copy = Object.assign({}, cell);
      if (copy.input) {
        const value = values.get(key);
        if (value !== undefined && value !== null) copy.value = value;
      }
      out[key] = copy;
    }
    return { rows, cols, widths, cells: out };
  }

  const autosave = window.OfficeAutosave({
    url: "/api/sheets/" + encodeURIComponent(data.id),
    csrf: data.csrf,
    version: data.version,
    noun: "spreadsheet",
    body,
    focus: () => wrap.focus(),
  });

  // ---------------------------------------------------------------- start

  values = F.compute(cells, { rows, cols });
  buildTable();
  wrap.focus({ preventScroll: true });
  paintSelection();
})();
