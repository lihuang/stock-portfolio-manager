use super::{migrations::run_migrations, Database};

#[test]
fn cn_v11_migration_keeps_latest_cache_rows_when_case_variants_collide() {
    let db = Database::new(":memory:").unwrap();
    let mut conn = db.conn.lock().unwrap();
    conn.execute_batch(
        "INSERT INTO cached_quotes(symbol,name,market,current_price,updated_at) VALUES
         ('SH600036','new quote','CN',45,'2026-10-02T09:00:00Z'),
         ('sh600036','old quote','CN',40,'2026-10-01T09:00:00Z');
         INSERT INTO stock_daily_prices(symbol,market,date,close,source,updated_at) VALUES
         ('SH600036','CN','2026-09-30',40,'old','2026-10-01T09:00:00Z'),
         ('sh600036','CN','2026-09-30',45,'new','2026-10-02T09:00:00Z'),
         ('SZ000001','CN','2026-09-30',12,'kept','2026-10-02T09:00:00Z');
         PRAGMA user_version=10;",
    )
    .unwrap();
    run_migrations(&mut conn).unwrap();
    let quotes: Vec<(String, String, f64)> = conn
        .prepare("SELECT symbol,name,current_price FROM cached_quotes ORDER BY symbol")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(quotes, [("sh600036".into(), "new quote".into(), 45.0)]);
    let closes: Vec<(String, String, f64)> = conn
        .prepare("SELECT symbol,source,close FROM stock_daily_prices ORDER BY symbol")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        closes,
        [
            ("sh600036".into(), "new".into(), 45.0),
            ("sz000001".into(), "kept".into(), 12.0)
        ]
    );
}

#[test]
fn cn_v11_migration_rolls_back_symbols_and_version_on_failure() {
    let db = Database::new(":memory:").unwrap();
    let mut conn = db.conn.lock().unwrap();
    conn.execute_batch(
        "INSERT INTO accounts(id,name,market,created_at,updated_at) VALUES ('a','A','CN','old','old');
         INSERT INTO holdings(id,account_id,symbol,name,market,currency,created_at,updated_at) VALUES ('h','a','SH600036','Bank','CN','CNY','old','old');
         INSERT INTO transactions(id,account_id,symbol,name,market,transaction_type,shares,price,total_amount,currency,traded_at,created_at) VALUES ('t','a','SH600036','Bank','CN','BUY',100,40,4000,'CNY','2026-09-01','old');
         CREATE TRIGGER block_normalization BEFORE UPDATE OF symbol ON transactions BEGIN SELECT RAISE(ABORT,'test failure'); END;
         PRAGMA user_version=10;",
    ).unwrap();
    assert!(run_migrations(&mut conn).is_err());
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        10
    );
    assert_eq!(
        conn.query_row::<String, _, _>("SELECT symbol FROM holdings WHERE id='h'", [], |r| r
            .get(0))
            .unwrap(),
        "SH600036"
    );
}

#[test]
fn cn_v11_migration_lowercases_business_codes_without_changing_financial_data() {
    let db = Database::new(":memory:").unwrap();
    let mut conn = db.conn.lock().unwrap();
    conn.execute_batch(
        "INSERT INTO accounts(id,name,market,created_at,updated_at) VALUES ('a','A','CN','old','old');
         INSERT INTO holdings(id,account_id,symbol,name,market,shares,avg_cost,currency,created_at,updated_at) VALUES
         ('h','a','SH600036','Bank','CN',100,40.12345678901234,'CNY','old','old'),
         ('cash','a','$CASH-CNY','Cash','CN',123.45,1,'CNY','old','old'),
         ('us','a','AAPL','Apple','US',1,200,'USD','old','old'),
         ('hk','a','700.HK','Tencent','HK',1,400,'HKD','old','old');
         INSERT INTO transactions(id,holding_id,account_id,symbol,name,market,transaction_type,shares,price,total_amount,commission,currency,traded_at,notes,created_at)
         VALUES ('t','h','a','SH600036','Bank','CN','BUY',100,40.12345678901234,4012.345678901234,1.23456789012345,'CNY','2026-09-01','source note','old');
         INSERT INTO daily_holding_snapshots(date,account_id,symbol,market,shares,avg_cost,close_price,market_value)
         VALUES ('2026-09-01','a','SH600036','CN',100,40,42,4200);
         INSERT INTO quarterly_snapshots(id,quarter,snapshot_date,created_at) VALUES ('q','2026-Q3','2026-09-30','old');
         INSERT INTO quarterly_holding_snapshots(id,quarterly_snapshot_id,account_id,symbol,name,market,shares,avg_cost,notes)
         VALUES ('qh','q','a','SH600036','Bank','CN',100,40,'kept');
         PRAGMA user_version=10;",
    ).unwrap();
    let before: (String, f64, f64, f64, f64, String, String) = conn.query_row(
        "SELECT holding_id,shares,price,total_amount,commission,notes,created_at FROM transactions WHERE id='t'", [],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)),
    ).unwrap();
    run_migrations(&mut conn).unwrap();
    for table in [
        "holdings",
        "transactions",
        "daily_holding_snapshots",
        "quarterly_holding_snapshots",
    ] {
        let symbol: String = conn
            .query_row(
                &format!(
                    "SELECT symbol FROM {table} WHERE market='CN' AND symbol NOT LIKE '$CASH-%'"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(symbol, "sh600036", "{table}");
    }
    let after = conn.query_row(
        "SELECT holding_id,shares,price,total_amount,commission,notes,created_at FROM transactions WHERE id='t'", [],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)),
    ).unwrap();
    assert_eq!(before, after);
    for (id, expected) in [("cash", "$CASH-CNY"), ("us", "AAPL"), ("hk", "700.HK")] {
        let symbol: String = conn
            .query_row("SELECT symbol FROM holdings WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(symbol, expected);
    }
    let once = conn.total_changes();
    run_migrations(&mut conn).unwrap();
    assert_eq!(
        conn.total_changes(),
        once,
        "reopening must not repeat the migration"
    );
}
