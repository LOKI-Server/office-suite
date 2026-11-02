// Sheets formula engine: reads cell inputs, evaluates formulas across a
// workbook's tabs, formats values, and rewrites references when rows,
// columns or tabs change. It has no DOM dependencies, so
// tests/formula.test.js runs it under Node.
//
// Values are null (empty), numbers, strings, booleans or {error: "#CODE"}.
// References may name a tab: Sheet2!A1, 'Q1 budget'!B2:B9.
"use strict";
(function (root) {
  const ERRORS = ["#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#ERROR!", "#CYCLE!"];
  const err = (code) => ({ error: code });
  const isErr = (v) => v !== null && typeof v === "object" && typeof v.error === "string";
  const isRange = (v) => v !== null && typeof v === "object" && Array.isArray(v.values);

  // ------------------------------------------------------------------ names

  function colName(col) {
    let name = "";
    for (let n = col + 1; n > 0; n = Math.floor((n - 1) / 26)) {
      name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
    }
    return name;
  }
  function colIndex(letters) {
    let n = 0;
    for (const ch of letters.toUpperCase()) n = n * 26 + (ch.charCodeAt(0) - 64);
    return n - 1;
  }
  const keyOf = (col, row) => colName(col) + (row + 1);
  function parseKey(key) {
    const m = /^([A-Z]{1,2})([1-9]\d{0,5})$/.exec(key);
    return m ? { col: colIndex(m[1]), row: Number(m[2]) - 1 } : null;
  }
  function parseRef(text, sheet) {
    const m = /^(\$?)([A-Za-z]{1,2})(\$?)([1-9]\d{0,5})$/.exec(text);
    if (!m) return null;
    return { col: colIndex(m[2]), row: Number(m[4]) - 1, colAbs: !!m[1], rowAbs: !!m[3], sheet: sheet || null };
  }
  // Tab names never contain quotes, so quoting needs no escapes.
  function quoteSheet(name) {
    return /^[A-Za-z_][A-Za-z0-9_.]*$/.test(name) && !/^[A-Za-z]{1,2}\d+$/.test(name) ? name : "'" + name + "'";
  }
  const sameSheet = (a, b) => a.toLowerCase() === b.toLowerCase();
  const cellText = (ref) =>
    (ref.colAbs ? "$" : "") + colName(ref.col) + (ref.rowAbs ? "$" : "") + (ref.row + 1);
  const refText = (ref) => (ref.sheet ? quoteSheet(ref.sheet) + "!" : "") + cellText(ref);
  // A reference to `name`'s cell, as typed in a formula on `home`.
  function pointText(name, home, col, row) {
    return (name && !sameSheet(name, home) ? quoteSheet(name) + "!" : "") + keyOf(col, row);
  }

  // ------------------------------------------------------------------ inputs

  const NUMBER = /^[-+]?(\d+(\.\d*)?|\.\d+)(e[-+]?\d+)?%?$/i;

  // The value of a non-formula input: a number, TRUE/FALSE, or text. A leading
  // apostrophe forces text.
  function literal(input) {
    if (input === "") return null;
    if (input[0] === "'") return input.slice(1);
    const t = input.trim();
    if (NUMBER.test(t)) {
      const percent = t.endsWith("%");
      const n = Number(percent ? t.slice(0, -1) : t);
      // Too large to hold, such as 1e999: keep it as text.
      if (Number.isFinite(n)) return percent ? n / 100 : n;
    }
    if (/^true$/i.test(t)) return true;
    if (/^false$/i.test(t)) return false;
    return input;
  }
  const isFormula = (input) => input.length > 1 && input[0] === "=";

  // ------------------------------------------------------------------ tokens

  const TOKEN = new RegExp(
    [
      /(\s+)/.source, // 1 space
      /((?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?)/.source, // 2 number
      /("(?:[^"]|"")*")/.source, // 3 string
      /(#(?:DIV\/0!|VALUE!|REF!|NAME\?|NUM!|N\/A|ERROR!|CYCLE!))/.source, // 4 error
      /((?:'[^']+'|[A-Za-z_][A-Za-z0-9_.]*)!)?(\$?[A-Za-z]{1,2}\$?[1-9]\d{0,5})(?![A-Za-z0-9_.(!])/.source, // 5 tab, 6 reference
      /([A-Za-z_][A-Za-z0-9_.]*)/.source, // 7 name
      /(<=|>=|<>|[-+*/^&=<>%:(),])/.source, // 8 operator
    ].join("|"),
    "y",
  );

  function tokenize(source) {
    const tokens = [];
    TOKEN.lastIndex = 0;
    while (TOKEN.lastIndex < source.length) {
      const start = TOKEN.lastIndex;
      const m = TOKEN.exec(source);
      if (!m) throw new SyntaxError("Unexpected character");
      const end = TOKEN.lastIndex;
      if (m[1]) continue;
      if (m[2]) tokens.push({ type: "num", value: Number(m[2]), start, end });
      else if (m[3]) tokens.push({ type: "str", value: m[3].slice(1, -1).replace(/""/g, '"'), start, end });
      else if (m[4]) tokens.push({ type: "err", value: m[4], start, end });
      else if (m[6]) {
        const sheet = m[5] ? m[5].slice(0, -1).replace(/^'(.*)'$/, "$1") : null;
        tokens.push({ type: "ref", value: parseRef(m[6], sheet), start, end });
      } else if (m[7]) tokens.push({ type: "name", value: m[7].toUpperCase(), start, end });
      else tokens.push({ type: "op", value: m[8], start, end });
    }
    return tokens;
  }

  // ------------------------------------------------------------------ parser
  // Precedence, low to high: comparison, &, + -, * /, ^, %, unary -, range.

  function parse(source) {
    const tokens = tokenize(source);
    let i = 0;
    const peek = () => tokens[i];
    const isOp = (value) => peek() && peek().type === "op" && peek().value === value;
    const expect = (value) => {
      if (!isOp(value)) throw new SyntaxError("Expected " + value);
      i++;
    };
    function binary(next, ops) {
      return () => {
        let left = next();
        while (peek() && peek().type === "op" && ops.includes(peek().value)) {
          const op = tokens[i++].value;
          left = { t: "bin", op, a: left, b: next() };
        }
        return left;
      };
    }
    function power() {
      let left = percent();
      while (isOp("^")) {
        i++;
        left = { t: "bin", op: "^", a: left, b: percent() };
      }
      return left;
    }
    function percent() {
      let x = unary();
      while (isOp("%")) {
        i++;
        x = { t: "pct", x };
      }
      return x;
    }
    function unary() {
      if (isOp("-") || isOp("+")) {
        const op = tokens[i++].value;
        return { t: "un", op, x: unary() };
      }
      return primary();
    }
    function primary() {
      const token = tokens[i++];
      if (!token) throw new SyntaxError("Unexpected end");
      switch (token.type) {
        case "num": return { t: "val", v: token.value };
        case "str": return { t: "val", v: token.value };
        case "err": return { t: "val", v: err(token.value) };
        case "ref":
          if (isOp(":") && tokens[i + 1] && tokens[i + 1].type === "ref") {
            i++;
            const b = tokens[i++].value;
            // A range lies on one tab: Sheet2!A1:B3, or Sheet2!A1:Sheet2!B3.
            if (b.sheet && !(token.value.sheet && sameSheet(b.sheet, token.value.sheet))) {
              return { t: "val", v: err("#REF!") };
            }
            return { t: "range", a: token.value, b };
          }
          return { t: "ref", ref: token.value };
        case "name": {
          if (isOp("(")) {
            i++;
            const args = [];
            if (!isOp(")")) {
              do {
                args.push(isOp(",") || isOp(")") ? { t: "val", v: null } : comparison());
              } while (isOp(",") && ++i);
            }
            expect(")");
            return { t: "fn", name: token.value, args };
          }
          if (token.value === "TRUE") return { t: "val", v: true };
          if (token.value === "FALSE") return { t: "val", v: false };
          return { t: "val", v: err("#NAME?") };
        }
        case "op":
          if (token.value === "(") {
            const x = comparison();
            expect(")");
            return x;
          }
      }
      throw new SyntaxError("Unexpected " + token.value);
    }
    const concat = binary(binary(binary(power, ["*", "/"]), ["+", "-"]), ["&"]);
    const comparison = binary(concat, ["=", "<>", "<", ">", "<=", ">="]);
    const ast = comparison();
    if (i < tokens.length) throw new SyntaxError("Unexpected " + tokens[i].value);
    return ast;
  }

  const parsed = new Map();
  function compile(formula) {
    let ast = parsed.get(formula);
    if (!ast) {
      try {
        ast = parse(formula.slice(1));
      } catch (_) {
        ast = { t: "val", v: err("#ERROR!") };
      }
      if (parsed.size > 20000) parsed.clear();
      parsed.set(formula, ast);
    }
    return ast;
  }

  // ------------------------------------------------------------------ coercion

  function toNumber(v) {
    if (isErr(v)) return v;
    if (v === null) return 0;
    if (typeof v === "number") return v;
    if (typeof v === "boolean") return v ? 1 : 0;
    const n = literal(v.trim());
    return typeof n === "number" ? n : err("#VALUE!");
  }
  function toText(v) {
    if (isErr(v)) return v;
    if (v === null) return "";
    if (typeof v === "number") return general(v);
    if (typeof v === "boolean") return v ? "TRUE" : "FALSE";
    return v;
  }
  function toBool(v) {
    if (isErr(v)) return v;
    if (v === null) return false;
    if (typeof v === "boolean") return v;
    if (typeof v === "number") return v !== 0;
    if (/^true$/i.test(v)) return true;
    if (/^false$/i.test(v)) return false;
    return err("#VALUE!");
  }
  function finite(n) {
    return typeof n === "number" && !Number.isFinite(n) ? err("#NUM!") : n;
  }
  // A range used where one value is expected: a single cell, else #VALUE!.
  function scalar(v) {
    if (!isRange(v)) return v;
    return v.values.length === 1 ? v.values[0] : err("#VALUE!");
  }

  // ------------------------------------------------------------------ evaluation

  function evaluate(node, ctx) {
    switch (node.t) {
      case "val": return node.v;
      case "ref": {
        const t = ctx.tab(node.ref);
        return t < 0 ? err("#REF!") : ctx.get(t, node.ref.col, node.ref.row);
      }
      case "range": {
        const t = ctx.tab(node.a);
        if (t < 0) return err("#REF!");
        const c1 = Math.min(node.a.col, node.b.col), c2 = Math.max(node.a.col, node.b.col);
        const r1 = Math.min(node.a.row, node.b.row), r2 = Math.max(node.a.row, node.b.row);
        const size = ctx.size(t);
        if (c2 >= size.cols || r2 >= size.rows) return err("#REF!");
        const values = [];
        for (let r = r1; r <= r2; r++) for (let c = c1; c <= c2; c++) values.push(ctx.get(t, c, r));
        return { values };
      }
      case "pct": {
        const n = toNumber(scalar(evaluate(node.x, ctx)));
        return isErr(n) ? n : n / 100;
      }
      case "un": {
        const n = toNumber(scalar(evaluate(node.x, ctx)));
        return isErr(n) ? n : node.op === "-" ? -n : n;
      }
      case "bin": return binaryOp(node.op, scalar(evaluate(node.a, ctx)), scalar(evaluate(node.b, ctx)));
      case "fn": {
        const fn = FUNCTIONS[node.name];
        if (!fn) return err("#NAME?");
        if (node.args.length < fn.min || node.args.length > fn.max) return err("#VALUE!");
        return fn.run(node.args, ctx);
      }
    }
    return err("#ERROR!");
  }

  function binaryOp(op, a, b) {
    if (op === "&") {
      const x = toText(a), y = toText(b);
      return isErr(x) ? x : isErr(y) ? y : x + y;
    }
    if (["=", "<>", "<", ">", "<=", ">="].includes(op)) {
      if (isErr(a)) return a;
      if (isErr(b)) return b;
      const c = compare(a, b);
      switch (op) {
        case "=": return c === 0;
        case "<>": return c !== 0;
        case "<": return c < 0;
        case ">": return c > 0;
        case "<=": return c <= 0;
        default: return c >= 0;
      }
    }
    const x = toNumber(a), y = toNumber(b);
    if (isErr(x)) return x;
    if (isErr(y)) return y;
    switch (op) {
      case "+": return x + y;
      case "-": return x - y;
      case "*": return x * y;
      case "/": return y === 0 ? err("#DIV/0!") : x / y;
      default: {
        const p = Math.pow(x, y);
        return Number.isFinite(p) ? p : err("#NUM!");
      }
    }
  }

  // Numbers sort before text, text before booleans; text ignores case. An
  // empty cell compares as the other side's empty value.
  function compare(a, b) {
    const rank = (v) => (typeof v === "number" ? 0 : typeof v === "string" ? 1 : 2);
    if (a === null) a = b === null ? 0 : typeof b === "number" ? 0 : typeof b === "string" ? "" : false;
    if (b === null) b = typeof a === "number" ? 0 : typeof a === "string" ? "" : false;
    if (rank(a) !== rank(b)) return rank(a) - rank(b);
    if (typeof a === "string") {
      const x = a.toLowerCase(), y = b.toLowerCase();
      return x < y ? -1 : x > y ? 1 : 0;
    }
    return a < b ? -1 : a > b ? 1 : 0;
  }

  // Numbers from arguments: ranges contribute only their numbers; values typed
  // straight into the call are converted, so SUM("2", TRUE) is 3.
  function numbers(args, ctx) {
    const out = [];
    for (const arg of args) {
      const v = evaluate(arg, ctx);
      if (isRange(v)) {
        for (const x of v.values) {
          if (isErr(x)) return x;
          if (typeof x === "number") out.push(x);
        }
      } else {
        if (v === null) continue;
        const n = toNumber(v);
        if (isErr(n)) return n;
        out.push(n);
      }
    }
    return out;
  }
  function values(args, ctx) {
    const out = [];
    for (const arg of args) {
      const v = evaluate(arg, ctx);
      if (isRange(v)) out.push(...v.values);
      else out.push(v);
    }
    return out;
  }
  const aggregate = (min, reduce) => ({
    min,
    max: 255,
    run(args, ctx) {
      const ns = numbers(args, ctx);
      return isErr(ns) ? ns : finite(reduce(ns));
    },
  });
  const unaryNumber = (fn) => ({
    min: 1,
    max: 1,
    run(args, ctx) {
      const n = toNumber(scalar(evaluate(args[0], ctx)));
      return isErr(n) ? n : finite(fn(n));
    },
  });
  const unaryText = (fn) => ({
    min: 1,
    max: 1,
    run(args, ctx) {
      const t = toText(scalar(evaluate(args[0], ctx)));
      return isErr(t) ? t : fn(t);
    },
  });
  function arg(args, index, ctx, fallback) {
    return index < args.length ? scalar(evaluate(args[index], ctx)) : fallback;
  }
  function roundTo(n, digits) {
    const factor = Math.pow(10, digits);
    const shifted = Math.abs(n) * factor;
    // Nudge by an ulp so 1.005 rounds to 1.01, as people expect.
    const rounded = Math.round(shifted + shifted * Number.EPSILON) / factor;
    return Math.sign(n) * rounded;
  }
  const logical = (combine) => ({
    min: 1,
    max: 255,
    run(args, ctx) {
      const bools = [];
      for (const v of values(args, ctx)) {
        if (v === null || (typeof v === "string" && !/^(true|false)$/i.test(v))) continue;
        const b = toBool(v);
        if (isErr(b)) return b;
        bools.push(b);
      }
      return bools.length ? combine(bools) : err("#VALUE!");
    },
  });
  const sideText = (fromLeft) => ({
    min: 1,
    max: 2,
    run(args, ctx) {
      const t = toText(arg(args, 0, ctx));
      const n = toNumber(arg(args, 1, ctx, 1));
      if (isErr(t)) return t;
      if (isErr(n)) return n;
      if (n < 0) return err("#VALUE!");
      const count = Math.floor(n);
      return fromLeft ? t.slice(0, count) : count ? t.slice(-count) : "";
    },
  });

  const FUNCTIONS = {
    SUM: aggregate(1, (ns) => ns.reduce((a, b) => a + b, 0)),
    PRODUCT: aggregate(1, (ns) => ns.reduce((a, b) => a * b, 1)),
    AVERAGE: { min: 1, max: 255, run(args, ctx) {
      const ns = numbers(args, ctx);
      if (isErr(ns)) return ns;
      return ns.length ? ns.reduce((a, b) => a + b, 0) / ns.length : err("#DIV/0!");
    } },
    MIN: aggregate(1, (ns) => (ns.length ? Math.min(...ns) : 0)),
    MAX: aggregate(1, (ns) => (ns.length ? Math.max(...ns) : 0)),
    MEDIAN: { min: 1, max: 255, run(args, ctx) {
      const ns = numbers(args, ctx);
      if (isErr(ns)) return ns;
      if (!ns.length) return err("#NUM!");
      ns.sort((a, b) => a - b);
      const mid = ns.length >> 1;
      return ns.length % 2 ? ns[mid] : (ns[mid - 1] + ns[mid]) / 2;
    } },
    COUNT: { min: 1, max: 255, run(args, ctx) {
      let count = 0;
      for (const arg of args) {
        const v = evaluate(arg, ctx);
        if (isRange(v)) count += v.values.filter((x) => typeof x === "number").length;
        else if (typeof toNumber(v) === "number" && v !== null) count++;
      }
      return count;
    } },
    COUNTA: { min: 1, max: 255, run(args, ctx) {
      return values(args, ctx).filter((v) => v !== null && v !== "").length;
    } },
    ROUND: { min: 1, max: 2, run(args, ctx) {
      const n = toNumber(arg(args, 0, ctx));
      const d = toNumber(arg(args, 1, ctx, 0));
      if (isErr(n)) return n;
      if (isErr(d)) return d;
      return roundTo(n, Math.trunc(d));
    } },
    ABS: unaryNumber(Math.abs),
    INT: unaryNumber(Math.floor),
    SQRT: unaryNumber((n) => (n < 0 ? NaN : Math.sqrt(n))),
    POWER: { min: 2, max: 2, run(args, ctx) {
      return binaryOp("^", arg(args, 0, ctx), arg(args, 1, ctx));
    } },
    MOD: { min: 2, max: 2, run(args, ctx) {
      const n = toNumber(arg(args, 0, ctx)), d = toNumber(arg(args, 1, ctx));
      if (isErr(n)) return n;
      if (isErr(d)) return d;
      return d === 0 ? err("#DIV/0!") : n - d * Math.floor(n / d);
    } },
    IF: { min: 2, max: 3, run(args, ctx) {
      const c = toBool(arg(args, 0, ctx));
      if (isErr(c)) return c;
      return c ? arg(args, 1, ctx) : arg(args, 2, ctx, false);
    } },
    IFERROR: { min: 2, max: 2, run(args, ctx) {
      const v = arg(args, 0, ctx);
      return isErr(v) ? arg(args, 1, ctx) : v;
    } },
    AND: logical((bs) => bs.every(Boolean)),
    OR: logical((bs) => bs.some(Boolean)),
    NOT: { min: 1, max: 1, run(args, ctx) {
      const b = toBool(arg(args, 0, ctx));
      return isErr(b) ? b : !b;
    } },
    CONCAT: { min: 1, max: 255, run(args, ctx) {
      let out = "";
      for (const v of values(args, ctx)) {
        const t = toText(v);
        if (isErr(t)) return t;
        out += t;
      }
      return out;
    } },
    LEN: unaryText((t) => [...t].length),
    UPPER: unaryText((t) => t.toUpperCase()),
    LOWER: unaryText((t) => t.toLowerCase()),
    TRIM: unaryText((t) => t.trim().replace(/ {2,}/g, " ")),
    LEFT: sideText(true),
    RIGHT: sideText(false),
  };
  FUNCTIONS.CONCATENATE = FUNCTIONS.CONCAT;

  // ------------------------------------------------------------------ sheets

  // Every filled cell's value in every tab. `tabs` is [{name, rows, cols,
  // cells}], with cells mapping names such as "B7" to {input}; the result has
  // one Map of values per tab.
  function computeBook(tabs) {
    const results = tabs.map(() => new Map());
    const pending = new Set();
    const tabIndex = (name) => tabs.findIndex((tab) => sameSheet(tab.name, name));
    const contexts = tabs.map((_, home) => ({
      tab: (ref) => (ref.sheet ? tabIndex(ref.sheet) : home),
      size: (t) => tabs[t],
      get,
    }));
    function get(t, col, row) {
      const tab = tabs[t];
      if (col >= tab.cols || row >= tab.rows) return err("#REF!");
      const key = keyOf(col, row);
      if (results[t].has(key)) return results[t].get(key);
      const cell = tab.cells.get(key);
      if (!cell || !cell.input) return null;
      const id = t + ":" + key;
      if (pending.has(id)) return err("#CYCLE!");
      pending.add(id);
      let value;
      if (isFormula(cell.input)) {
        value = scalar(evaluate(compile(cell.input), contexts[t]));
        if (value === null) value = 0;
        value = finite(value);
      } else {
        value = literal(cell.input);
      }
      pending.delete(id);
      results[t].set(key, value);
      return value;
    }
    tabs.forEach((tab, t) => {
      for (const key of tab.cells.keys()) {
        const p = parseKey(key);
        if (p) get(t, p.col, p.row);
      }
    });
    return results;
  }

  // One tab on its own.
  function compute(cells, size) {
    return computeBook([{ name: "Sheet1", rows: size.rows, cols: size.cols, cells }])[0];
  }

  // ------------------------------------------------------------------ moving references

  // Rewrites the references in a formula. `map(ref)` returns the new ref or
  // null for a deleted one; `mapRange(a, b)` the same for both ends of a range.
  function rewrite(input, map, mapRange) {
    if (!isFormula(input)) return input;
    let tokens;
    try {
      tokens = tokenize(input.slice(1));
    } catch (_) {
      return input;
    }
    const source = input.slice(1);
    let out = "";
    let last = 0;
    for (let i = 0; i < tokens.length; i++) {
      const token = tokens[i];
      if (token.type !== "ref") continue;
      const next = tokens[i + 1], end = tokens[i + 2];
      let text, stop;
      // Unchanged references keep the text as typed.
      if (next && next.type === "op" && next.value === ":" && end && end.type === "ref") {
        const range = mapRange(token.value, end.value);
        if (range && range[0] === token.value && range[1] === end.value) text = source.slice(token.start, end.end);
        else text = range ? refText(range[0]) + ":" + refText(range[1]) : "#REF!";
        stop = end.end;
        i += 2;
      } else {
        const ref = map(token.value);
        if (ref === token.value) text = source.slice(token.start, token.end);
        else text = ref ? refText(ref) : "#REF!";
        stop = token.end;
      }
      out += source.slice(last, token.start) + text;
      last = stop;
    }
    return "=" + out + source.slice(last);
  }

  const field = (axis) => (axis === "row" ? "row" : "col");

  // Whether a row or column change on tab `target.name` touches a reference in
  // a formula on tab `target.home`. Without a target, references naming no tab do.
  const touches = (target) => (ref) =>
    target ? (ref.sheet ? sameSheet(ref.sheet, target.name) : sameSheet(target.home, target.name)) : !ref.sheet;

  // References at or after `at` move by `count` when rows or columns are inserted.
  function insertShift(input, axis, at, count, target) {
    const f = field(axis);
    const applies = touches(target);
    const shift = (ref) => (ref[f] >= at ? Object.assign({}, ref, { [f]: ref[f] + count }) : ref);
    return rewrite(
      input,
      (ref) => (applies(ref) ? shift(ref) : ref),
      (a, b) => (applies(a) ? [shift(a), shift(b)] : [a, b]),
    );
  }

  // References into deleted rows or columns become #REF!; ranges shrink.
  function deleteShift(input, axis, at, count, target) {
    const f = field(axis);
    const applies = touches(target);
    const map = (ref) => {
      if (!applies(ref) || ref[f] < at) return ref;
      if (ref[f] < at + count) return null;
      return Object.assign({}, ref, { [f]: ref[f] - count });
    };
    const mapRange = (a, b) => {
      if (!applies(a)) return [a, b];
      const lo = Math.min(a[f], b[f]), hi = Math.max(a[f], b[f]);
      const newLo = lo < at ? lo : lo >= at + count ? lo - count : at;
      const newHi = hi < at ? hi : hi >= at + count ? hi - count : at - 1;
      if (newHi < newLo) return null;
      const first = a[f] <= b[f] ? a : b, second = a[f] <= b[f] ? b : a;
      return [Object.assign({}, first, { [f]: newLo }), Object.assign({}, second, { [f]: newHi })];
    };
    return rewrite(input, map, mapRange);
  }

  // Relative references follow a formula copied `dCol` columns and `dRow` rows away.
  function moveFormula(input, dCol, dRow, size) {
    const move = (ref) => {
      const col = ref.colAbs ? ref.col : ref.col + dCol;
      const row = ref.rowAbs ? ref.row : ref.row + dRow;
      if (col < 0 || row < 0 || col >= size.cols || row >= size.rows) return null;
      return Object.assign({}, ref, { col, row });
    };
    return rewrite(input, move, (a, b) => {
      const x = move(a), y = move(b);
      return x && y ? [x, y] : null;
    });
  }

  // References to tab `from` follow it when it is renamed `to`.
  function renameSheet(input, from, to) {
    const map = (ref) => (ref.sheet && sameSheet(ref.sheet, from) ? Object.assign({}, ref, { sheet: to }) : ref);
    return rewrite(input, map, (a, b) => [map(a), map(b)]);
  }

  // References to a deleted tab become #REF!.
  function dropSheet(input, name) {
    const gone = (ref) => !!ref.sheet && sameSheet(ref.sheet, name);
    return rewrite(input, (ref) => (gone(ref) ? null : ref), (a, b) => (gone(a) ? null : [a, b]));
  }

  // Sort order for values: numbers, text (ignoring case, numbers in text by
  // value), booleans, errors. Empty cells always come last, also when descending.
  function sortCompare(a, b, descending) {
    const empty = (v) => v === null || v === undefined || v === "";
    if (empty(a) || empty(b)) return empty(a) === empty(b) ? 0 : empty(a) ? 1 : -1;
    const rank = (v) => (typeof v === "number" ? 0 : typeof v === "string" ? 1 : typeof v === "boolean" ? 2 : 3);
    let order = rank(a) - rank(b);
    if (!order) {
      if (typeof a === "string") order = a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
      else if (isErr(a)) order = a.error < b.error ? -1 : a.error > b.error ? 1 : 0;
      else order = a < b ? -1 : a > b ? 1 : 0;
    }
    return descending ? -order : order;
  }

  // ------------------------------------------------------------------ display
  // Matches src/sheets/render.rs, which formats the same values on the server.

  function general(n) {
    if (n === 0) return "0";
    if (Number.isInteger(n) && Math.abs(n) < 1e15) return String(n);
    return String(Number(n.toPrecision(12)));
  }
  function grouped(n, decimals) {
    const fixed = Math.abs(n).toFixed(decimals);
    const [whole, fraction] = fixed.split(".");
    const text = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",") + (fraction ? "." + fraction : "");
    return n < 0 && /[1-9]/.test(text) ? "-" + text : text;
  }
  function formatNumber(n, fmt) {
    switch (fmt) {
      case "number": return grouped(n, 2);
      case "integer": return grouped(n, 0);
      case "percent": {
        let text = (n * 100).toFixed(2).replace(/\.?0+$/, "");
        if (text === "-0") text = "0";
        return text + "%";
      }
      case "currency": {
        const text = grouped(n, 2);
        return text[0] === "-" ? "-$" + text.slice(1) : "$" + text;
      }
      default: return general(n);
    }
  }
  function display(value, fmt) {
    if (value === null || value === undefined) return "";
    if (typeof value === "number") return formatNumber(value, fmt);
    if (typeof value === "boolean") return value ? "TRUE" : "FALSE";
    if (isErr(value)) return value.error;
    return value;
  }

  // ------------------------------------------------------------------ CSV / TSV

  function parseDelimited(text, delimiter) {
    const rows = [];
    let row = [], field = "", quoted = false, i = 0;
    text = text.replace(/^﻿/, "");
    while (i < text.length) {
      const ch = text[i];
      if (quoted) {
        if (ch === '"' && text[i + 1] === '"') {
          field += '"';
          i += 2;
          continue;
        }
        if (ch === '"') quoted = false;
        else field += ch;
        i++;
        continue;
      }
      if (ch === '"' && field === "") quoted = true;
      else if (ch === delimiter) {
        row.push(field);
        field = "";
      } else if (ch === "\n" || ch === "\r") {
        if (ch === "\r" && text[i + 1] === "\n") i++;
        row.push(field);
        rows.push(row);
        row = [];
        field = "";
      } else field += ch;
      i++;
    }
    if (field !== "" || row.length) {
      row.push(field);
      rows.push(row);
    }
    return rows;
  }
  function toDelimited(rows, delimiter) {
    return rows
      .map((row) =>
        row
          .map((v) => (/["\n\r]/.test(v) || v.includes(delimiter) ? '"' + v.replace(/"/g, '""') + '"' : v))
          .join(delimiter),
      )
      .join("\r\n");
  }

  const api = {
    ERRORS, colName, colIndex, keyOf, parseKey, literal, isFormula, compute, computeBook,
    insertShift, deleteShift, moveFormula, renameSheet, dropSheet, sortCompare,
    quoteSheet, pointText, display, general, isErr,
    parseDelimited, toDelimited,
  };
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.SheetFormula = api;
})(typeof window !== "undefined" ? window : globalThis);
