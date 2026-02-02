// SQLite database module for quota caching

use anyhow::Result;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

static DB_CONNECTION: OnceCell<Mutex<Connection>> = OnceCell::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedQuota {
    pub account_id: String,
    pub provider: String,
    pub quota_data: String,
    pub last_updated: i64,
}

/// Initialize the SQLite database
pub fn init_db(app_data_dir: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&app_data_dir)?;
    let db_path = app_data_dir.join("quota_cache.db");

    let conn = Connection::open(&db_path)?;

    // Create tables
    conn.execute(
        "CREATE TABLE IF NOT EXISTS quota_cache (
            account_id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            quota_data TEXT NOT NULL,
            last_updated INTEGER NOT NULL
        )",
        [],
    )?;

    tracing::info!("SQLite database initialized at {:?}", db_path);

    DB_CONNECTION
        .set(Mutex::new(conn))
        .map_err(|_| anyhow::anyhow!("Database already initialized"))?;

    Ok(())
}

/// Save quota data to cache
pub fn save_quota_cache(account_id: &str, provider: &str, quota_data: &str) -> Result<()> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT OR REPLACE INTO quota_cache (account_id, provider, quota_data, last_updated)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![account_id, provider, quota_data, now],
    )?;

    tracing::debug!("Saved quota cache for account: {}", account_id);
    Ok(())
}

/// Get cached quota for a specific account
pub fn get_quota_cache(account_id: &str) -> Result<Option<CachedQuota>> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();

    let mut stmt = conn.prepare(
        "SELECT account_id, provider, quota_data, last_updated FROM quota_cache WHERE account_id = ?1",
    )?;

    let result = stmt.query_row([account_id], |row| {
        Ok(CachedQuota {
            account_id: row.get(0)?,
            provider: row.get(1)?,
            quota_data: row.get(2)?,
            last_updated: row.get(3)?,
        })
    });

    match result {
        Ok(quota) => Ok(Some(quota)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all cached quotas
pub fn get_all_quota_cache() -> Result<HashMap<String, CachedQuota>> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();

    let mut stmt = conn.prepare(
        "SELECT account_id, provider, quota_data, last_updated FROM quota_cache",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CachedQuota {
            account_id: row.get(0)?,
            provider: row.get(1)?,
            quota_data: row.get(2)?,
            last_updated: row.get(3)?,
        })
    })?;

    let mut result = HashMap::new();
    for row in rows {
        let quota = row?;
        result.insert(quota.account_id.clone(), quota);
    }

    Ok(result)
}

/// Delete cached quota for a specific account
pub fn delete_quota_cache(account_id: &str) -> Result<()> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();

    conn.execute(
        "DELETE FROM quota_cache WHERE account_id = ?1",
        [account_id],
    )?;

    tracing::debug!("Deleted quota cache for account: {}", account_id);
    Ok(())
}
