/// Mainland stock/fund/bond codes use lowercase exchange prefixes in storage.
/// Cash sentinels, other markets, and unrecognized symbols keep their spelling.
pub(crate) fn normalize_cn_stock_symbol(symbol: &str, market: &str) -> String {
    if market.eq_ignore_ascii_case("CN") && is_cn_stock_symbol(symbol) {
        symbol.trim().to_ascii_lowercase()
    } else {
        symbol.to_string()
    }
}

pub(crate) fn is_cn_stock_symbol(symbol: &str) -> bool {
    let bytes = symbol.trim().as_bytes();
    bytes.len() == 8
        && [b"sh", b"sz", b"bj"]
            .iter()
            .any(|prefix| bytes[..2].eq_ignore_ascii_case(*prefix))
        && bytes[2..].iter().all(u8::is_ascii_digit)
}
