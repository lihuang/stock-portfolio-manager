use crate::db::Database;
use chrono::{NaiveDate, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DailyMarketPoint {
    pub date: NaiveDate,
    pub close: f64,
}

/// Store authoritative daily closes without fabricating OHLCV fields.
pub(crate) fn upsert_stock_closes(
    db: &Database,
    symbol: &str,
    market: &str,
    source: &str,
    prices: &[(NaiveDate, f64)],
) -> Result<(), String> {
    let symbol = crate::symbols::normalize_cn_stock_symbol(symbol, market);
    let mut conn = db.conn.lock().map_err(|error| error.to_string())?;
    let transaction = conn.transaction().map_err(|error| error.to_string())?;
    let updated_at = Utc::now().to_rfc3339();

    for (date, close) in prices {
        if !close.is_finite() || *close <= 0.0 {
            continue;
        }
        transaction
            .execute(
                "INSERT INTO stock_daily_prices
                    (symbol, market, date, open, high, low, close, volume, adjusted_close, dividend, source, updated_at)
                 VALUES (?1, ?2, ?3, NULL, NULL, NULL, ?4, NULL, NULL, NULL, ?5, ?6)
                 ON CONFLICT(symbol, market, date) DO UPDATE SET
                    open = NULL,
                    high = NULL,
                    low = NULL,
                    close = excluded.close,
                    volume = NULL,
                    adjusted_close = NULL,
                    dividend = NULL,
                    source = excluded.source,
                    updated_at = excluded.updated_at",
                params![
                    symbol,
                    market,
                    date.format("%Y-%m-%d").to_string(),
                    close,
                    source,
                    updated_at,
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

/// Load cached stock closes ordered by their actual observation dates.
pub(crate) fn load_stock_price_series(
    db: &Database,
    symbol: &str,
    market: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<DailyMarketPoint>, String> {
    let symbol = crate::symbols::normalize_cn_stock_symbol(symbol, market);
    let conn = db.conn.lock().map_err(|error| error.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT date, close
             FROM stock_daily_prices
             WHERE symbol = ?1 AND market = ?2 AND date BETWEEN ?3 AND ?4
               AND NOT (
                   ?2 IN ('CN', 'HK')
                   AND LOWER(source) = 'xueqiu'
                   AND (open IS NOT NULL OR high IS NOT NULL OR low IS NOT NULL OR volume IS NOT NULL)
               )
             ORDER BY date ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                symbol,
                market,
                start.format("%Y-%m-%d").to_string(),
                end.format("%Y-%m-%d").to_string(),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
        )
        .map_err(|error| error.to_string())?;

    rows.map(|row| {
        let (date, close) = row.map_err(|error| error.to_string())?;
        let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map_err(|error| format!("Invalid cached stock price date '{}': {}", date, error))?;
        Ok(DailyMarketPoint { date, close })
    })
    .collect()
}

pub(crate) fn default_benchmark_symbol(market: &str) -> Option<&'static str> {
    match market {
        "US" => Some("^GSPC"),
        "CN" => Some("000300.SS"),
        "HK" => Some("^HSI"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{default_benchmark_symbol, load_stock_price_series, upsert_stock_closes};
    use crate::db::Database;
    use chrono::NaiveDate;

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn mainland_close_cache_uses_one_lowercase_row_for_either_caller_case() {
        let db = Database::new(":memory:").unwrap();
        let day = date("2026-10-12");
        for (uppercase, lowercase) in [
            ("SH600519", "sh600519"),
            ("SZ000858", "sz000858"),
            ("BJ430047", "bj430047"),
        ] {
            upsert_stock_closes(&db, uppercase, "CN", "test", &[(day, 20.0)]).unwrap();
            upsert_stock_closes(&db, lowercase, "CN", "test", &[(day, 24.0)]).unwrap();
            let rows: Vec<(String, f64)> = db.conn.lock().unwrap()
                .prepare("SELECT symbol, close FROM stock_daily_prices WHERE market = 'CN' AND LOWER(symbol) = ?1")
                .unwrap()
                .query_map([lowercase], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(rows, vec![(lowercase.to_string(), 24.0)]);
            for caller in [uppercase, lowercase] {
                let points = load_stock_price_series(&db, caller, "CN", day, day).unwrap();
                assert_eq!(
                    points,
                    vec![super::DailyMarketPoint {
                        date: day,
                        close: 24.0
                    }]
                );
            }
        }
    }

    #[test]
    fn close_cache_preserves_symbols_outside_mainland_prefixed_codes() {
        let db = Database::new(":memory:").unwrap();
        let day = date("2026-10-12");
        for (symbol, market) in [
            ("AAPL", "US"),
            ("700.HK", "HK"),
            ("$CASH-CNY", "CN"),
            ("000300.SS", "CN"),
            ("600519", "CN"),
            ("SH600519", "US"),
        ] {
            upsert_stock_closes(&db, symbol, market, "test", &[(day, 20.0)]).unwrap();
            let stored: String = db
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT symbol FROM stock_daily_prices WHERE symbol = ?1 AND market = ?2",
                    [symbol, market],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(stored, symbol);
            assert_eq!(
                load_stock_price_series(&db, symbol, market, day, day)
                    .unwrap()
                    .len(),
                1
            );
        }
    }

    #[test]
    fn endpoint_cache_reads_only_real_rows_in_requested_range() {
        let db = Database::new(":memory:").unwrap();
        upsert_stock_closes(
            &db,
            "001248",
            "CN",
            "test",
            &[(date("2026-07-02"), 20.0), (date("2026-07-31"), 24.0)],
        )
        .unwrap();

        let points =
            load_stock_price_series(&db, "001248", "CN", date("2026-06-30"), date("2026-07-31"))
                .unwrap();
        assert_eq!(
            points.iter().map(|point| point.date).collect::<Vec<_>>(),
            [date("2026-07-02"), date("2026-07-31")],
        );
        assert_eq!(
            points.iter().map(|point| point.close).collect::<Vec<_>>(),
            [20.0, 24.0],
        );
    }

    #[test]
    fn endpoint_cache_excludes_legacy_cn_xueqiu_ohlcv_rows() {
        let db = Database::new(":memory:").unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO stock_daily_prices
                    (symbol, market, date, open, high, low, close, volume, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    "001248",
                    "CN",
                    "2026-07-01",
                    19.0,
                    21.0,
                    18.0,
                    20.0,
                    1_000.0,
                    "XueQiu",
                    "2026-07-01T00:00:00Z",
                ],
            )
            .unwrap();
        upsert_stock_closes(
            &db,
            "001248",
            "CN",
            "authoritative",
            &[(date("2026-07-02"), 24.0)],
        )
        .unwrap();

        let points =
            load_stock_price_series(&db, "001248", "CN", date("2026-07-01"), date("2026-07-02"))
                .unwrap();

        assert_eq!(
            points,
            vec![super::DailyMarketPoint {
                date: date("2026-07-02"),
                close: 24.0,
            }],
        );
    }

    #[test]
    fn endpoint_cache_excludes_legacy_hk_xueqiu_ohlcv_rows() {
        let db = Database::new(":memory:").unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO stock_daily_prices
                    (symbol, market, date, open, high, low, close, volume, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    "00700",
                    "HK",
                    "2026-07-01",
                    499.0,
                    501.0,
                    498.0,
                    500.0,
                    1_000.0,
                    "xueqiu",
                    "2026-07-01T00:00:00Z",
                ],
            )
            .unwrap();
        upsert_stock_closes(
            &db,
            "00700",
            "HK",
            "authoritative",
            &[(date("2026-07-02"), 504.0)],
        )
        .unwrap();

        let points =
            load_stock_price_series(&db, "00700", "HK", date("2026-07-01"), date("2026-07-02"))
                .unwrap();

        assert_eq!(
            points,
            vec![super::DailyMarketPoint {
                date: date("2026-07-02"),
                close: 504.0,
            }],
        );
    }

    #[test]
    fn endpoint_cache_keeps_us_xueqiu_ohlcv_rows() {
        let db = Database::new(":memory:").unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO stock_daily_prices
                    (symbol, market, date, open, high, low, close, volume, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    "AAPL",
                    "US",
                    "2026-07-01",
                    199.0,
                    201.0,
                    198.0,
                    200.0,
                    1_000.0,
                    "xueqiu",
                    "2026-07-01T00:00:00Z",
                ],
            )
            .unwrap();

        let points =
            load_stock_price_series(&db, "AAPL", "US", date("2026-07-01"), date("2026-07-01"))
                .unwrap();

        assert_eq!(
            points,
            vec![super::DailyMarketPoint {
                date: date("2026-07-01"),
                close: 200.0,
            }],
        );
    }

    #[test]
    fn default_benchmarks_match_supported_markets() {
        assert_eq!(default_benchmark_symbol("US"), Some("^GSPC"));
        assert_eq!(default_benchmark_symbol("CN"), Some("000300.SS"));
        assert_eq!(default_benchmark_symbol("HK"), Some("^HSI"));
        assert_eq!(default_benchmark_symbol("OTHER"), None);
    }
}
