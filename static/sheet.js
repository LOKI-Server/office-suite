// Sheets editor: a virtualised grid over a workbook of tabs.
//
// Cells hold what was typed; formula.js computes every value in every tab
// after each change. Saving posts each cell's input with its value, which the
// server uses for exports and the read-only view.
"use strict";
(() => {
  const F = window.SheetFormula;
  const data = JSON.parse(document.getElementById("doc-data").textContent);
  const $ = (id) => document.getElementById(id);
  const wrap = $("grid-wrap");
  const table = $("grid");
  const editorEl = $("cell-editor");
  const formulaInput = $("formula");
  const cellName = $("cell-name");
  const statsEl = $("selection-stats");
  const formatSelect = $("number-format");
  const importInput = $("import-csv");
  const exportLink = $("export-csv");
  const fillHandle = $("fill-handle");
  const tabsEl = $("tabs");
  const filterBar = $("filter-bar");
  const filterQuery = $("filter-query");

  const MAX_ROWS = 2000;
  const MAX_COLS = 52;
  const MAX_CELLS = 50000;
  const MAX_TABS = 20;
  const MAX_FROZEN_ROWS = 10;
  const MAX_FROZEN_COLS = 5;
  const ROW_H = 25;
  const HEAD_W = 48;
  const DEFAULT_W = 100;
  const BUFFER = 15;
  // Formatting a selection bigger than this only touches cells that have content.
  const FORMAT_ALL_LIMIT = 10000;

  // ---------------------------------------------------------------- state

  const fromSaved = (t) => ({
    name: t.name || "Sheet1",
    rows: t.rows,
    cols: t.cols,
    widths: Object.assign({}, t.widths || {}),
    // Cell objects are never changed in place, so snapshots can share them.
    cells: new Map(Object.entries(t.cells || {})),
    frozenRows: t.frozen_rows || 0,
    frozenCols: t.frozen_cols || 0,
    filter: t.filter || null,
  });
  let tabs = data.body.tabs.map(fromSaved);
  let active = 0;
  let values = [];
  // Rows the filter shows, in order, and each row's place among them (-1: hidden).
  let shown = [];
  let place = [];

  const tab = () => tabs[active];
  let anchor = { col: 0, row: 0 }; // the active cell
  let focus = { col: 0, row: 0 }; // the other corner of the selection
  // {tab, col, row, mode: "enter" | "edit", source: "cell" | "bar", point}
  let editing = null;
  let clip = null; // the last copy: {text, origin, grid}

  const width = (col) => tab().widths[F.colName(col)] || DEFAULT_W;
  const cellAt = (col, row) => tab().cells.get(F.keyOf(col, row));
  const valueAt = (col, row) => {
    const v = values[active] && values[active].get(F.keyOf(col, row));
    return v === undefined ? null : v;
  };
  const selection = () => ({
    c1: Math.min(anchor.col, focus.col),
    c2: Math.max(anchor.col, focus.col),
    r1: Math.min(anchor.row, focus.row),
    r2: Math.max(anchor.row, focus.row),
  });

  function setCell(key, patch, target) {
    const cells = (target || tab()).cells;
    const next = Object.assign({}, cells.get(key) || {}, patch);
    for (const k of Object.keys(next)) {
      if (next[k] === "" || next[k] === false || next[k] === null || next[k] === undefined) delete next[k];
    }
    if (Object.keys(next).length) cells.set(key, next);
    else cells.delete(key);
  }

  // Rewrites every formula in every tab with `fn(input, homeTab)`.
  function rewriteAll(fn) {
    for (const t of tabs) {
      for (const [key, cell] of t.cells) {
        if (cell.input && F.isFormula(cell.input)) {
          const input = fn(cell.input, t);
          if (input !== cell.input) t.cells.set(key, Object.assign({}, cell, { input }));
        }
      }
    }
  }

  // ---------------------------------------------------------------- history

  const undoStack = [];
  const redoStack = [];
  const snapshot = () => ({
    active,
    tabs: tabs.map((t) => Object.assign({}, t, {
      widths: Object.assign({}, t.widths),
      cells: new Map(t.cells),
      filter: t.filter && Object.assign({}, t.filter),
    })),
  });
  function restore(s) {
    tabs = s.tabs.map((t) => Object.assign({}, t, { widths: Object.assign({}, t.widths), cells: new Map(t.cells) }));
    active = Math.min(s.active, tabs.length - 1);
    anchor = clampPoint(anchor);
    focus = clampPoint(focus);
    changed(true);
  }
  // Runs `change` as one undoable step. Returns false if it was refused.
  function apply(change, structural) {
    const before = snapshot();
    if (change() === false) return false;
    if (tabs.reduce((n, t) => n + t.cells.size, 0) > MAX_CELLS) {
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

  // ---------------------------------------------------------------- filter

  // The last row with content, plus one.
  function usedRows(t) {
    let rows = 0;
    for (const [key, cell] of t.cells) {
      if (!cell.input) continue;
      const p = F.parseKey(key);
      if (p) rows = Math.max(rows, p.row + 1);
    }
    return rows;
  }

  // Header rows, rows past the data and rows matching the filter show; this
  // matches src/sheets/render.rs.
  function computeShown() {
    const t = tab();
    const f = t.filter;
    shown = [];
    place = new Array(t.rows).fill(-1);
    const header = Math.max(t.frozenRows, 1);
    const used = f ? usedRows(t) : 0;
    const query = f ? f.query.toLowerCase() : "";
    for (let r = 0; r < t.rows; r++) {
      let visible = true;
      if (f && query && r >= header && r < used) {
        const cell = cellAt(f.col, r);
        visible = F.display(valueAt(f.col, r), cell && cell.fmt).toLowerCase().includes(query);
      }
      if (visible) {
        place[r] = shown.length;
        shown.push(r);
      }
    }
    if (f && query) {
      const matching = shown.filter((r) => r >= header && r < used).length;
      $("filter-count").textContent = `${matching} of ${Math.max(0, used - header)} rows`;
    }
  }

  function showFilterBar() {
    const f = tab().filter;
    filterBar.hidden = !f;
    if (f) {
      $("filter-col").textContent = F.colName(f.col);
      if (document.activeElement !== filterQuery) filterQuery.value = f.query;
      if (!f.query) $("filter-count").textContent = "";
    }
  }

  // ---------------------------------------------------------------- rendering

  let colgroup, headRow, tbody, firstShown = -1, lastShown = -1;

  // Left offset of a frozen column, past the row header and earlier frozen columns.
  function frozenLeft(col) {
    let left = HEAD_W;
    for (let c = 0; c < col; c++) left += width(c);
    return left;
  }

  function buildTable() {
    const t = tab();
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
    for (let c = 0; c < t.cols; c++) {
      const col = document.createElement("col");
      col.style.width = width(c) + "px";
      colgroup.appendChild(col);
      const th = document.createElement("th");
      th.scope = "col";
      th.dataset.col = c;
      th.textContent = F.colName(c);
      if (c < t.frozenCols) {
        th.classList.add("frozen-col");
        th.style.left = frozenLeft(c) + "px";
      }
      if (c === t.frozenCols - 1) th.classList.add("frozen-edge");
      if (t.filter && t.filter.col === c) th.classList.add("filtered");
      const handle = document.createElement("span");
      handle.className = "resize";
      handle.dataset.col = c;
      th.appendChild(handle);
      headRow.appendChild(th);
    }
    let total = HEAD_W;
    for (let c = 0; c < t.cols; c++) total += width(c);
    table.style.width = total + "px";
    firstShown = lastShown = -1;
    renderRows(true);
    renderTabs();
    showFilterBar();
    exportLink.href = exportLink.href.split("?")[0] + "?tab=" + active;
  }

  function spacer(height) {
    const tr = document.createElement("tr");
    tr.className = "spacer";
    const td = document.createElement("td");
    td.colSpan = tab().cols + 1;
    td.style.height = height + "px";
    tr.appendChild(td);
    return tr;
  }

  function rowElement(r, frozenIndex) {
    const t = tab();
    const tr = document.createElement("tr");
    tr.dataset.row = r;
    const th = document.createElement("th");
    th.scope = "row";
    th.textContent = r + 1;
    tr.appendChild(th);
    if (frozenIndex !== undefined) {
      tr.className = "frozen-row";
      const top = headRow.offsetHeight + frozenIndex * ROW_H;
      th.style.top = top + "px";
      tr.dataset.top = top;
    }
    if (frozenIndex === t.frozenRows - 1) tr.classList.add("frozen-edge");
    for (let c = 0; c < t.cols; c++) {
      const td = document.createElement("td");
      if (c < t.frozenCols) {
        td.classList.add("frozen-col");
        td.style.left = frozenLeft(c) + "px";
      }
      if (c === t.frozenCols - 1) td.classList.add("frozen-edge-col");
      if (frozenIndex !== undefined) td.style.top = tr.dataset.top + "px";
      paintCell(td, c, r);
      tr.appendChild(td);
    }
    return tr;
  }

  // Draws the frozen rows and the shown rows near the viewport.
  function renderRows(force) {
    const t = tab();
    const frozen = Math.min(t.frozenRows, shown.length);
    const top = Math.max(0, wrap.scrollTop - ROW_H);
    const first = Math.max(frozen, frozen + Math.floor(top / ROW_H) - BUFFER);
    const last = Math.min(shown.length, frozen + Math.ceil((top + wrap.clientHeight) / ROW_H) + BUFFER);
    if (!force && first === firstShown && last === lastShown) return;
    firstShown = first;
    lastShown = last;
    const fragment = document.createDocumentFragment();
    for (let i = 0; i < frozen; i++) fragment.appendChild(rowElement(shown[i], i));
    if (first > frozen) fragment.appendChild(spacer((first - frozen) * ROW_H));
    for (let i = first; i < last; i++) fragment.appendChild(rowElement(shown[i]));
    if (last < shown.length) fragment.appendChild(spacer((shown.length - last) * ROW_H));
    tbody.replaceChildren(fragment);
    paintSelection();
    placeEditor();
  }

  function paintCell(td, col, row) {
    const cell = cellAt(col, row);
    const value = valueAt(col, row);
    td.textContent = F.display(value, cell && cell.fmt);
    let align = cell && cell.align;
    if (!align) {
      align = typeof value === "number" ? "right" : typeof value === "boolean" || F.isErr(value) ? "center" : "left";
    }
    const keep = ["frozen-col", "frozen-edge-col"].filter((c) => td.classList.contains(c));
    const classes = keep;
    if (align !== "left") classes.push("align-" + align);
    if (cell && cell.bold) classes.push("bold");
    if (cell && cell.italic) classes.push("italic");
    if (F.isErr(value)) classes.push("error");
    td.className = classes.join(" ");
  }

  const rowEl = (row) => tbody.querySelector(`tr[data-row="${row}"]`);
  function tdAt(col, row) {
    const tr = rowEl(row);
    return tr ? tr.cells[col + 1] || null : null;
  }

  function repaintValues() {
    for (const tr of tbody.querySelectorAll("tr[data-row]")) {
      const r = Number(tr.dataset.row);
      for (let c = 0; c < tab().cols; c++) paintCell(tr.cells[c + 1], c, r);
    }
    paintSelection();
  }

  let painted = [];
  function mark(el, cls) {
    el.classList.add(cls);
    painted.push([el, cls]);
  }
  function paintSelection() {
    for (const [el, cls] of painted) el.classList.remove(cls);
    painted = [];
    const { c1, c2, r1, r2 } = selection();
    const multi = c1 !== c2 || r1 !== r2;
    for (const tr of tbody.querySelectorAll("tr[data-row]")) {
      const r = Number(tr.dataset.row);
      if (r < r1 || r > r2) continue;
      mark(tr.cells[0], "head-selected");
      if (multi) for (let c = c1; c <= c2; c++) mark(tr.cells[c + 1], "selected");
    }
    for (let c = c1; c <= c2; c++) mark(headRow.cells[c + 1], "head-selected");
    const activeTd = tdAt(anchor.col, anchor.row);
    if (activeTd) mark(activeTd, "active");
    if (fillPreview) {
      for (let r = fillPreview.r1; r <= fillPreview.r2; r++) {
        for (let c = fillPreview.c1; c <= fillPreview.c2; c++) {
          const td = tdAt(c, r);
          if (td) mark(td, "fill-preview");
        }
      }
    }
    if (editing && editing.point) {
      const p = editing.point;
      for (let r = Math.min(p.r1, p.r2); r <= Math.max(p.r1, p.r2); r++) {
        for (let c = Math.min(p.c1, p.c2); c <= Math.max(p.c1, p.c2); c++) {
          const td = tdAt(c, r);
          if (td) mark(td, "pointed");
        }
      }
    }
    placeFillHandle();
    updateChrome();
  }

  // Positions an overlay over a cell, in the scrolling area's coordinates.
  function cellBox(td) {
    const box = td.getBoundingClientRect();
    const outer = wrap.getBoundingClientRect();
    return {
      left: box.left - outer.left + wrap.scrollLeft,
      top: box.top - outer.top + wrap.scrollTop,
      width: box.width,
      height: box.height,
    };
  }

  function placeFillHandle() {
    const { c2, r2 } = selection();
    const td = !editing && tdAt(c2, r2);
    fillHandle.hidden = !td;
    if (!td) return;
    const box = cellBox(td);
    fillHandle.style.left = box.left + box.width - 4 + "px";
    fillHandle.style.top = box.top + box.height - 4 + "px";
  }

  // The formula bar, toolbar state and selection statistics.
  function updateChrome() {
    const key = F.keyOf(anchor.col, anchor.row);
    const cell = tab().cells.get(key);
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
        if (place[r] < 0) continue;
        for (let c = c1; c <= c2; c++) {
          const v = valueAt(c, r);
          if (v !== null && v !== "") filled++;
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
    const t = tab();
    const index = place[row];
    if (index >= 0 && index >= t.frozenRows) {
      const headH = headRow.offsetHeight + t.frozenRows * ROW_H;
      const top = (index - t.frozenRows) * ROW_H;
      if (top < wrap.scrollTop) wrap.scrollTop = top;
      else if (top + ROW_H + headH > wrap.scrollTop + wrap.clientHeight) wrap.scrollTop = top + ROW_H + headH - wrap.clientHeight;
    }
    if (col >= t.frozenCols) {
      const pinned = frozenLeft(t.frozenCols) - HEAD_W;
      let left = 0;
      for (let c = t.frozenCols; c < col; c++) left += width(c);
      if (left < wrap.scrollLeft) wrap.scrollLeft = left;
      else if (left + width(col) + HEAD_W + pinned > wrap.scrollLeft + wrap.clientWidth) {
        wrap.scrollLeft = left + width(col) + HEAD_W + pinned - wrap.clientWidth;
      }
    }
    renderRows(false);
  }

  // Recomputes values after a change; `structural` rebuilds the grid.
  function changed(structural) {
    values = F.computeBook(tabs);
    const before = shown.length;
    computeShown();
    if (structural || shown.length !== before || tab().filter) buildTable();
    else repaintValues();
    autosave.changed();
  }

  // ---------------------------------------------------------------- tabs

  function renderTabs() {
    tabsEl.replaceChildren(...tabs.map((t, i) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "tab" + (i === active ? " active" : "");
      button.setAttribute("role", "tab");
      button.setAttribute("aria-selected", String(i === active));
      button.dataset.index = i;
      button.textContent = t.name;
      button.title = "Double-click to rename";
      return button;
    }));
  }

  function switchTab(index) {
    if (index === active || index < 0 || index >= tabs.length) return;
    if (editing) commit(null);
    tab().anchor = anchor;
    tab().focus = focus;
    tab().scroll = [wrap.scrollLeft, wrap.scrollTop];
    active = index;
    anchor = tab().anchor || { col: 0, row: 0 };
    focus = tab().focus || anchor;
    computeShown();
    buildTable();
    const [left, top] = tab().scroll || [0, 0];
    wrap.scrollLeft = left;
    wrap.scrollTop = top;
    renderRows(true);
    wrap.focus({ preventScroll: true });
  }

  const NAME_FORBIDDEN = /['!:\\/?*[\]]/;
  function checkName(name, except) {
    if (!name || name.length > 40 || name.trim() !== name || NAME_FORBIDDEN.test(name) || /[\u0000-\u001f]/.test(name)) {
      return "Tab names need 1 to 40 characters, without spaces at the ends or any of ' ! : \\ / ? * [ ].";
    }
    if (tabs.some((t, i) => i !== except && t.name.toLowerCase() === name.toLowerCase())) {
      return `There is already a tab named “${name}”.`;
    }
    return null;
  }
  // "Sheet2", "Sheet3", … for new tabs; "Budget (2)", … for copies.
  function freeName(base) {
    for (let n = 2; ; n++) {
      const name = base === "Sheet" ? `Sheet${n - 1}` : `${base.slice(0, 33)} (${n})`;
      if (!checkName(name)) return name;
    }
  }

  function addTab() {
    if (tabs.length >= MAX_TABS) return window.alert(`Spreadsheets can have up to ${MAX_TABS} tabs.`);
    apply(() => {
      tabs.push({ name: freeName("Sheet"), rows: 100, cols: 26, widths: {}, cells: new Map(), frozenRows: 0, frozenCols: 0, filter: null });
      active = tabs.length - 1;
      anchor = focus = { col: 0, row: 0 };
    }, true);
  }

  function renameTab() {
    const old = tab().name;
    const name = window.prompt("Rename this tab:", old);
    if (name === null || name === old) return;
    const problem = checkName(name, active);
    if (problem) return window.alert(problem);
    apply(() => {
      tab().name = name;
      rewriteAll((input) => F.renameSheet(input, old, name));
    }, true);
  }

  function tabAction(action) {
    if (editing) commit(null);
    switch (action) {
      case "rename": return renameTab();
      case "duplicate": {
        if (tabs.length >= MAX_TABS) return window.alert(`Spreadsheets can have up to ${MAX_TABS} tabs.`);
        return apply(() => {
          const copy = Object.assign({}, tab(), {
            name: freeName(tab().name),
            widths: Object.assign({}, tab().widths),
            cells: new Map(tab().cells),
            filter: tab().filter && Object.assign({}, tab().filter),
          });
          tabs.splice(active + 1, 0, copy);
          active += 1;
        }, true);
      }
      case "left":
      case "right": {
        const to = active + (action === "left" ? -1 : 1);
        if (to < 0 || to >= tabs.length) return;
        return apply(() => {
          [tabs[active], tabs[to]] = [tabs[to], tabs[active]];
          active = to;
        }, true);
      }
      case "delete": {
        if (tabs.length === 1) return window.alert("A spreadsheet needs at least one tab.");
        const name = tab().name;
        if (!window.confirm(`Delete the tab “${name}”? Formulas that use it will show #REF!.`)) return;
        return apply(() => {
          tabs.splice(active, 1);
          active = Math.max(0, active - 1);
          rewriteAll((input) => F.dropSheet(input, name));
          anchor = focus = { col: 0, row: 0 };
        }, true);
      }
    }
  }

  tabsEl.addEventListener("click", (event) => {
    const button = event.target.closest(".tab");
    if (button) switchTab(Number(button.dataset.index));
  });
  tabsEl.addEventListener("dblclick", (event) => {
    if (event.target.closest(".tab")) renameTab();
  });
  $("tab-add").addEventListener("click", () => {
    if (editing) commit(null);
    addTab();
  });
  for (const button of document.querySelectorAll("[data-tab]")) {
    button.addEventListener("click", () => {
      button.closest("details").open = false;
      tabAction(button.dataset.tab);
    });
  }

  // ---------------------------------------------------------------- selection

  const clampPoint = (p) => ({
    col: Math.min(Math.max(p.col, 0), tab().cols - 1),
    row: Math.min(Math.max(p.row, 0), tab().rows - 1),
  });

  function select(point, extend) {
    point = clampPoint(point);
    if (extend) focus = point;
    else anchor = focus = point;
    scrollIntoView(extend ? focus.col : anchor.col, extend ? focus.row : anchor.row);
    paintSelection();
  }

  // Moves `steps` shown rows from `row`, skipping rows the filter hides.
  function stepRow(row, steps) {
    const index = place[row] >= 0 ? place[row] : shown.findIndex((r) => r > row);
    const target = Math.min(Math.max(index + steps, 0), shown.length - 1);
    return shown[target];
  }

  // ---------------------------------------------------------------- editing

  const activeInput = () => (editing && editing.source === "bar" ? formulaInput : editorEl);

  function startEditing(mode, text) {
    const cell = cellAt(anchor.col, anchor.row);
    editing = { tab: active, col: anchor.col, row: anchor.row, mode, source: "cell", point: null };
    editorEl.value = text !== undefined ? text : cell ? cell.input || "" : "";
    formulaInput.value = editorEl.value;
    editorEl.hidden = false;
    fillHandle.hidden = true;
    placeEditor();
    editorEl.focus();
    const end = editorEl.value.length;
    editorEl.setSelectionRange(end, end);
  }

  function placeEditor() {
    if (!editing || editing.source !== "cell" || editing.tab !== active) return;
    const td = tdAt(editing.col, editing.row);
    if (!td) return;
    const box = cellBox(td);
    editorEl.style.left = box.left + "px";
    editorEl.style.top = box.top + "px";
    editorEl.style.minWidth = box.width + "px";
    editorEl.style.height = box.height + "px";
  }

  // Saves the edit; `move` then moves the active cell by {col, row}.
  function commit(move) {
    if (!editing) return;
    const { tab: t, col, row, source } = editing;
    const text = (source === "bar" ? formulaInput.value : editorEl.value).replace(/\r/g, "");
    if (new TextEncoder().encode(text).length > 8192) {
      cancel();
      window.alert("A cell can hold up to 8 KB of text, so that entry wasn't kept.");
      return;
    }
    editing = null;
    editorEl.hidden = true;
    const key = F.keyOf(col, row);
    const before = (tabs[t].cells.get(key) || {}).input || "";
    if (text !== before) apply(() => setCell(key, { input: text }, tabs[t]));
    wrap.focus({ preventScroll: true });
    if (move && t === active) {
      select({ col: col + move.col, row: move.row ? stepRow(row, move.row) : row });
    } else paintSelection();
  }

  function cancel() {
    if (!editing) return;
    editing = null;
    editorEl.hidden = true;
    wrap.focus({ preventScroll: true });
    paintSelection();
  }

  // While typing a formula, clicking cells inserts their references: right
  // after an operator, a comma or an opening bracket, or over the reference
  // that the last click inserted.
  function canPoint() {
    if (!editing || editing.tab !== active) return false;
    const input = activeInput();
    if (!F.isFormula(input.value) && input.value !== "=") return false;
    const caret = input.selectionStart;
    if (editing.point && editing.point.end === caret && input.selectionEnd === caret) return true;
    if (input.selectionStart !== input.selectionEnd) return false;
    return /[=(,+\-*/^&<>:]\s*$/.test(input.value.slice(0, caret));
  }

  function pointAt(cell, extend) {
    const input = activeInput();
    const p = editing.point;
    const range = extend && p ? { c1: p.c1, r1: p.r1, c2: cell.col, r2: cell.row } : { c1: cell.col, r1: cell.row, c2: cell.col, r2: cell.row };
    const home = tabs[editing.tab].name;
    const text = F.pointText(tab().name, home, range.c1, range.r1) +
      (range.c1 !== range.c2 || range.r1 !== range.r2 ? ":" + F.keyOf(range.c2, range.r2) : "");
    const start = p && p.end === input.selectionStart ? p.start : input.selectionStart;
    const end = p && p.end === input.selectionStart ? p.end : input.selectionEnd;
    input.value = input.value.slice(0, start) + text + input.value.slice(end);
    const caret = start + text.length;
    input.setSelectionRange(caret, caret);
    editing.point = Object.assign(range, { start, end: caret });
    if (input === editorEl) formulaInput.value = editorEl.value;
    paintSelection();
  }

  editorEl.addEventListener("input", () => {
    formulaInput.value = editorEl.value;
    if (editing) editing.point = null;
    paintSelection();
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
    } else if (editing && editing.mode === "enter" && arrows[event.key] && !canPoint()) {
      // Typing over a cell: arrows finish the entry, as in other spreadsheets.
      event.preventDefault();
      const [col, row] = arrows[event.key];
      commit({ col, row });
    }
  });
  editorEl.addEventListener("blur", () => {
    if (editing && editing.source === "cell" && !pointing) commit(null);
  });

  formulaInput.addEventListener("focus", () => {
    if (editing && editing.source === "bar") return;
    if (editing && editing.source === "cell") commit(null);
    editing = { tab: active, col: anchor.col, row: anchor.row, mode: "edit", source: "bar", point: null };
  });
  formulaInput.addEventListener("input", () => {
    if (editing) editing.point = null;
    paintSelection();
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
    if (editing && editing.source === "bar" && !pointing) commit(null);
  });

  // ---------------------------------------------------------------- formatting

  // Calls `fn(key, col, row)` for every cell in the selection, or only the
  // filled ones when the selection is very large.
  function eachSelected(fn) {
    const { c1, c2, r1, r2 } = selection();
    if ((c2 - c1 + 1) * (r2 - r1 + 1) > FORMAT_ALL_LIMIT) {
      for (const key of Array.from(tab().cells.keys())) {
        const p = F.parseKey(key);
        if (p && p.col >= c1 && p.col <= c2 && p.row >= r1 && p.row <= r2) fn(key, p.col, p.row);
      }
      return;
    }
    for (let r = r1; r <= r2; r++) for (let c = c1; c <= c2; c++) fn(F.keyOf(c, r), c, r);
  }

  function toggleMark(markName) {
    const on = !(cellAt(anchor.col, anchor.row) || {})[markName];
    apply(() => eachSelected((key) => setCell(key, { [markName]: on })));
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

  // ---------------------------------------------------------------- rows and columns

  // Moves the active tab's cells along one axis. `move(index)` returns a
  // cell's new index, or null to drop it.
  function shiftCells(axis, move) {
    const next = new Map();
    for (const [key, cell] of tab().cells) {
      const p = F.parseKey(key);
      if (!p) continue;
      const index = move(axis === "row" ? p.row : p.col);
      if (index === null) continue;
      next.set(axis === "row" ? F.keyOf(p.col, index) : F.keyOf(index, p.row), cell);
    }
    tab().cells = next;
  }
  function shiftWidths(move) {
    const next = {};
    for (const [name, w] of Object.entries(tab().widths)) {
      const index = move(F.colIndex(name));
      if (index !== null) next[F.colName(index)] = w;
    }
    tab().widths = next;
  }

  function insert(axis, at, count) {
    const t = tab();
    const limit = axis === "row" ? MAX_ROWS : MAX_COLS;
    if ((axis === "row" ? t.rows : t.cols) + count > limit) {
      window.alert(`Spreadsheets can have up to ${limit} ${axis === "row" ? "rows" : "columns"}.`);
      return;
    }
    apply(() => {
      const target = t.name;
      rewriteAll((input, home) => F.insertShift(input, axis, at, count, { home: home.name, name: target }));
      const move = (i) => (i >= at ? i + count : i);
      shiftCells(axis, move);
      if (axis === "row") t.rows += count;
      else {
        t.cols += count;
        shiftWidths(move);
      }
    }, true);
  }

  function remove(axis, at, count) {
    const t = tab();
    if (count >= (axis === "row" ? t.rows : t.cols)) {
      window.alert(`A tab needs at least one ${axis === "row" ? "row" : "column"}.`);
      return;
    }
    apply(() => {
      const target = t.name;
      rewriteAll((input, home) => F.deleteShift(input, axis, at, count, { home: home.name, name: target }));
      const move = (i) => (i < at ? i : i < at + count ? null : i - count);
      shiftCells(axis, move);
      if (axis === "row") {
        t.rows -= count;
        t.frozenRows = Math.min(t.frozenRows, t.rows - 1);
      } else {
        t.cols -= count;
        t.frozenCols = Math.min(t.frozenCols, t.cols - 1);
        shiftWidths(move);
        if (t.filter && t.filter.col >= at) {
          t.filter = t.filter.col < at + count ? null : Object.assign({}, t.filter, { col: t.filter.col - count });
        }
      }
      anchor = clampPoint(anchor);
      focus = clampPoint(focus);
    }, true);
  }

  // ---------------------------------------------------------------- sort, freeze, filter, fill

  // Sorts the selected rows (only the selected columns move), or with a
  // single cell selected every row below the frozen ones, by the active column.
  function sort(descending) {
    const t = tab();
    const { c1, c2, r1, r2 } = selection();
    const whole = r1 === r2;
    const top = whole ? t.frozenRows : r1;
    const bottom = whole ? usedRows(t) - 1 : r2;
    const left = whole ? 0 : c1, right = whole ? t.cols - 1 : c2;
    if (bottom <= top) return;
    const rows = [];
    for (let r = top; r <= bottom; r++) rows.push(r);
    const sorted = rows
      .map((r, i) => ({ r, i, v: valueAt(anchor.col, r) }))
      .sort((a, b) => F.sortCompare(a.v, b.v, descending) || a.i - b.i)
      .map((x) => x.r);
    apply(() => {
      const size = { rows: t.rows, cols: t.cols };
      const moved = new Map();
      sorted.forEach((from, i) => {
        const to = rows[i];
        for (let c = left; c <= right; c++) {
          const cell = t.cells.get(F.keyOf(c, from));
          if (!cell) continue;
          const input = cell.input && F.isFormula(cell.input) ? F.moveFormula(cell.input, 0, to - from, size) : cell.input;
          moved.set(F.keyOf(c, to), input === cell.input ? cell : Object.assign({}, cell, { input }));
        }
      });
      for (const r of rows) for (let c = left; c <= right; c++) t.cells.delete(F.keyOf(c, r));
      for (const [key, cell] of moved) t.cells.set(key, cell);
    });
  }

  function freeze(kind) {
    const t = tab();
    apply(() => {
      if (kind === "rows") t.frozenRows = Math.min(anchor.row + 1, MAX_FROZEN_ROWS, t.rows - 1);
      else if (kind === "cols") t.frozenCols = Math.min(anchor.col + 1, MAX_FROZEN_COLS, t.cols - 1);
      else t.frozenRows = t.frozenCols = 0;
    }, true);
  }

  function startFilter() {
    const t = tab();
    if (!t.filter || t.filter.col !== anchor.col) t.filter = { col: anchor.col, query: "" };
    computeShown();
    buildTable();
    filterQuery.focus();
  }
  // Filter changes save but are not undo steps, so typing a query stays quick.
  let filterTimer = null;
  filterQuery.addEventListener("input", () => {
    clearTimeout(filterTimer);
    filterTimer = setTimeout(() => {
      tab().filter = Object.assign({}, tab().filter, { query: filterQuery.value.slice(0, 200) });
      wrap.scrollTop = 0;
      computeShown();
      buildTable();
      autosave.changed();
    }, 150);
  });
  filterQuery.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === "Escape") {
      event.preventDefault();
      wrap.focus({ preventScroll: true });
    }
  });
  $("filter-clear").addEventListener("click", () => {
    tab().filter = null;
    computeShown();
    buildTable();
    autosave.changed();
    wrap.focus({ preventScroll: true });
  });

  // Fills `target` (a block beside the selection) from the selection: numbers
  // that step evenly continue their series; anything else repeats, with
  // formulas following as when pasted.
  function fill(source, target, direction) {
    const t = tab();
    const size = { rows: t.rows, cols: t.cols };
    const down = direction === "down";
    const lines = down ? source.c2 - source.c1 + 1 : source.r2 - source.r1 + 1;
    const length = down ? source.r2 - source.r1 + 1 : source.c2 - source.c1 + 1;
    apply(() => {
      for (let l = 0; l < lines; l++) {
        const at = (i) => (down ? { col: source.c1 + l, row: source.r1 + i } : { col: source.c1 + i, row: source.r1 + l });
        const sources = Array.from({ length }, (_, i) => {
          const p = at(i);
          return { p, cell: t.cells.get(F.keyOf(p.col, p.row)) };
        });
        const numbers = sources.map((s) => (s.cell && s.cell.input && !F.isFormula(s.cell.input) ? F.literal(s.cell.input) : null));
        const series = length > 1 && numbers.every((n) => typeof n === "number");
        const step = series ? (numbers[length - 1] - numbers[0]) / (length - 1) : 0;
        const start = down ? target.r1 : target.c1;
        const end = down ? target.r2 : target.c2;
        for (let k = start; k <= end; k++) {
          const offset = k - (down ? source.r1 : source.c1);
          const from = sources[offset % length];
          const p = down ? { col: source.c1 + l, row: k } : { col: k, row: source.r1 + l };
          const key = F.keyOf(p.col, p.row);
          if (!from.cell) {
            t.cells.delete(key);
            continue;
          }
          let input = from.cell.input || "";
          if (series) input = F.general(numbers[0] + step * offset);
          else if (F.isFormula(input)) input = F.moveFormula(input, p.col - from.p.col, p.row - from.p.row, size);
          const copy = Object.assign({}, from.cell, { input });
          if (!copy.input) delete copy.input;
          t.cells.set(key, copy);
        }
      }
    });
  }

  // Ctrl+D / Ctrl+R: copy the first row (column) of the selection into the
  // rest, or, with one row (column) selected, the one above (left of) it.
  function fillDownOrRight(direction) {
    const { c1, c2, r1, r2 } = selection();
    if (direction === "down") {
      if (r1 === r2 && r1 === 0) return;
      const sourceRow = r1 === r2 ? r1 - 1 : r1;
      fill({ c1, c2, r1: sourceRow, r2: sourceRow }, { c1, c2, r1: sourceRow + 1, r2 }, "down");
    } else {
      if (c1 === c2 && c1 === 0) return;
      const sourceCol = c1 === c2 ? c1 - 1 : c1;
      fill({ c1: sourceCol, c2: sourceCol, r1, r2 }, { c1: sourceCol + 1, c2, r1, r2 }, "right");
    }
  }

  // ---------------------------------------------------------------- commands

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
      case "sortAsc": return sort(false);
      case "sortDesc": return sort(true);
      case "filter": return startFilter();
      case "freezeRows": return freeze("rows");
      case "freezeCols": return freeze("cols");
      case "unfreeze": return freeze(null);
      case "fillDown": return fillDownOrRight("down");
      case "fillRight": return fillDownOrRight("right");
      case "addRows": {
        const count = Math.min(100, MAX_ROWS - tab().rows);
        if (count > 0) apply(() => { tab().rows += count; }, true);
        return;
      }
    }
  }

  // ---------------------------------------------------------------- clipboard

  function selectionGrid() {
    const { c1, c2, r1, r2 } = selection();
    const grid = [];
    for (let r = r1; r <= r2; r++) {
      if (place[r] < 0) continue;
      const line = [];
      for (let c = c1; c <= c2; c++) line.push(cellAt(c, r) || null);
      grid.push(line);
    }
    return grid;
  }

  function copySelection(event) {
    const { c1, r1, r2, c2 } = selection();
    const rowsShown = [];
    for (let r = r1; r <= r2; r++) if (place[r] >= 0) rowsShown.push(r);
    const text = F.toDelimited(
      rowsShown.map((r) => {
        const line = [];
        for (let c = c1; c <= c2; c++) {
          const cell = cellAt(c, r);
          line.push(F.display(valueAt(c, r), cell && cell.fmt));
        }
        return line;
      }),
      "\t",
    );
    clip = { text, origin: { col: c1, row: r1 }, grid: selectionGrid() };
    event.clipboardData.setData("text/plain", text);
    event.preventDefault();
  }

  // Pastes a grid of cells (from this spreadsheet) or of typed text at the active cell.
  function paste(text) {
    const t = tab();
    const internal = clip && clip.text === text;
    const grid = internal ? clip.grid : F.parseDelimited(text.replace(/\r?\n$/, ""), "\t").map((line) => line.map((input) => ({ input })));
    if (!grid.length) return;
    const { c1, c2, r1, r2 } = selection();
    // One copied cell fills the whole selection.
    const single = grid.length === 1 && grid[0].length === 1 && (c1 !== c2 || r1 !== r2);
    const height = single ? r2 - r1 + 1 : grid.length;
    const widthCells = single ? c2 - c1 + 1 : Math.max(...grid.map((line) => line.length));
    if (r1 + height > MAX_ROWS || c1 + widthCells > MAX_COLS) {
      window.alert("That doesn't fit: tabs can have up to 2,000 rows and 52 columns.");
      return;
    }
    apply(() => {
      t.rows = Math.max(t.rows, r1 + height);
      t.cols = Math.max(t.cols, c1 + widthCells);
      for (let r = 0; r < height; r++) {
        for (let c = 0; c < widthCells; c++) {
          const source = single ? grid[0][0] : (grid[r] || [])[c];
          const key = F.keyOf(c1 + c, r1 + r);
          if (!source) {
            if (internal) t.cells.delete(key);
            continue;
          }
          let input = source.input || "";
          if (internal && F.isFormula(input)) {
            const from = single ? clip.origin : { col: clip.origin.col + c, row: clip.origin.row + r };
            input = F.moveFormula(input, c1 + c - from.col, r1 + r - from.row, t);
          }
          if (internal) {
            const copy = Object.assign({}, source, { input });
            if (!copy.input) delete copy.input;
            t.cells.set(key, copy);
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
    const tabbed = /\.tsv$/i.test(file.name) || (!text.split("\n", 1)[0].includes(",") && text.includes("\t"));
    const grid = F.parseDelimited(text, tabbed ? "\t" : ",");
    const height = grid.length;
    const widthCells = Math.max(0, ...grid.map((line) => line.length));
    if (!height) return;
    if (height > MAX_ROWS || widthCells > MAX_COLS) {
      window.alert(`That file has ${height} rows and ${widthCells} columns; tabs can have up to ${MAX_ROWS} rows and ${MAX_COLS} columns.`);
      return;
    }
    const t = tab();
    if (t.cells.size && !window.confirm(`Replace everything in the tab “${t.name}” with ${file.name}?`)) return;
    apply(() => {
      t.cells = new Map();
      t.filter = null;
      t.rows = Math.max(t.rows, height);
      t.cols = Math.max(t.cols, widthCells);
      grid.forEach((line, r) => line.forEach((input, c) => {
        if (input) t.cells.set(F.keyOf(c, r), { input });
      }));
      anchor = focus = { col: 0, row: 0 };
    }, true);
  });

  // ---------------------------------------------------------------- mouse

  let dragging = false;
  let pointing = false;
  let fillDrag = null;
  let fillPreview = null;

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
  const isCell = (p) => p && p.col !== undefined && p.row !== undefined && !p.header;

  table.addEventListener("mousedown", (event) => {
    if (event.button !== 0 || event.target.classList.contains("resize")) return;
    const point = pointFromEvent(event);
    if (!point) return;
    if (isCell(point) && canPoint()) {
      // Keep typing the formula: insert the clicked cell's reference.
      event.preventDefault();
      pointing = true;
      pointAt(point, event.shiftKey);
      return;
    }
    if (editing) commit(null);
    event.preventDefault();
    wrap.focus({ preventScroll: true });
    const t = tab();
    if (point.all) {
      anchor = { col: 0, row: 0 };
      focus = { col: t.cols - 1, row: t.rows - 1 };
    } else if (point.header === "col") {
      if (!event.shiftKey) anchor = { col: point.col, row: 0 };
      focus = { col: point.col, row: t.rows - 1 };
    } else if (point.header === "row") {
      if (!event.shiftKey) anchor = { col: 0, row: point.row };
      focus = { col: t.cols - 1, row: point.row };
    } else {
      select(point, event.shiftKey);
      dragging = true;
      return;
    }
    paintSelection();
  });
  table.addEventListener("mouseover", (event) => {
    const point = pointFromEvent(event);
    if (!isCell(point)) return;
    if (pointing && editing) {
      pointAt(point, true);
    } else if (fillDrag) {
      const { c1, c2, r1, r2 } = fillDrag;
      const down = point.row - r2, right = point.col - c2;
      if (down > 0 && down >= right) fillPreview = { c1, c2, r1: r2 + 1, r2: point.row, direction: "down" };
      else if (right > 0) fillPreview = { c1: c2 + 1, c2: point.col, r1, r2, direction: "right" };
      else fillPreview = null;
      paintSelection();
    } else if (dragging) {
      focus = point;
      paintSelection();
    }
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
    if (pointing) {
      pointing = false;
      activeInput().focus();
    }
    if (fillDrag) {
      const source = fillDrag, target = fillPreview;
      fillDrag = fillPreview = null;
      if (target) {
        fill(source, target, target.direction);
        anchor = { col: source.c1, row: source.r1 };
        focus = { col: target.c2, row: target.r2 };
      }
      paintSelection();
    }
  });
  table.addEventListener("dblclick", (event) => {
    if (isCell(pointFromEvent(event)) && !editing) startEditing("edit");
  });
  fillHandle.addEventListener("mousedown", (event) => {
    event.preventDefault();
    event.stopPropagation();
    fillDrag = selection();
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
          if (current === DEFAULT_W) delete tab().widths[name];
          else tab().widths[name] = current;
        }, true);
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  });

  wrap.addEventListener("scroll", () => {
    window.requestAnimationFrame(() => {
      renderRows(false);
      placeEditor();
      placeFillHandle();
    });
  });

  // ---------------------------------------------------------------- keyboard

  wrap.addEventListener("keydown", (event) => {
    if (event.target !== wrap) return;
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key;
    const t = tab();
    const run = (fn) => {
      event.preventDefault();
      fn();
    };
    const from = event.shiftKey ? focus : anchor;
    const move = (dc, dr) => run(() => select({ col: from.col + dc, row: dr ? stepRow(from.row, dr) : from.row }, event.shiftKey));
    const jump = { ArrowUp: [0, -t.rows], ArrowDown: [0, t.rows], ArrowLeft: [-t.cols, 0], ArrowRight: [t.cols, 0] };
    if (mod && jump[key]) return move(...jump[key]);
    if (mod && key === "PageDown") return run(() => switchTab(active + 1));
    if (mod && key === "PageUp") return run(() => switchTab(active - 1));
    switch (key) {
      case "ArrowUp": return move(0, -1);
      case "ArrowDown": return move(0, 1);
      case "ArrowLeft": return move(-1, 0);
      case "ArrowRight": return move(1, 0);
      case "PageUp": return move(0, -20);
      case "PageDown": return move(0, 20);
      case "Home": return run(() => select({ col: 0, row: mod ? shown[0] : from.row }, event.shiftKey));
      case "End": return run(() => select({ col: t.cols - 1, row: mod ? shown[shown.length - 1] : from.row }, event.shiftKey));
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
      if (lower === "d") return run(() => fillDownOrRight("down"));
      if (lower === "r") return run(() => fillDownOrRight("right"));
      if (lower === "a") {
        return run(() => {
          anchor = { col: 0, row: 0 };
          focus = { col: t.cols - 1, row: t.rows - 1 };
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
      if (button.dataset.cmd !== "filter") wrap.focus({ preventScroll: true });
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
    return {
      tabs: tabs.map((t, i) => {
        const cells = {};
        for (const [key, cell] of t.cells) {
          const copy = Object.assign({}, cell);
          if (copy.input) {
            const value = values[i] && values[i].get(key);
            if (value !== undefined && value !== null) copy.value = value;
          }
          cells[key] = copy;
        }
        const out = { name: t.name, rows: t.rows, cols: t.cols, widths: t.widths, cells };
        if (t.frozenRows) out.frozen_rows = t.frozenRows;
        if (t.frozenCols) out.frozen_cols = t.frozenCols;
        if (t.filter && t.filter.query) out.filter = t.filter;
        return out;
      }),
    };
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

  values = F.computeBook(tabs);
  computeShown();
  buildTable();
  wrap.focus({ preventScroll: true });
  paintSelection();
})();
