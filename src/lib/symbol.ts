/** Canonicalize only explicit six-digit mainland exchange codes. */
export function normalizeMainlandSymbolCase(symbol: string): string {
  const trimmed = symbol.trim();
  return /^(?:sh|sz|bj)\d{6}$/i.test(trimmed) ? trimmed.toLowerCase() : symbol;
}
