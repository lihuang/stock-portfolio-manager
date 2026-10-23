function isVisible(element: HTMLElement, root: HTMLElement): boolean {
  for (let current: HTMLElement | null = element; current; current = current.parentElement) {
    const style = current.ownerDocument.defaultView?.getComputedStyle(current);
    if (current.hidden || current.getAttribute("aria-hidden") === "true"
      || style?.display === "none" || style?.visibility === "hidden") return false;
    if (current === root) break;
  }
  return true;
}

/** Supply complete table markup and TSV instead of relying on WebView copy formatting. */
export function copyOptionTableSelection(event: ClipboardEvent, root: HTMLElement): void {
  if (event.defaultPrevented || !event.clipboardData) return;
  const target = event.target as Element | null;
  if (target?.closest?.("input, textarea, [contenteditable]:not([contenteditable='false'])")) return;
  const document = root.ownerDocument;
  const selection = document.getSelection();
  if (!selection || selection.rangeCount !== 1 || selection.isCollapsed) return;

  const range = selection.getRangeAt(0);
  if (!range.intersectsNode(root)) return;
  const ancestor = range.commonAncestorContainer;
  const element = ancestor.nodeType === 1 ? ancestor as Element : ancestor.parentElement;
  // Do not replace a broader selection that also contains unrelated tables.
  if (!root.contains(ancestor) && Array.from(element?.querySelectorAll("table") ?? []).some(table =>
    !root.contains(table) && range.intersectsNode(table),
  )) return;

  let selectedCount = 0;
  type Cell = { text: string; header: boolean };
  type Bounds = { first: number; last: number };
  const boundsByTable = new Map<Element, Bounds>();
  const rows: { cells: (Cell | null)[]; bounds: Bounds }[] = [];
  // DOM order keeps each opening followed by its visible opening/closing details.
  for (const row of root.querySelectorAll<HTMLElement>("tr, [data-option-copy-row]")) {
    if (!isVisible(row, root) || row.classList.contains("ant-table-measure-row")) continue;
    const isTableRow = row.tagName === "TR";
    const owner = isTableRow ? row.closest("table")! : row;
    let bounds = boundsByTable.get(owner);
    if (!bounds) {
      bounds = { first: Infinity, last: -1 };
      boundsByTable.set(owner, bounds);
    }
    const cells: HTMLElement[] = isTableRow
      ? Array.from(row.children).filter((cell): cell is HTMLTableCellElement =>
        cell.tagName === "TD" || cell.tagName === "TH",
      ).filter(cell =>
        !cell.classList.contains("ant-table-row-expand-icon-cell")
        // Expanded container cells must not concatenate their entire nested table.
        && !cell.querySelector("table, [data-option-copy-row]"),
      )
      : Array.from(row.querySelectorAll<HTMLElement>("[data-option-copy-cell]"));
    const selected = cells.map((cell, column) => {
      if (!isVisible(cell, root) || !range.intersectsNode(cell)) return null;
      const cellRange = document.createRange();
      cellRange.selectNodeContents(cell);
      // Clip to the selected portion of the first and last cells.
      if (range.compareBoundaryPoints(0, cellRange) > 0) {
        cellRange.setStart(range.startContainer, range.startOffset);
      }
      if (range.compareBoundaryPoints(2, cellRange) < 0) {
        cellRange.setEnd(range.endContainer, range.endOffset);
      }
      selectedCount += 1;
      bounds.first = Math.min(bounds.first, column);
      bounds.last = Math.max(bounds.last, column);
      return { text: cellRange.toString().replace(/\s+/g, " ").trim(), header: cell.tagName === "TH" };
    });
    if (selected.some(cell => cell !== null)) {
      rows.push({ cells: selected, bounds });
    }
  }
  // Preserve ordinary substring copying within a single cell.
  if (selectedCount < 2) return;

  const clipboardTable = document.createElement("table");
  const textRows: string[] = [];
  for (const row of rows) {
    const clipboardRow = clipboardTable.insertRow();
    const values: string[] = [];
    for (let column = row.bounds.first; column <= row.bounds.last; column += 1) {
      const value = row.cells[column] ?? { text: "", header: false };
      const cell = document.createElement(value.header ? "th" : "td");
      cell.textContent = value.text;
      clipboardRow.append(cell);
      values.push(value.text);
    }
    textRows.push(values.join("\t"));
  }
  event.clipboardData.setData("text/html", clipboardTable.outerHTML);
  event.clipboardData.setData("text/plain", textRows.join("\r\n"));
  event.preventDefault();
}
