use crate::symbols::{is_cn_stock_symbol, normalize_cn_stock_symbol};
use rusqlite::{params, Connection, Result};
use std::collections::HashSet;

/// Change spelling only: preserve business row IDs, quantities, costs and dates.
/// Import source/audit JSON stays unchanged; its working copies are normalized
/// by import_batch when comparing or restoring a saved account state.
pub(super) fn migrate_v11(conn: &Connection) -> Result<()> {
    for table in [
        "holdings",
        "transactions",
        "daily_holding_snapshots",
        "quarterly_holding_snapshots",
        "price_alerts",
    ] {
        let rows = conn
            .prepare(&format!(
                "SELECT rowid,symbol FROM {table} WHERE market='CN'"
            ))?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        for (id, symbol) in rows {
            let normalized = normalize_cn_stock_symbol(&symbol, "CN");
            if normalized != symbol {
                conn.execute(
                    &format!("UPDATE {table} SET symbol=?1 WHERE rowid=?2"),
                    params![normalized, id],
                )?;
            }
        }
    }

    normalize_cache(conn, "cached_quotes", "''")?;
    normalize_cache(conn, "stock_daily_prices", "date")
}

/// Cache aliases may already coexist under case-sensitive primary keys. Keep
/// the latest observation for each normalized key before updating its spelling.
fn normalize_cache(conn: &Connection, table: &str, date_column: &str) -> Result<()> {
    let rows = conn
        .prepare(&format!(
            "SELECT rowid,symbol,{date_column} FROM {table} WHERE market='CN'
             ORDER BY JULIANDAY(updated_at) DESC,rowid DESC"
        ))?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    let mut updates = Vec::new();
    for (id, symbol, date) in rows {
        if !is_cn_stock_symbol(&symbol) {
            continue;
        }
        let normalized = normalize_cn_stock_symbol(&symbol, "CN");
        if !seen.insert((normalized.clone(), date)) {
            conn.execute(&format!("DELETE FROM {table} WHERE rowid=?1"), [id])?;
        } else if normalized != symbol {
            updates.push((id, normalized));
        }
    }
    for (id, symbol) in updates {
        conn.execute(
            &format!("UPDATE {table} SET symbol=?1 WHERE rowid=?2"),
            params![symbol, id],
        )?;
    }
    Ok(())
}
