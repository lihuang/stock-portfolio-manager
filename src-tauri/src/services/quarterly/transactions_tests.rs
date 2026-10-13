use super::*;

fn fixture() -> Database {
    let db = Database::new(":memory:").unwrap();
    db.conn
        .lock()
        .unwrap()
        .execute_batch(
            "INSERT INTO accounts (id, name, market, created_at, updated_at) VALUES
             ('a', 'Account A', 'CN', '2026-01-01', '2026-01-01'),
             ('b', 'Account B', 'CN', '2026-01-01', '2026-01-01');
             INSERT INTO quarterly_snapshots (id, quarter, snapshot_date, created_at)
             VALUES ('q3', '2026-Q3', '2026-09-30', '2026-10-01');",
        )
        .unwrap();
    db
}

#[allow(clippy::too_many_arguments)]
fn transaction(
    db: &Database,
    id: &str,
    account: &str,
    symbol: &str,
    market: &str,
    kind: &str,
    shares: f64,
    amount: f64,
    date: &str,
) {
    let currency = match market {
        "CN" => "CNY",
        "HK" => "HKD",
        _ => "USD",
    };
    db.conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO transactions
         (id, account_id, symbol, name, market, transaction_type, shares, price,
          total_amount, commission, currency, traded_at, created_at)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, 1, ?7, 0, ?8, ?9, ?9)",
            rusqlite::params![id, account, symbol, market, kind, shares, amount, currency, date],
        )
        .unwrap();
}

#[test]
fn quarterly_transactions_merge_symbol_case_across_accounts_without_losing_trades() {
    let db = fixture();
    for (symbol, kind, shares, amount, expected_shares, expected_amount) in [
        (
            "SH510900",
            "SELL",
            [770_000.0, 100_000.0],
            [812_350.0, 101_900.0],
            870_000.0,
            914_250.0,
        ),
        (
            "SH600036",
            "BUY",
            [47_500.0, 25_200.0],
            [1_871_196.0, 1_038_226.0],
            72_700.0,
            2_909_422.0,
        ),
        (
            "SZ000001",
            "SELL",
            [56_000.0, 74_000.0],
            [650_090.0, 873_620.0],
            130_000.0,
            1_523_710.0,
        ),
    ] {
        let lower = symbol.to_ascii_lowercase();
        transaction(
            &db,
            &lower,
            "a",
            &lower,
            "CN",
            kind,
            shares[0],
            amount[0],
            "2026-08-21",
        );
        transaction(
            &db,
            symbol,
            "b",
            symbol,
            "CN",
            kind,
            shares[1],
            amount[1],
            "2026-09-15",
        );

        let groups = get_quarterly_transactions(&db, "q3").unwrap();
        let matching: Vec<_> = groups
            .iter()
            .filter(|group| group.symbol.eq_ignore_ascii_case(symbol))
            .collect();
        assert_eq!(matching.len(), 1, "split group for {symbol}");
        let group = matching[0];
        assert_eq!(group.symbol, lower);
        assert_eq!(group.market, "CN");
        assert_eq!(group.currency, "CNY");
        assert_eq!(group.transactions.len(), 2);
        assert_eq!(group.transactions[0].id, lower);
        assert_eq!(group.transactions[0].symbol, lower);
        assert_eq!(group.transactions[0].account_id, "a");
        assert_eq!(group.transactions[1].id, symbol);
        assert_eq!(group.transactions[1].account_id, "b");
        if kind == "BUY" {
            assert_eq!((group.buy_count, group.sell_count), (2, 0));
            assert_eq!(
                (group.total_buy_shares, group.total_sell_shares),
                (expected_shares, 0.0)
            );
            assert_eq!(
                (group.total_buy_amount, group.total_sell_amount),
                (expected_amount, 0.0)
            );
        } else {
            assert_eq!((group.buy_count, group.sell_count), (0, 2));
            assert_eq!(
                (group.total_buy_shares, group.total_sell_shares),
                (0.0, expected_shares)
            );
            assert_eq!(
                (group.total_buy_amount, group.total_sell_amount),
                (0.0, expected_amount)
            );
        }
    }
    assert_eq!(get_quarterly_transactions(&db, "q3").unwrap().len(), 3);
    let stored: String = db
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT symbol FROM transactions WHERE id = 'sh510900'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, "sh510900");
}

#[test]
fn quarterly_transactions_keep_markets_separate_and_sort_normalized_symbols() {
    let db = fixture();
    for (id, symbol, market) in [
        ("cn-bank", "SH600036", "CN"),
        ("cn-etf", "sh510900 ", "CN"),
        ("hk", "aapl", "HK"),
        ("us", "aapl", "US"),
    ] {
        transaction(
            &db,
            id,
            "a",
            symbol,
            market,
            "BUY",
            100.0,
            200.0,
            "2026-08-01",
        );
    }

    let groups = get_quarterly_transactions(&db, "q3").unwrap();
    let keys: Vec<_> = groups
        .iter()
        .map(|group| (group.market.as_str(), group.symbol.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            ("CN", "sh510900"),
            ("CN", "sh600036"),
            ("HK", "AAPL"),
            ("US", "AAPL")
        ]
    );
    assert!(groups
        .iter()
        .all(|group| group.buy_count == 1 && group.transactions.len() == 1));
}

#[test]
fn quarterly_transactions_preserve_filters_and_non_trade_details_after_merging() {
    let db = fixture();
    for (id, symbol, kind, date) in [
        (
            "previous-quarter",
            "sh600036",
            "BUY",
            "2026-06-30T23:59:59Z",
        ),
        ("quarter-start", "sh600036", "BUY", "2026-07-01T00:00:00Z"),
        ("open", "SH600036", "OPEN", "2026-07-02"),
        ("backfill", "SH600036", "BUY", "2026-07-03"),
        ("cash", "$CASH-CNY", "BUY", "2026-07-04"),
        ("dividend", "SH600036", "PAY", "2026-08-01"),
        ("transfer", "sh600036", "STOCK_IN", "2026-08-02"),
        ("quarter-end", "SH600036", "SELL", "2026-09-30T23:59:59Z"),
        ("next-quarter", "sh600036", "SELL", "2026-10-01T00:00:00Z"),
    ] {
        transaction(&db, id, "a", symbol, "CN", kind, 100.0, 200.0, date);
    }
    db.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE transactions SET notes = 'backfill:initial' WHERE id = 'backfill'",
            [],
        )
        .unwrap();

    let groups = get_quarterly_transactions(&db, "q3").unwrap();
    assert_eq!(groups.len(), 1);
    let group = &groups[0];
    assert_eq!((group.buy_count, group.sell_count), (1, 1));
    assert_eq!(
        (group.total_buy_shares, group.total_sell_shares),
        (100.0, 100.0)
    );
    assert_eq!(
        (group.total_buy_amount, group.total_sell_amount),
        (200.0, 200.0)
    );
    let ids: Vec<_> = group
        .transactions
        .iter()
        .map(|txn| txn.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["quarter-start", "dividend", "transfer", "quarter-end"]
    );
}
