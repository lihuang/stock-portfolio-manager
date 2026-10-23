// @ts-nocheck -- Exercise clipboard serialization against real DOM selections.
import assert from "node:assert/strict";
import test from "node:test";
import { Window } from "happy-dom";
import { copyOptionTableSelection } from "./copyOptionTableSelection.ts";

function fixture(markup: string) {
  const window = new Window();
  const document = window.document;
  document.body.innerHTML = `<div id="records">${markup}</div>`;
  const root = document.getElementById("records");
  const selection = document.getSelection();
  const range = document.createRange();
  const data = new Map();
  let prevented = false;
  return {
    document, root, range,
    copy() {
      data.clear();
      prevented = false;
      selection.removeAllRanges();
      selection.addRange(range);
      copyOptionTableSelection({
        currentTarget: root,
        clipboardData: { setData: (type, value) => data.set(type, value) },
        preventDefault: () => { prevented = true; },
      }, root);
      return { data, prevented };
    },
  };
}

test("copies completed records as a standalone HTML table and Excel rows, including closed status and empty columns", () => {
  const f = fixture(`<table>
    <thead><tr><th class="ant-table-row-expand-icon-cell"></th><th>期权标识</th><th>佣金</th><th>结果</th></tr></thead>
    <tbody>
      <tr><td class="ant-table-row-expand-icon-cell">+</td><td>BABA &amp; MSFT</td><td>$4.90</td><td><button style="user-select:none">已平仓</button></td></tr>
      <tr><td class="ant-table-row-expand-icon-cell"></td><td>&lt;测试&gt;</td><td></td><td>已到期</td></tr>
    </tbody>
  </table>`);
  f.range.selectNodeContents(f.root.querySelector("table"));
  const result = f.copy();
  assert.equal(result.prevented, true);
  assert.equal(result.data.get("text/plain"), "期权标识\t佣金\t结果\r\nBABA & MSFT\t$4.90\t已平仓\r\n<测试>\t\t已到期");
  const pasted = f.document.createElement("div");
  pasted.innerHTML = result.data.get("text/html");
  assert.equal(pasted.firstElementChild.tagName, "TABLE");
  assert.deepEqual([...pasted.querySelectorAll("tr")].map(row => [...row.cells].map(cell => cell.textContent)), [
    ["期权标识", "佣金", "结果"], ["BABA & MSFT", "$4.90", "已平仓"], ["<测试>", "", "已到期"],
  ]);
  assert.equal(pasted.querySelector("button, svg, 测试"), null);
});

test("keeps native copying for a selection within one cell or outside the records", () => {
  const f = fixture("<table><tr><td>BABA</td><td>已平仓</td></tr></table>");
  f.range.setStart(f.root.querySelector("td").firstChild, 1);
  f.range.setEnd(f.root.querySelector("td").firstChild, 3);
  assert.equal(f.copy().prevented, false);
  const outside = f.document.createElement("p");
  outside.textContent = "表格外的文字";
  f.document.body.append(outside);
  f.range.selectNodeContents(outside);
  assert.equal(f.copy().prevented, false);
});

test("retains only selected text and column positions when a selection crosses row boundaries", () => {
  const f = fixture("<table><tr><td>ABC</td><td>DEF</td></tr><tr><td>GHI</td><td>JKL</td></tr></table>");
  const cells = f.root.querySelectorAll("td");
  f.range.setStart(cells[1].firstChild, 1);
  f.range.setEnd(cells[2].firstChild, 2);
  assert.equal(f.copy().data.get("text/plain"), "\tEF\r\nGH\t");
});

test("does not copy hidden closing details after a row is collapsed", () => {
  const f = fixture(`<table><tr><td>BABA</td><td>已平仓</td></tr>
    <tr style="display:none"><td colspan="2"><table><tr><td>隐藏平仓时间</td><td>$5.20</td></tr></table></td></tr>
    <tr><td>MSFT</td><td>已到期</td></tr></table>`);
  f.range.selectNodeContents(f.root.querySelector("table"));
  assert.equal(f.copy().data.get("text/plain"), "BABA\t已平仓\r\nMSFT\t已到期");
});

test("supports copying within a nested closing table without copying its parent row", () => {
  const f = fixture(`<table><tr><td>开仓记录</td><td>已平仓</td></tr><tr><td colspan="2">
    <table id="closes"><tr><th>平仓时间</th><th>平仓价</th></tr><tr><td>2024-12-04</td><td>$5.20</td></tr></table>
  </td></tr></table>`);
  f.range.selectNodeContents(f.document.getElementById("closes"));
  assert.equal(f.copy().data.get("text/plain"), "平仓时间\t平仓价\r\n2024-12-04\t$5.20");
  f.range.selectNodeContents(f.root.querySelector("table"));
  assert.equal(f.copy().data.get("text/plain"), "开仓记录\t已平仓\r\n平仓时间\t平仓价\r\n2024-12-04\t$5.20");
});

test("copies an expanded opening table selected through its containing row rather than its table node", () => {
  const f = fixture(`<table><tr><td>BABA 汇总</td><td>2</td></tr>
    <tr id="expanded"><td colspan="2"><div><table>
      <tr><th>期权标识</th><th>结果</th></tr>
      <tr><td>BABA Call</td><td>被执行</td></tr>
      <tr><td>BABA Put</td><td>已到期</td></tr>
    </table></div></td></tr></table>`);
  f.range.selectNodeContents(f.document.getElementById("expanded"));
  const result = f.copy();
  assert.equal(result.prevented, true);
  assert.equal(result.data.get("text/plain"), "期权标识\t结果\r\nBABA Call\t被执行\r\nBABA Put\t已到期");
});

test("copies the same table when selection encloses its wrappers or starts before the records", () => {
  const f = fixture("<div class='ant-table-wrapper'><table><tr><th>期权标识</th><th>结果</th></tr><tr><td>BABA</td><td>已到期</td></tr></table></div>");
  for (const node of [f.root, f.root.querySelector(".ant-table-wrapper"), f.root.querySelector("table")]) {
    f.range.selectNode(node);
    assert.equal(f.copy().data.get("text/plain"), "期权标识\t结果\r\nBABA\t已到期");
    f.range.selectNodeContents(node);
    assert.equal(f.copy().data.get("text/plain"), "期权标识\t结果\r\nBABA\t已到期");
  }
});

test("keeps visible closing summaries in order without duplicating expanded container text", () => {
  const f = fixture(`<table><tr><td>BABA</td><td>已平仓</td></tr><tr><td colspan="2">
    <table><tr><th>平仓价</th><th>费用</th></tr><tr><td>$5.20</td><td>$4.44</td></tr></table>
    <div data-option-copy-row><span data-option-copy-cell>开仓收入：$11,300</span><span data-option-copy-cell>净盈亏：+$6,090.66</span></div>
  </td></tr></table>`);
  f.range.selectNodeContents(f.root);
  assert.equal(f.copy().data.get("text/plain"), "BABA\t已平仓\r\n平仓价\t费用\r\n$5.20\t$4.44\r\n开仓收入：$11,300\t净盈亏：+$6,090.66");
});

test("normalizes line breaks and tabs inside cells without inventing Excel rows or columns", () => {
  const f = fixture("<table><tr><td>BABA\n  Call</td><td>已\t平仓</td></tr></table>");
  f.range.selectNodeContents(f.root.querySelector("table"));
  assert.equal(f.copy().data.get("text/plain"), "BABA Call\t已 平仓");
});
