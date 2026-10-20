// @ts-nocheck -- Node runs a Bun probe against the real React page and Ant Design tables.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const projectRoot = fileURLToPath(new URL("../../../", import.meta.url));

function runScenario(grouped = false) {
  const probe = String.raw`
    import { Window } from "happy-dom";
    import { mock } from "bun:test";
    const dom = new Window({ url: "http://localhost" });
    for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLDocument", "Element", "Node", "SVGElement", "ShadowRoot", "DocumentFragment", "MutationObserver", "ResizeObserver", "localStorage"]) {
      Object.defineProperty(globalThis, key, { configurable: true, value: key === "window" ? dom : dom[key] });
    }
    globalThis.getComputedStyle = dom.getComputedStyle.bind(dom);
    globalThis.requestAnimationFrame = dom.requestAnimationFrame.bind(dom);
    globalThis.cancelAnimationFrame = dom.cancelAnimationFrame.bind(dom);
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import("react");
    const { act } = React;
    const { createRoot } = await import("react-dom/client");
    const h = React.createElement;
    const noop = () => {};
    const accounts = [{ id: "a", name: "测试账户", market: "US" }];
    mock.module("./src/stores/accountStore", () => ({ useAccountStore: () => ({ accounts, fetchAccounts: noop }) }));
    const report = { account_id: "a", currency: "USD", period_days: null, underlyings: [] };
    mock.module("./src/stores/optionReviewStore", () => ({ useOptionReviewStore: () => ({
      report, loading: false, error: null, requestedAccountId: "a", requestedPeriodDays: null,
      fetchOptionReview: noop, clearOptionReview: noop,
    }) }));
    mock.module("./src/components/charts/StatCard", () => ({ default: () => null }));
    const contract = {
      id: "baba-open", account_id: "a", option_symbol: "BABA 15JAN27 160 C", underlying: "BABA",
      expiry_date: "15JAN27", strike_price: 160, option_type: "C", contracts: -10, remaining_contracts: 0,
      open_price: 11.3, open_amount: 11300, commission: -4.9, traded_at: "2024-10-01, 10:01:41",
      close_price: 5.2, close_code: "C", status: "closed",
      close_details: { close_cost: 5200, fees: 9.34, realized_pnl: 6090.66, records: [
        { id: "baba-close", code: "C", quantity: 10, price: 5.2, amount: 5200, commission: 4.44, fee: 0, traded_at: "2024-12-04, 09:57:34" },
      ] },
    };
    const contracts = [contract, { ...contract, id: "expired", underlying: "MSFT", option_symbol: "MSFT 15JAN27 160 C", status: "expired", close_price: 0, close_code: "C;Ep", close_details: null }];
    if (${JSON.stringify(grouped)}) contracts.push({ ...contract, id: "baba-open-2", open_price: 12, open_amount: 12000,
      close_details: { ...contract.close_details, realized_pnl: 6790.66 } });
    dom.__TAURI_INTERNALS__ = { invoke: async command => {
      if (command === "get_option_contracts") return contracts;
      throw new Error(command);
    } };
    localStorage.setItem("options_selected_account_id", "a");
    const { default: OptionsPage } = await import("./src/pages/Options/index.tsx");
    const container = document.createElement("div"); document.body.append(container);
    const root = createRoot(container);
    await act(async () => root.render(h(OptionsPage)));
    const tab = [...container.querySelectorAll('[role="tab"]')].find(node => node.textContent.includes("已到期"));
    await act(async () => tab.click());
    const buttons = () => [...container.querySelectorAll("button")].filter(node => node.textContent.includes("已平仓"));
    let target = buttons()[0];
    const before = { buttonCount: buttons().length, expanded: target?.getAttribute("aria-expanded") ?? null };
    let after = null, collapsed = null, returned = null;
    if (target) {
      await act(async () => target.click());
      if (${JSON.stringify(grouped)}) {
        target = buttons().find(node => node.closest("table") !== target.closest("table"));
        if (target) await act(async () => target.click());
      }
      if (target) {
        const row = target.closest("tr");
        const detailRow = row.nextElementSibling;
        after = { expanded: target.getAttribute("aria-expanded"), text: detailRow?.textContent ?? "", below: detailRow?.querySelector("table") != null };
        await act(async () => target.click());
        collapsed = { expanded: target.getAttribute("aria-expanded"), visible: detailRow?.style.display !== "none" };
        const summary = [...container.querySelectorAll(".ant-card")].find(node => node.querySelector(".ant-card-head-title")?.textContent === "个股期权汇总" && node.closest('[role="tabpanel"]')?.getAttribute("aria-hidden") !== "true");
        const summaryRow = symbol => [...summary.querySelectorAll("tbody tr")].find(node => node.querySelector("td")?.textContent.includes(symbol));
        await act(async () => target.click());
        await act(async () => summaryRow("MSFT").click());
        await act(async () => summaryRow("BABA").click());
        returned = buttons()[0]?.getAttribute("aria-expanded");
      }
    }
    const result = { before, after, collapsed, returned };
    await act(async () => root.unmount());
    await dom.happyDOM.close();
    process.stdout.write(JSON.stringify(result));
  `;
  return JSON.parse(execFileSync("bun", ["--eval", probe], {
    cwd: projectRoot, encoding: "utf8", timeout: 30000,
  }));
}

test("clicking a closed status reveals its closing trades and net P&L below the opening row, then collapses", () => {
  const result = runScenario();
  assert.equal(result.before.buttonCount, 1);
  assert.equal(result.before.expanded, "false");
  assert.equal(result.after.expanded, "true");
  assert.equal(result.after.below, true);
  assert.match(result.after.text, /2024-12-04/);
  assert.match(result.after.text, /\$5\.20/);
  assert.match(result.after.text, /\$5,200\.00/);
  assert.match(result.after.text, /\$9\.34/);
  assert.match(result.after.text, /\+\$6,090\.66/);
  assert.equal(result.collapsed.expanded, "false");
  assert.equal(result.collapsed.visible, false);
  assert.equal(result.returned, "false");
});

test("grouped completed contracts still allow each opening record to reveal its own closing details", () => {
  const result = runScenario(true);
  assert.equal(result.after?.expanded, "true");
  assert.match(result.after.text, /\+\$6,090\.66/);
  assert.equal(result.collapsed.expanded, "false");
});
