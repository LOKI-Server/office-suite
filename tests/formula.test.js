// Tests for the Sheets formula engine. Run with: node --test tests/
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const F = require("../static/formula.js");

// Evaluates a sheet given as {A1: "input", ...} and returns {key: value}.
function sheet(inputs, size = { rows: 100, cols: 26 }) {
  const cells = new Map(Object.entries(inputs).map(([k, input]) => [k, { input }]));
  return Object.fromEntries(F.compute(cells, size));
}
const value = (formula, inputs = {}) => sheet({ ...inputs, Z99: formula }).Z99;

test("literals", () => {
  assert.equal(F.literal("42"), 42);
  assert.equal(F.literal(" -1.5e3 "), -1500);
  assert.equal(F.literal("12.5%"), 0.125);
  assert.equal(F.literal("true"), true);
  assert.equal(F.literal("'007"), "007");
  assert.equal(F.literal("hello"), "hello");
  assert.equal(F.literal(""), null);
});

test("arithmetic and precedence", () => {
  assert.equal(value("=1+2*3"), 7);
  assert.equal(value("=(1+2)*3"), 9);
  assert.equal(value("=-2^2"), 4);
  assert.equal(value("=2^3^2"), 64);
  assert.equal(value("=50%*4"), 2);
  assert.equal(value('="a"&1+1'), "a2");
  assert.equal(value("=1+2=3"), true);
  assert.equal(value("=0.1+0.2"), 0.1 + 0.2);
  assert.deepEqual(value("=1/0"), { error: "#DIV/0!" });
  assert.deepEqual(value('="x"+1'), { error: "#VALUE!" });
  assert.deepEqual(value("=(-1)^0.5"), { error: "#NUM!" });
});

test("references, ranges and empty cells", () => {
  const out = sheet({ A1: "10", A2: "20", A3: "text", B1: "=A1+A2", B2: "=SUM(A1:A3)", B3: "=C9", B4: "=C9&\"!\"" });
  assert.equal(out.B1, 30);
  assert.equal(out.B2, 30);
  assert.equal(out.B3, 0);
  assert.equal(out.B4, "!");
  assert.deepEqual(value("=A1:A2"), { error: "#VALUE!" });
  assert.deepEqual(value("=A1+AA1"), { error: "#REF!" });
});

test("functions", () => {
  const nums = { A1: "1", A2: "2", A3: "3", A4: "x", A5: "" };
  assert.equal(value("=AVERAGE(A1:A5)", nums), 2);
  assert.equal(value("=MIN(A1:A3)+MAX(A1:A3)", nums), 4);
  assert.equal(value("=COUNT(A1:A5)", nums), 3);
  assert.equal(value("=COUNTA(A1:A5)", nums), 4);
  assert.equal(value("=MEDIAN(A1:A3,10)", nums), 2.5);
  assert.equal(value("=PRODUCT(A1:A3)", nums), 6);
  assert.equal(value('=SUM("2",TRUE)'), 3);
  assert.equal(value("=ROUND(1.005,2)"), 1.01);
  assert.equal(value("=ROUND(-2.5)"), -3);
  assert.equal(value("=MOD(-3,2)"), 1);
  assert.equal(value('=IF(1>2,"yes","no")'), "no");
  assert.equal(value("=IF(TRUE,1)"), 1);
  assert.equal(value("=IF(FALSE,1/0,5)"), 5);
  assert.equal(value("=IFERROR(1/0,\"none\")"), "none");
  assert.equal(value("=AND(TRUE,1)"), true);
  assert.equal(value("=OR(FALSE,0)"), false);
  assert.equal(value('=CONCAT("a",1,TRUE)'), "a1TRUE");
  assert.equal(value('=LEFT("Hello",2)&RIGHT("Hello")'), "Heo");
  assert.equal(value('=TRIM("  a   b ")'), "a b");
  assert.equal(value('=LEN("héllo")'), 5);
  assert.deepEqual(value("=NOPE(1)"), { error: "#NAME?" });
  assert.deepEqual(value("=foo+1"), { error: "#NAME?" });
  assert.deepEqual(value("=SQRT(-1)"), { error: "#NUM!" });
  assert.deepEqual(value("=AVERAGE(B1:B3)"), { error: "#DIV/0!" });
  assert.deepEqual(value("=ABS()"), { error: "#VALUE!" });
});

test("parse errors and cycles", () => {
  assert.deepEqual(value("=1+"), { error: "#ERROR!" });
  assert.deepEqual(value("=(1"), { error: "#ERROR!" });
  assert.deepEqual(value('="open'), { error: "#ERROR!" });
  const out = sheet({ A1: "=B1+1", B1: "=A1+1", C1: "=C1", D1: "=A1" });
  assert.deepEqual(out.A1, { error: "#CYCLE!" });
  assert.deepEqual(out.C1, { error: "#CYCLE!" });
  assert.deepEqual(out.D1, { error: "#CYCLE!" });
});

test("inserting rows and columns moves references", () => {
  assert.equal(F.insertShift("=A1+A3*$B$5", "row", 1, 2), "=A1+A5*$B$7");
  assert.equal(F.insertShift("=SUM(A1:A10)", "row", 4, 1), "=SUM(A1:A11)");
  assert.equal(F.insertShift("=B2&C2", "col", 2, 1), "=B2&D2");
  assert.equal(F.insertShift("plain", "row", 0, 1), "plain");
  assert.equal(F.insertShift('="A1"&A1', "row", 0, 1), '="A1"&A2');
});

test("deleting rows and columns", () => {
  assert.equal(F.deleteShift("=A1+A2+A5", "row", 1, 2), "=A1+#REF!+A3");
  assert.equal(F.deleteShift("=SUM(A1:A10)", "row", 2, 3), "=SUM(A1:A7)");
  assert.equal(F.deleteShift("=SUM(A3:A4)", "row", 2, 2), "=SUM(#REF!)");
  assert.equal(F.deleteShift("=SUM(B1:D1)", "col", 0, 2), "=SUM(A1:B1)");
  assert.deepEqual(value("=#REF!+1"), { error: "#REF!" });
});

test("copying formulas moves relative references", () => {
  const size = { rows: 100, cols: 26 };
  assert.equal(F.moveFormula("=A1+$A$1+A$1+$A1", 1, 2, size), "=B3+$A$1+B$1+$A3");
  assert.equal(F.moveFormula("=SUM(A1:B2)", 0, 1, size), "=SUM(A2:B3)");
  assert.equal(F.moveFormula("=A1", -1, 0, size), "=#REF!");
});

test("display formats match the server", () => {
  assert.equal(F.display(0.1 + 0.2), "0.3");
  assert.equal(F.display(1 / 3), "0.333333333333");
  assert.equal(F.display(1e21), "1e+21");
  assert.equal(F.display(2e-7), "2e-7");
  assert.equal(F.display(1234.5, "number"), "1,234.50");
  assert.equal(F.display(-1234567, "integer"), "-1,234,567");
  assert.equal(F.display(0.125, "percent"), "12.5%");
  assert.equal(F.display(0.5, "percent"), "50%");
  assert.equal(F.display(-1234.5, "currency"), "-$1,234.50");
  assert.equal(F.display(-0.001, "number"), "0.00");
  assert.equal(F.display(true), "TRUE");
  assert.equal(F.display({ error: "#N/A" }), "#N/A");
});

test("CSV and TSV", () => {
  assert.deepEqual(F.parseDelimited('a,"b,c","say ""hi"""\r\n1,,"x\ny"\n', ","), [
    ["a", "b,c", 'say "hi"'],
    ["1", "", "x\ny"],
  ]);
  assert.deepEqual(F.parseDelimited("a\tb\n", "\t"), [["a", "b"]]);
  assert.equal(F.toDelimited([["a", "b\tc"], ["1", 'q"']], "\t"), 'a\t"b\tc"\r\n1\t"q"""');
});

test("names", () => {
  assert.equal(F.colName(0), "A");
  assert.equal(F.colName(27), "AB");
  assert.deepEqual(F.parseKey("AZ12"), { col: 51, row: 11 });
  assert.equal(F.parseKey("A0"), null);
});

// Evaluates a workbook given as {tabName: {A1: "input"}}.
function book(tabs) {
  const list = Object.entries(tabs).map(([name, inputs]) => ({
    name, rows: 100, cols: 26, cells: new Map(Object.entries(inputs).map(([k, input]) => [k, { input }])),
  }));
  return F.computeBook(list).map((values) => Object.fromEntries(values));
}

test("references to other tabs", () => {
  const [main, other] = book({
    Main: { A1: "=Data!A1*2", A2: "=SUM('Q1 data'!A1:A2)", A3: "=Missing!A1", A4: "=data!A1", A5: "=Data!A1:B1" },
    Data: { A1: "21", B1: "=Main!A1+1" },
    "Q1 data": { A1: "1", A2: "2" },
  });
  assert.equal(main.A1, 42);
  assert.equal(main.A2, 3);
  assert.deepEqual(main.A3, { error: "#REF!" });
  assert.equal(main.A4, 21);
  assert.deepEqual(main.A5, { error: "#VALUE!" });
  assert.equal(other.B1, 43);
  const [cycle] = book({ A: { A1: "=B!A1" }, B: { A1: "=A!A1" } });
  assert.deepEqual(cycle.A1, { error: "#CYCLE!" });
  const [mixed] = book({ A: { A1: "=SUM(B!A1:C!A2)" }, B: {}, C: {} });
  assert.deepEqual(mixed.A1, { error: "#REF!" });
});

test("row changes on one tab only move references to it", () => {
  const onData = { home: "Main", name: "Data" };
  assert.equal(F.insertShift("=A5+Data!A5+Other!A5", "row", 1, 2, onData), "=A5+Data!A7+Other!A5");
  const onMain = { home: "Main", name: "Main" };
  assert.equal(F.insertShift("=A5+Data!A5", "row", 1, 2, onMain), "=A7+Data!A5");
  assert.equal(F.deleteShift("=SUM(Data!A1:A9)+A3", "row", 0, 2, onData), "=SUM(Data!A1:A7)+A3");
  assert.equal(F.deleteShift("='Q1 data'!B2", "row", 1, 1, { home: "Main", name: "q1 DATA" }), "=#REF!");
});

test("renaming and deleting tabs", () => {
  assert.equal(F.renameSheet("=Data!A1+data!B2:C3+A1", "Data", "Q1 data"), "='Q1 data'!A1+'Q1 data'!B2:C3+A1");
  assert.equal(F.renameSheet("='Q1 data'!A1", "Q1 data", "Totals"), "=Totals!A1");
  assert.equal(F.renameSheet("=x!A1", "x", "Q2"), "='Q2'!A1");
  assert.equal(F.dropSheet("=Data!A1+SUM(Data!A1:A2)+B1", "data"), "=#REF!+SUM(#REF!)+B1");
});

test("unchanged references keep their text", () => {
  assert.equal(F.insertShift("=a1+sum(b1:b2)", "row", 50, 1), "=a1+sum(b1:b2)");
});

test("pointing at cells", () => {
  assert.equal(F.pointText("Main", "Main", 1, 2), "B3");
  assert.equal(F.pointText("Q1 data", "Main", 0, 0), "'Q1 data'!A1");
  assert.equal(F.pointText("Data", "Main", 0, 0), "Data!A1");
  assert.equal(F.quoteSheet("A1"), "'A1'");
});

test("sort order", () => {
  const values = ["b", 10, null, true, "A", 2, { error: "#N/A" }, "item 10", "item 9", ""];
  const up = [...values].sort((a, b) => F.sortCompare(a, b, false));
  assert.deepEqual(up.slice(0, 7), [2, 10, "A", "b", "item 9", "item 10", true]);
  const down = [...values].sort((a, b) => F.sortCompare(a, b, true));
  assert.deepEqual(down.slice(0, 3), [{ error: "#N/A" }, true, "item 10"]);
  assert.ok([null, ""].includes(down[8]) && [null, ""].includes(down[9]));
});
