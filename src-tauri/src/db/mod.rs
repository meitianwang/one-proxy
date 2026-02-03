// SQLite database module for quota caching and request logs

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogEntry {
    pub id: i64,
    pub status: i32,
    pub method: String,
    pub model: Option<String>,
    pub protocol: Option<String>,
    pub provider: Option<String>,
    pub account_id: Option<String>,
    pub path: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub duration_ms: i64,
    pub timestamp: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogFilter {
    #[serde(default)]
    pub errors_only: bool,
    pub protocol: Option<String>,
    pub search: Option<String>,
    pub account_id: Option<String>,
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

    // Create request_logs table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            status INTEGER NOT NULL,
            method TEXT NOT NULL,
            model TEXT,
            protocol TEXT,
            provider TEXT,
            account_id TEXT,
            path TEXT NOT NULL,
            input_tokens INTEGER DEFAULT 0,
            output_tokens INTEGER DEFAULT 0,
            duration_ms INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            error_message TEXT
        )",
        [],
    )?;
    
    // Add provider column if it doesn't exist (migration for existing databases)
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN provider TEXT", []);

    // Create index for faster queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_request_logs_timestamp ON request_logs(timestamp DESC)",
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

// ============ Request Logs Functions ============

/// Save a request log entry
pub fn save_request_log(
    status: i32,
    method: &str,
    model: Option<&str>,
    protocol: Option<&str>,
    provider: Option<&str>,
    account_id: Option<&str>,
    path: &str,
    input_tokens: i32,
    output_tokens: i32,
    duration_ms: i64,
    error_message: Option<&str>,
) -> Result<()> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();
    let now = chrono::Utc::now().timestamp_millis();

    conn.execute(
        "INSERT INTO request_logs (status, method, model, protocol, provider, account_id, path, input_tokens, output_tokens, duration_ms, timestamp, error_message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![status, method, model, protocol, provider, account_id, path, input_tokens, output_tokens, duration_ms, now, error_message],
    )?;

    tracing::debug!("Saved request log: {} {} -> {}", method, path, status);
    Ok(())
}


/// Get request logs with optional filtering
pub fn get_request_logs(limit: u32, offset: u32, filter: Option<LogFilter>) -> Result<Vec<RequestLogEntry>> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();
    let filter = filter.unwrap_or_default();

    let mut sql = String::from(
        "SELECT id, status, method, model, protocol, provider, account_id, path, input_tokens, output_tokens, duration_ms, timestamp, error_message
         FROM request_logs WHERE 1=1"
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if filter.errors_only {
        sql.push_str(" AND status >= 400");
    }

    if let Some(ref protocol) = filter.protocol {
        sql.push_str(" AND protocol = ?");
        params.push(Box::new(protocol.clone()));
    }

    if let Some(ref account_id) = filter.account_id {
        sql.push_str(" AND account_id = ?");
        params.push(Box::new(account_id.clone()));
    }

    if let Some(ref search) = filter.search {
        sql.push_str(" AND (path LIKE ? OR model LIKE ?)");
        let search_pattern = format!("%{}%", search);
        params.push(Box::new(search_pattern.clone()));
        params.push(Box::new(search_pattern));
    }

    sql.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");
    params.push(Box::new(limit));
    params.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(RequestLogEntry {
            id: row.get(0)?,
            status: row.get(1)?,
            method: row.get(2)?,
            model: row.get(3)?,
            protocol: row.get(4)?,
            provider: row.get(5)?,
            account_id: row.get(6)?,
            path: row.get(7)?,
            input_tokens: row.get(8)?,
            output_tokens: row.get(9)?,
            duration_ms: row.get(10)?,
            timestamp: row.get(11)?,
            error_message: row.get(12)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }

    Ok(result)
}


/// Get count of request logs with optional filtering
pub fn get_request_logs_count(filter: Option<LogFilter>) -> Result<i64> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();
    let filter = filter.unwrap_or_default();

    let mut sql = String::from("SELECT COUNT(*) FROM request_logs WHERE 1=1");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if filter.errors_only {
        sql.push_str(" AND status >= 400");
    }

    if let Some(ref protocol) = filter.protocol {
        sql.push_str(" AND protocol = ?");
        params.push(Box::new(protocol.clone()));
    }

    if let Some(ref account_id) = filter.account_id {
        sql.push_str(" AND account_id = ?");
        params.push(Box::new(account_id.clone()));
    }

    if let Some(ref search) = filter.search {
        sql.push_str(" AND (path LIKE ? OR model LIKE ?)");
        let search_pattern = format!("%{}%", search);
        params.push(Box::new(search_pattern.clone()));
        params.push(Box::new(search_pattern));
    }

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))?;

    Ok(count)
}

/// Clear all request logs
pub fn clear_request_logs() -> Result<()> {
    let conn = DB_CONNECTION
        .get()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;

    let conn = conn.lock();
    conn.execute("DELETE FROM request_logs", [])?;

    tracing::info!("Cleared all request logs");
    Ok(())
}
