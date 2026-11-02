// Docs editor: a contenteditable surface over the document model.
//
// The page embeds the model as JSON; the editor renders it to DOM, reads the
// DOM back into the model on save, and posts only the model, never HTML.
// Undo history is kept here as snapshots so custom block operations undo
// alongside typing.
"use strict";
(() => {
  const data = JSON.parse(document.getElementById("doc-data").textContent);
  const editor = document.getElementById("editor");
  const wordsEl = document.getElementById("word-count");
  const styleSelect = document.getElementById("block-style");

  const BLOCK_TAGS = new Set([
    "P", "DIV", "H1", "H2", "H3", "H4", "H5", "H6", "BLOCKQUOTE", "PRE", "UL", "OL", "LI", "HR",
    "TABLE", "THEAD", "TBODY", "TFOOT", "TR", "TD", "TH", "CAPTION", "SECTION", "ARTICLE", "HEADER",
    "FOOTER", "MAIN", "ASIDE", "NAV", "FIGURE", "FIGCAPTION", "DL", "DT", "DD", "ADDRESS", "CENTER",
  ]);
  const SKIP_TAGS = new Set([
    "SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT", "IFRAME", "OBJECT", "EMBED", "SVG", "CANVAS", "VIDEO",
    "AUDIO", "HEAD", "TITLE", "META", "LINK", "BUTTON", "INPUT", "SELECT", "TEXTAREA", "IMG",
  ]);
  // Elements that start a line; each counts as one position when saving the selection.
  const UNIT_TAGS = new Set(["P", "DIV", "H1", "H2", "H3", "H4", "H5", "H6", "BLOCKQUOTE", "PRE", "LI"]);
  const ALIGNS = ["center", "right", "justify"];
  const MARK_TAGS = [["code", "code"], ["strike", "s"], ["underline", "u"], ["italic", "em"], ["bold", "strong"]];

  const el = (tag) => document.createElement(tag);
  const br = () => el("br");

  // ---------------------------------------------------------------- model → DOM

  function renderRuns(runs, parent) {
    for (const run of runs || []) {
      let node = document.createDocumentFragment();
      run.text.split("\n").forEach((line, i) => {
        if (i) node.appendChild(br());
        if (line) node.appendChild(document.createTextNode(line));
      });
      for (const [flag, tag] of MARK_TAGS) {
        if (run[flag]) {
          const wrapper = el(tag);
          wrapper.appendChild(node);
          node = wrapper;
        }
      }
      if (run.link) {
        const a = el("a");
        a.href = run.link;
        a.appendChild(node);
        node = a;
      }
      parent.appendChild(node);
    }
    return parent;
  }

  // An element whose content ends in a line break needs one more <br> to show it.
  function finishLine(node) {
    if (!node.lastChild || node.lastChild.nodeName === "BR") node.appendChild(br());
    return node;
  }

  function aligned(node, align) {
    if (ALIGNS.includes(align)) node.classList.add("align-" + align);
    return node;
  }

  function renderBlock(block) {
    switch (block.type) {
      case "heading":
        return aligned(finishLine(renderRuns(block.runs, el("h" + Math.min(Math.max(block.level, 1), 3)))), block.align);
      case "quote":
        return finishLine(renderRuns(block.runs, el("blockquote")));
      case "code": {
        const pre = el("pre");
        pre.textContent = block.text;
        if (!block.text || block.text.endsWith("\n")) pre.appendChild(br());
        return pre;
      }
      case "bullet_list":
      case "ordered_list": {
        const list = el(block.type === "bullet_list" ? "ul" : "ol");
        for (const item of block.items) list.appendChild(finishLine(renderRuns(item, el("li"))));
        return list;
      }
      case "divider":
        return el("hr");
      default:
        return aligned(finishLine(renderRuns(block.runs, el("p"))), block.align);
    }
  }

  function renderBlocks(blocks) {
    const fragment = document.createDocumentFragment();
    for (const block of blocks) fragment.appendChild(renderBlock(block));
    return fragment;
  }

  // ---------------------------------------------------------------- DOM → model

  function cleanLink(href) {
    const link = (href || "").trim();
    if (/^(https?:\/\/|mailto:)\S+$/i.test(link) && !/["<>]/.test(link) && link.length <= 2048) return link;
    return null;
  }

  function marksFor(node, marks) {
    const m = Object.assign({}, marks);
    switch (node.nodeName) {
      case "B": case "STRONG": m.bold = true; break;
      case "I": case "EM": case "CITE": case "DFN": m.italic = true; break;
      case "U": case "INS": m.underline = true; break;
      case "S": case "STRIKE": case "DEL": m.strike = true; break;
      case "CODE": case "KBD": case "SAMP": case "TT": m.code = true; break;
      case "A": {
        const link = cleanLink(node.getAttribute("href"));
        if (link) m.link = link;
        break;
      }
    }
    const style = node.style;
    if (style) {
      const weight = style.fontWeight;
      if (weight === "bold" || weight === "bolder" || parseInt(weight, 10) >= 600) m.bold = true;
      else if (weight === "normal" || weight === "lighter" || parseInt(weight, 10) < 600) m.bold = false;
      if (style.fontStyle === "italic" || style.fontStyle === "oblique") m.italic = true;
      else if (style.fontStyle === "normal") m.italic = false;
      const decoration = style.textDecorationLine || style.textDecoration || "";
      if (decoration.includes("underline")) m.underline = true;
      if (decoration.includes("line-through")) m.strike = true;
      if (/monospace|courier|consolas/i.test(style.fontFamily)) m.code = true;
    }
    return m;
  }

  function pushText(runs, text, marks) {
    if (!text) return;
    const run = { text };
    for (const flag of ["bold", "italic", "underline", "strike", "code"]) if (marks[flag]) run[flag] = true;
    if (marks.link) run.link = marks.link;
    runs.push(run);
  }

  const sameMarks = (a, b) =>
    ["bold", "italic", "underline", "strike", "code", "link"].every((k) => (a[k] || false) === (b[k] || false));

  // Merges runs, drops the filler line break, and in pasted content trims the ends.
  function finishRuns(runs, pasted) {
    const merged = [];
    for (const run of runs) {
      const last = merged[merged.length - 1];
      if (last && sameMarks(last, run)) last.text += run.text;
      else merged.push(run);
    }
    const last = merged[merged.length - 1];
    if (last && last.text.endsWith("\n")) last.text = last.text.slice(0, -1);
    if (pasted && merged.length) {
      merged[0].text = merged[0].text.replace(/^[ \n]+/, "");
      const end = merged[merged.length - 1];
      end.text = end.text.replace(/[ \n]+$/, "");
    }
    return merged.filter((run) => run.text);
  }

  function collectInline(node, runs, marks, pasted) {
    if (node.nodeType === Node.TEXT_NODE) {
      let text = node.data.replace(/ /g, " ");
      if (pasted) text = text.replace(/[ \t\r\n]+/g, " ");
      pushText(runs, text.replace(/\r/g, ""), marks);
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE || SKIP_TAGS.has(node.nodeName)) return;
    if (node.nodeName === "BR") {
      pushText(runs, "\n", marks);
      return;
    }
    const isBlock = BLOCK_TAGS.has(node.nodeName);
    const needsBreak = () => runs.length && !runs[runs.length - 1].text.endsWith("\n");
    if (isBlock && needsBreak()) pushText(runs, "\n", {});
    const inner = marksFor(node, marks);
    for (const child of node.childNodes) collectInline(child, runs, inner, pasted);
    if (isBlock && node.nodeName !== "LI" && needsBreak()) pushText(runs, "\n", {});
  }

  function inlineRuns(node, pasted) {
    const runs = [];
    for (const child of node.childNodes) collectInline(child, runs, {}, pasted);
    return finishRuns(runs, pasted);
  }

  function alignOf(node) {
    for (const align of ALIGNS) if (node.classList.contains("align-" + align)) return align;
    const style = (node.style && node.style.textAlign) || node.getAttribute("align") || "";
    return ALIGNS.includes(style) ? style : null;
  }

  function preText(node) {
    let text = "";
    const walk = (n) => {
      for (const child of n.childNodes) {
        if (child.nodeType === Node.TEXT_NODE) text += child.data;
        else if (child.nodeName === "BR") text += "\n";
        else if (child.nodeType === Node.ELEMENT_NODE && !SKIP_TAGS.has(child.nodeName)) {
          if (BLOCK_TAGS.has(child.nodeName) && text && !text.endsWith("\n")) text += "\n";
          walk(child);
        }
      }
    };
    walk(node);
    if (node.lastChild && node.lastChild.nodeName === "BR") text = text.replace(/\n$/, "");
    return text.replace(/ /g, " ").replace(/\r/g, "");
  }

  const hasBlockChild = (node) => Array.from(node.children).some((c) => BLOCK_TAGS.has(c.nodeName));
  const BLOCK_SELECTOR = Array.from(BLOCK_TAGS).join(",");
  const hasBlockDescendant = (node) => !!node.querySelector(BLOCK_SELECTOR);

  // Reads a container's DOM into model blocks. `pasted` collapses HTML whitespace.
  function serialize(root, pasted) {
    const blocks = [];
    let loose = null; // inline content outside any block
    const flush = () => {
      if (loose) {
        const runs = finishRuns(loose, pasted);
        if (runs.length) blocks.push({ type: "paragraph", runs });
        loose = null;
      }
    };
    const paragraph = (node, runs) => {
      const block = { type: "paragraph", runs };
      const align = alignOf(node);
      if (align) block.align = align;
      return block;
    };
    const listItems = (list, items) => {
      for (const child of list.childNodes) {
        if (child.nodeName === "LI") {
          // Nested lists become following items of this one.
          const nested = Array.from(child.children).filter((c) => c.nodeName === "UL" || c.nodeName === "OL");
          nested.forEach((n) => n.remove());
          items.push(inlineRuns(child, pasted));
          nested.forEach((n) => listItems(n, items));
        } else if (child.nodeName === "UL" || child.nodeName === "OL") {
          listItems(child, items);
        } else if (child.textContent.trim()) {
          const runs = [];
          collectInline(child, runs, {}, pasted);
          items.push(finishRuns(runs, pasted));
        }
      }
      return items;
    };
    const visit = (node) => {
      if (node.nodeType === Node.TEXT_NODE) {
        if (!loose && !node.data.trim()) return;
        loose = loose || [];
        collectInline(node, loose, {}, pasted);
        return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE || SKIP_TAGS.has(node.nodeName)) return;
      const tag = node.nodeName;
      if (!BLOCK_TAGS.has(tag) && hasBlockDescendant(node)) {
        // Inline wrappers around blocks, like Google Docs' outer <b>, are containers.
        flush();
        for (const child of node.childNodes) visit(child);
        flush();
        return;
      }
      if (!BLOCK_TAGS.has(tag)) {
        loose = loose || [];
        collectInline(node, loose, {}, pasted);
        return;
      }
      flush();
      if (/^H[1-6]$/.test(tag)) {
        const block = { type: "heading", level: Math.min(Number(tag[1]), 3), runs: inlineRuns(node, pasted) };
        const align = alignOf(node);
        if (align) block.align = align;
        if (!pasted || block.runs.length) blocks.push(block);
      } else if (tag === "BLOCKQUOTE") {
        const runs = inlineRuns(node, pasted);
        if (!pasted || runs.length) blocks.push({ type: "quote", runs });
      } else if (tag === "PRE") {
        blocks.push({ type: "code", text: preText(node) });
      } else if (tag === "UL" || tag === "OL") {
        const items = listItems(node.cloneNode(true), []);
        if (items.length) blocks.push({ type: tag === "UL" ? "bullet_list" : "ordered_list", items });
      } else if (tag === "LI") {
        const last = blocks[blocks.length - 1];
        const runs = inlineRuns(node, pasted);
        if (last && last.type === "bullet_list") last.items.push(runs);
        else blocks.push({ type: "bullet_list", items: [runs] });
      } else if (tag === "HR") {
        blocks.push({ type: "divider" });
      } else if (tag === "P" || !hasBlockChild(node)) {
        const runs = inlineRuns(node, pasted);
        if (!pasted || runs.length) blocks.push(paragraph(node, runs));
      } else {
        for (const child of node.childNodes) visit(child);
        flush();
      }
    };
    for (const child of root.childNodes) visit(child);
    flush();
    return blocks;
  }

  // ---------------------------------------------------------------- selection as text offsets
  // Block operations move nodes around, which would collapse a live Range, so
  // the selection is saved as a position in the text: characters, line breaks
  // and the start of each line-level element each count as one.

  function lengthOf(node) {
    let length = 0;
    const walk = (n) => {
      if (n.nodeType === Node.TEXT_NODE) length += n.length;
      else if (n.nodeName === "BR") length += 1;
      else {
        if (UNIT_TAGS.has(n.nodeName)) length += 1;
        for (const child of n.childNodes) walk(child);
      }
    };
    for (const child of node.childNodes) walk(child);
    return length;
  }

  function offsetOf(container, offset) {
    const range = document.createRange();
    range.setStart(editor, 0);
    range.setEnd(container, offset);
    return lengthOf(range.cloneContents());
  }

  function pointAt(target) {
    const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    let position = 0;
    let last = null;
    const index = (n) => Array.prototype.indexOf.call(n.parentNode.childNodes, n);
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (node.nodeType === Node.TEXT_NODE) {
        if (position + node.length >= target) return [node, target - position];
        position += node.length;
        last = node;
      } else if (node.nodeName === "BR") {
        if (position === target) return [node.parentNode, index(node)];
        position += 1;
        last = node;
      } else if (UNIT_TAGS.has(node.nodeName)) {
        position += 1;
        if (position === target && !node.firstChild) return [node, 0];
        last = node;
      }
    }
    if (!last) return [editor, 0];
    if (last.nodeType === Node.TEXT_NODE) return [last, last.length];
    if (last.nodeName === "BR") return [last.parentNode, index(last) + 1];
    return [last, last.childNodes.length];
  }

  function saveSelection() {
    const range = currentRange();
    if (!range) return null;
    return { start: offsetOf(range.startContainer, range.startOffset), end: offsetOf(range.endContainer, range.endOffset) };
  }

  function restoreSelection(saved) {
    if (!saved) return;
    const range = document.createRange();
    try {
      range.setStart(...pointAt(saved.start));
      range.setEnd(...pointAt(saved.end));
    } catch (_) {
      return;
    }
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    lastRange = range.cloneRange();
  }

  let lastRange = null;
  function currentRange() {
    const selection = window.getSelection();
    if (selection.rangeCount) {
      const range = selection.getRangeAt(0);
      if (editor.contains(range.commonAncestorContainer)) return range;
    }
    return lastRange && editor.contains(lastRange.commonAncestorContainer) ? lastRange : null;
  }

  // Puts the selection back in the editor after a toolbar control took focus.
  function focusEditor() {
    const range = currentRange();
    editor.focus({ preventScroll: true });
    if (range) {
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
  }

  // ---------------------------------------------------------------- undo history

  const history = { stack: [], index: -1, timer: null };

  function record() {
    clearTimeout(history.timer);
    history.timer = null;
    const snapshot = { html: editor.innerHTML, selection: saveSelection() };
    const current = history.stack[history.index];
    if (current && current.html === snapshot.html) {
      current.selection = snapshot.selection;
      return;
    }
    history.stack.length = history.index + 1;
    history.stack.push(snapshot);
    if (history.stack.length > 200) history.stack.shift();
    history.index = history.stack.length - 1;
  }

  function recordSoon() {
    clearTimeout(history.timer);
    history.timer = setTimeout(record, 400);
  }

  function travel(step) {
    if (history.timer) record();
    const next = history.index + step;
    if (next < 0 || next >= history.stack.length) return;
    history.index = next;
    const snapshot = history.stack[next];
    editor.innerHTML = snapshot.html;
    editor.focus({ preventScroll: true });
    restoreSelection(snapshot.selection);
    changed();
  }

  // Runs a DOM change as one undoable step.
  function mutate(change) {
    focusEditor();
    if (history.timer) record();
    change();
    ensureStructure();
    record();
    changed();
  }

  // ---------------------------------------------------------------- structure helpers

  function topLevel(node, offset) {
    if (node === editor) {
      return editor.childNodes[Math.min(offset, editor.childNodes.length - 1)] || null;
    }
    while (node && node.parentNode !== editor) node = node.parentNode;
    return node;
  }

  function closest(node, tags) {
    for (let n = node; n && n !== editor; n = n.parentNode) if (tags.includes(n.nodeName)) return n;
    return null;
  }

  // The line-level elements the selection touches: top-level blocks, or list items.
  function selectedUnits() {
    const range = currentRange();
    if (!range) return [];
    const start = topLevel(range.startContainer, range.startOffset);
    let end = topLevel(range.endContainer, range.endOffset);
    if (!range.collapsed && range.endOffset === 0 && end !== start && end.previousSibling) end = end.previousSibling;
    const units = [];
    for (let block = start; block; block = block.nextSibling) {
      if (block.nodeName === "UL" || block.nodeName === "OL") {
        for (const li of block.children) if (li.nodeName === "LI" && range.intersectsNode(li)) units.push(li);
      } else if (block.nodeType === Node.ELEMENT_NODE) {
        units.push(block);
      }
      if (block === end) break;
    }
    return units;
  }

  function moveInline(from, to) {
    if (from.nodeName === "PRE") {
      to.textContent = preText(from);
      finishLine(to);
    } else {
      // Live content already carries any line-break filler it needs.
      while (from.firstChild) to.appendChild(from.firstChild);
      if (!to.firstChild) to.appendChild(br());
    }
    return to;
  }

  // Replaces each unit with build(unit), splitting lists around converted items.
  function replaceUnits(units, build) {
    const chosen = new Set(units);
    const lists = new Set();
    for (const unit of units) {
      if (unit.nodeName === "LI") lists.add(unit.parentNode);
      else unit.replaceWith(build(unit));
    }
    for (const list of lists) {
      const pieces = [];
      let kept = null;
      for (const li of Array.from(list.children)) {
        if (chosen.has(li)) {
          kept = null;
          pieces.push(build(li));
        } else {
          if (!kept) {
            kept = list.cloneNode(false);
            pieces.push(kept);
          }
          kept.appendChild(li);
        }
      }
      list.replaceWith(...pieces);
    }
  }

  // Wraps top-level <li> runs in lists of `tag` and joins them to neighbouring ones.
  function gatherItems(tag) {
    for (let child = editor.firstChild; child; ) {
      if (child.nodeName !== "LI") {
        child = child.nextSibling;
        continue;
      }
      let list = el(tag);
      child.before(list);
      while (list.nextSibling && list.nextSibling.nodeName === "LI") list.appendChild(list.nextSibling);
      const previous = list.previousSibling;
      if (previous && previous.nodeName === list.nodeName) {
        previous.append(...list.childNodes);
        list.remove();
        list = previous;
      }
      const next = list.nextSibling;
      if (next && next.nodeName === list.nodeName) {
        list.append(...next.childNodes);
        next.remove();
      }
      child = list.nextSibling;
    }
  }

  // Keeps every piece of top-level content inside a block element.
  function ensureStructure() {
    if (!editor.firstElementChild && !editor.textContent.trim()) {
      editor.replaceChildren(finishLine(el("p")));
      if (document.activeElement === editor) {
        const range = document.createRange();
        range.setStart(editor.firstChild, 0);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
      }
    }
    const stray = Array.from(editor.childNodes).filter(
      (n) => (n.nodeType === Node.TEXT_NODE && n.data.trim()) ||
        (n.nodeType === Node.ELEMENT_NODE && !BLOCK_TAGS.has(n.nodeName) && n.nodeName !== "BR"),
    );
    if (stray.length) {
      const saved = saveSelection();
      for (const node of stray) {
        if (!node.parentNode) continue;
        const p = el("p");
        node.before(p);
        while (p.nextSibling && (p.nextSibling.nodeType === Node.TEXT_NODE ||
          (!BLOCK_TAGS.has(p.nextSibling.nodeName)))) {
          p.appendChild(p.nextSibling);
        }
        finishLine(p);
      }
      restoreSelection(saved);
    }
    editor.classList.toggle(
      "is-empty",
      editor.childNodes.length === 1 && editor.firstChild.nodeName === "P" && !editor.textContent,
    );
  }

  // ---------------------------------------------------------------- commands

  function setBlockStyle(tag) {
    mutate(() => {
      const saved = saveSelection();
      replaceUnits(selectedUnits(), (unit) => {
        const node = moveInline(unit, el(tag));
        const align = /^(P|H\d|DIV)$/.test(unit.nodeName) && alignOf(unit);
        if (align && /^(p|h\d)$/.test(tag)) aligned(node, align);
        return node;
      });
      restoreSelection(saved);
    });
  }

  function toggleList(tag) {
    const units = selectedUnits();
    if (!units.length) return;
    if (units.every((u) => u.nodeName === "LI" && u.parentNode.nodeName === tag.toUpperCase())) {
      setBlockStyle("p");
      return;
    }
    mutate(() => {
      const saved = saveSelection();
      replaceUnits(units.filter((u) => u.nodeName !== "HR"), (unit) => moveInline(unit, el("li")));
      gatherItems(tag);
      restoreSelection(saved);
    });
  }

  function setAlign(align) {
    const units = selectedUnits().filter((u) => /^(P|H[1-6]|DIV)$/.test(u.nodeName));
    if (!units.length) return;
    const target = align !== "left" && units.every((u) => alignOf(u) === align) ? "left" : align;
    mutate(() => {
      for (const unit of units) {
        for (const a of ALIGNS) unit.classList.remove("align-" + a);
        unit.style.removeProperty("text-align");
        unit.removeAttribute("align");
        if (target !== "left") unit.classList.add("align-" + target);
        if (!unit.classList.length) unit.removeAttribute("class");
      }
    });
  }

  function insertDivider() {
    mutate(() => {
      const range = currentRange();
      const block = range ? topLevel(range.startContainer, range.startOffset) : editor.lastChild;
      const hr = el("hr");
      const after = finishLine(el("p"));
      if (block && block.nodeName === "P" && !block.textContent) {
        block.replaceWith(hr, after);
      } else if (block) {
        block.after(hr, after);
      } else {
        editor.append(hr, after);
      }
      const caret = document.createRange();
      caret.setStart(after, 0);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(caret);
    });
  }

  function toggleCode() {
    const range = currentRange();
    if (!range) return;
    const code = closest(range.commonAncestorContainer, ["CODE"]);
    if (code && !closest(code, ["PRE"])) {
      mutate(() => {
        const saved = saveSelection();
        code.replaceWith(...code.childNodes);
        restoreSelection(saved);
      });
      return;
    }
    if (range.collapsed || closest(range.commonAncestorContainer, ["PRE"])) return;
    const unit = (n) => closest(n, Array.from(UNIT_TAGS)) || topLevel(n, 0);
    if (unit(range.startContainer) !== unit(range.endContainer)) return;
    mutate(() => {
      const saved = saveSelection();
      const r = currentRange();
      const fragment = r.extractContents();
      fragment.querySelectorAll("code").forEach((c) => c.replaceWith(...c.childNodes));
      const wrapper = el("code");
      wrapper.appendChild(fragment);
      r.insertNode(wrapper);
      restoreSelection(saved);
    });
  }

  function normalizeUrl(input) {
    const text = input.trim();
    if (/^(https?:\/\/|mailto:)/i.test(text)) return cleanLink(text);
    if (/^[^\s@/]+@[^\s@/]+\.[^\s@/]+$/.test(text)) return "mailto:" + text;
    if (/^[\w-]+(\.[\w-]+)+([/?#]\S*)?$/.test(text)) return "https://" + text;
    return null;
  }

  function editLink() {
    const range = currentRange();
    if (!range) return;
    const anchor = closest(range.commonAncestorContainer, ["A"]);
    const answer = window.prompt(
      anchor ? "Edit the link address, or clear it to remove the link:" : "Link to a web or email address:",
      anchor ? anchor.getAttribute("href") : "https://",
    );
    if (answer === null) {
      focusEditor();
      return;
    }
    focusEditor();
    if (anchor) {
      const whole = document.createRange();
      whole.selectNodeContents(anchor);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(whole);
      if (!answer.trim()) {
        document.execCommand("unlink");
        return;
      }
    }
    const url = normalizeUrl(answer);
    if (!url) {
      window.alert("Links must be web addresses (https://…) or email addresses.");
      return;
    }
    document.execCommand("createLink", false, url);
  }

  function clearFormatting() {
    focusEditor();
    document.execCommand("removeFormat");
    document.execCommand("unlink");
  }

  function command(name) {
    switch (name) {
      case "undo": return travel(-1);
      case "redo": return travel(1);
      case "code": return toggleCode();
      case "link": return editLink();
      case "clear": return clearFormatting();
      case "divider": return insertDivider();
      case "insertUnorderedList": return toggleList("ul");
      case "insertOrderedList": return toggleList("ol");
      default:
        focusEditor();
        document.execCommand(name);
    }
  }

  // In a code block Enter adds a line; Enter on an empty last line, or
  // Shift+Enter, leaves the block.
  function handleCodeEnter(pre, shift) {
    const range = currentRange();
    const rest = document.createRange();
    rest.setStart(range.endContainer, range.endOffset);
    rest.setEnd(pre, pre.childNodes.length);
    const atEnd = !rest.toString().length;
    const text = preText(pre);
    if (shift || (atEnd && (text.endsWith("\n") || !text))) {
      mutate(() => {
        const trimmed = text.replace(/\n$/, "");
        const after = finishLine(el("p"));
        if (!trimmed) {
          pre.replaceWith(after);
        } else {
          pre.textContent = trimmed;
          pre.after(after);
        }
        const caret = document.createRange();
        caret.setStart(after, 0);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(caret);
      });
      return;
    }
    document.execCommand("insertText", false, "\n");
    // A trailing newline only shows when something follows it.
    if (atEnd && (!pre.lastChild || pre.lastChild.nodeName !== "BR")) {
      const saved = saveSelection();
      pre.appendChild(br());
      restoreSelection(saved);
    }
  }

  // ---------------------------------------------------------------- toolbar state

  const toolButtons = Array.from(document.querySelectorAll(".toolbar [data-cmd]"));
  const alignButtons = Array.from(document.querySelectorAll(".toolbar [data-align]"));

  function updateToolbar() {
    const range = currentRange();
    if (!range) return;
    const node = range.startContainer;
    const states = {
      bold: document.queryCommandState("bold"),
      italic: document.queryCommandState("italic"),
      underline: document.queryCommandState("underline"),
      strikeThrough: document.queryCommandState("strikeThrough"),
      code: !!closest(node, ["CODE"]) && !closest(node, ["PRE"]),
      link: !!closest(node, ["A"]),
      insertUnorderedList: !!closest(node, ["UL"]),
      insertOrderedList: !!closest(node, ["OL"]),
    };
    for (const button of toolButtons) {
      if (button.dataset.cmd in states) button.setAttribute("aria-pressed", String(states[button.dataset.cmd]));
    }
    const block = topLevel(node, range.startOffset);
    const tag = block && block.nodeName ? block.nodeName.toLowerCase() : "p";
    styleSelect.value = ["h1", "h2", "h3", "blockquote", "pre"].includes(tag) ? tag : "p";
    const align = (block && block.nodeType === Node.ELEMENT_NODE && alignOf(block)) || "left";
    for (const button of alignButtons) button.setAttribute("aria-pressed", String(button.dataset.align === align));
  }

  // ---------------------------------------------------------------- saving

  const autosave = window.OfficeAutosave({
    url: "/api/docs/" + encodeURIComponent(data.id),
    csrf: data.csrf,
    version: data.version,
    noun: "document",
    body: () => ({ blocks: serialize(editor, false) }),
    focus: () => editor.focus(),
  });

  function changed() {
    autosave.changed();
    countWordsSoon();
    updateToolbar();
  }

  let countTimer = null;
  function countWordsSoon() {
    clearTimeout(countTimer);
    countTimer = setTimeout(() => {
      const words = (editor.innerText.match(/\S+/g) || []).length;
      wordsEl.textContent = words === 1 ? "1 word" : words.toLocaleString() + " words";
    }, 250);
  }

  // ---------------------------------------------------------------- paste and drop

  function placeCaretAfter(node) {
    const caret = document.createRange();
    if (node.nodeType === Node.ELEMENT_NODE && BLOCK_TAGS.has(node.nodeName)) {
      let target = node;
      while (target.lastElementChild && BLOCK_TAGS.has(target.lastElementChild.nodeName)) target = target.lastElementChild;
      const last = target.lastChild;
      if (last && last.nodeName === "BR") caret.setStartBefore(last);
      else caret.selectNodeContents(target);
      caret.collapse(false);
    } else {
      caret.setStartAfter(node);
    }
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(caret);
  }

  // Inserts model blocks at the selection. The DOM is built here rather than
  // with insertHTML, which nests blocks and adds inline styles.
  function insertModel(blocks) {
    if (!blocks.length) return;
    const range = currentRange();
    if (!range) return;
    const unit = closest(range.startContainer, Array.from(UNIT_TAGS));
    const inlineOnly = blocks.length === 1 && blocks[0].type === "paragraph";
    if (!inlineOnly && unit && unit.nodeName === "LI") {
      // Lists hold lines, not blocks: paste each block as a new item.
      document.execCommand("insertText", false, blocks.map((b) => blockText(b)).join("\n"));
      return;
    }
    mutate(() => {
      const r = currentRange();
      r.deleteContents();
      if (inlineOnly || !unit) {
        const runs = inlineOnly ? blocks[0].runs : [{ text: blocks.map(blockText).join("\n") }];
        const fragment = renderRuns(runs, document.createDocumentFragment());
        const last = fragment.lastChild;
        if (!last) return;
        r.insertNode(fragment);
        placeCaretAfter(last);
        return;
      }
      const top = topLevel(unit, 0);
      const tailRange = document.createRange();
      tailRange.setStart(r.startContainer, r.startOffset);
      tailRange.setEnd(top, top.childNodes.length);
      const tail = top.cloneNode(false);
      tail.appendChild(tailRange.extractContents());
      const rendered = renderBlocks(blocks);
      const lastBlock = rendered.lastChild;
      top.after(rendered);
      lastBlock.after(tail);
      if (!top.textContent.trim()) top.remove();
      else if (!top.lastChild) top.appendChild(br());
      if (!tail.textContent.trim()) tail.remove();
      placeCaretAfter(lastBlock);
    });
  }

  function blockText(block) {
    if (block.type === "code") return block.text;
    const runs = block.runs || (block.items || []).flatMap((item, i) => (i ? [{ text: "\n" }] : []).concat(item));
    return runs.map((r) => r.text).join("");
  }

  function insertTransfer(transfer) {
    const html = transfer.getData("text/html");
    const text = transfer.getData("text/plain");
    if (closest(currentRange() && currentRange().startContainer, ["PRE"])) {
      document.execCommand("insertText", false, (text || "").replace(/\r\n?/g, "\n"));
    } else if (html) {
      const parsed = new DOMParser().parseFromString(html, "text/html");
      insertModel(serialize(parsed.body, true));
    } else if (text) {
      document.execCommand("insertText", false, text.replace(/\r\n?/g, "\n"));
    }
  }

  editor.addEventListener("paste", (event) => {
    if (!event.clipboardData) return;
    event.preventDefault();
    insertTransfer(event.clipboardData);
  });

  editor.addEventListener("drop", (event) => {
    const transfer = event.dataTransfer;
    if (!transfer || transfer.files.length || !(transfer.types.includes("text/html") || transfer.types.includes("text/plain"))) {
      if (transfer && transfer.files.length) event.preventDefault();
      return;
    }
    event.preventDefault();
    const point = document.caretRangeFromPoint
      ? document.caretRangeFromPoint(event.clientX, event.clientY)
      : null;
    if (point) {
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(point);
    }
    insertTransfer(transfer);
  });

  // ---------------------------------------------------------------- events

  editor.addEventListener("input", () => {
    ensureStructure();
    recordSoon();
    changed();
  });

  editor.addEventListener("beforeinput", (event) => {
    if (event.inputType === "historyUndo" || event.inputType === "historyRedo") {
      event.preventDefault();
      travel(event.inputType === "historyUndo" ? -1 : 1);
    }
  });

  editor.addEventListener("keydown", (event) => {
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    const run = (fn) => {
      event.preventDefault();
      fn();
    };
    if (mod && !event.altKey) {
      if (key === "z" && !event.shiftKey) return run(() => travel(-1));
      if ((key === "z" && event.shiftKey) || key === "y") return run(() => travel(1));
      if (key === "k") return run(editLink);
      if (key === "e" && !event.shiftKey) return run(toggleCode);
      if (key === "\\") return run(clearFormatting);
      if (event.shiftKey && event.code === "Digit7") return run(() => toggleList("ol"));
      if (event.shiftKey && event.code === "Digit8") return run(() => toggleList("ul"));
      const aligns = { l: "left", e: "center", r: "right", j: "justify" };
      if (event.shiftKey && aligns[key]) return run(() => setAlign(aligns[key]));
    }
    if (mod && event.altKey && /^Digit[0-3]$/.test(event.code)) {
      const level = event.code.slice(-1);
      return run(() => setBlockStyle(level === "0" ? "p" : "h" + level));
    }
    if (event.altKey && event.shiftKey && event.code === "Digit5") return run(() => command("strikeThrough"));
    const range = currentRange();
    const pre = range && closest(range.startContainer, ["PRE"]);
    if (pre && event.key === "Enter" && !mod) return run(() => handleCodeEnter(pre, event.shiftKey));
    if (pre && event.key === "Tab" && !mod) return run(() => document.execCommand("insertText", false, "\t"));
  });

  editor.addEventListener("click", (event) => {
    const anchor = event.target.closest && event.target.closest("a");
    if (anchor && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      const link = cleanLink(anchor.getAttribute("href"));
      if (link) window.open(link, "_blank", "noopener,noreferrer");
    }
  });

  document.addEventListener("selectionchange", () => {
    const selection = window.getSelection();
    if (selection.rangeCount && editor.contains(selection.anchorNode)) {
      lastRange = selection.getRangeAt(0).cloneRange();
      updateToolbar();
    }
  });

  for (const button of [...toolButtons, ...alignButtons]) {
    // Keep the editor's selection when pressing a toolbar button.
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", () => {
      if (button.dataset.cmd) command(button.dataset.cmd);
      else setAlign(button.dataset.align);
    });
  }
  styleSelect.addEventListener("change", () => setBlockStyle(styleSelect.value));

  // ---------------------------------------------------------------- start

  document.execCommand("defaultParagraphSeparator", false, "p");
  document.execCommand("styleWithCSS", false, false);
  editor.replaceChildren(renderBlocks(data.body.blocks || []));
  ensureStructure();
  record();
  countWordsSoon();
  if (!data.body.blocks || !data.body.blocks.length) {
    editor.focus();
    const range = document.createRange();
    range.setStart(editor.firstChild, 0);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  }
})();
