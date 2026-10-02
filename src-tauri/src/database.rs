use anyhow::Result;
use chrono::{DateTime, Utc};
use dirs;
use log::{error, info, warn};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferHistory {
    pub id: i64,
    pub transfer_id: String,
    pub timestamp: DateTime<Utc>,
    pub direction: String, // "sent" or "received"
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub total_size: u64,
    pub status: String, // "completed", "failed", "cancelled"
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub id: i64,
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub trusted: bool,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub id: i64,
    pub key: String,
    pub value: String,
}

pub struct Database {
    pub conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Create tables if they don't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS transfer_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                transfer_id TEXT NOT NULL,
                timestamp DATETIME NOT NULL,
                direction TEXT NOT NULL,
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                platform TEXT NOT NULL,
                total_size INTEGER NOT NULL,
                status TEXT NOT NULL,
                verified BOOLEAN NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS devices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT UNIQUE NOT NULL,
                device_name TEXT NOT NULL,
                platform TEXT NOT NULL,
                trusted BOOLEAN NOT NULL DEFAULT 0,
                last_seen DATETIME NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT UNIQUE NOT NULL,
                value TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn add_transfer_record(&self, record: TransferHistory) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        conn.execute(
            "INSERT INTO transfer_history
             (transfer_id, timestamp, direction, device_id, device_name, platform, total_size, status, verified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.transfer_id,
                record.timestamp.to_rfc3339(),
                record.direction,
                record.device_id,
                record.device_name,
                record.platform,
                record.total_size,
                record.status,
                record.verified
            ],
        )?;
        Ok(())
    }

    pub fn get_transfer_history(&self, limit: Option<i64>) -> Result<Vec<TransferHistory>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        let mut stmt = if let Some(limit) = limit {
            conn.prepare(
                "SELECT id, transfer_id, timestamp, direction, device_id, device_name, platform, total_size, status, verified 
                 FROM transfer_history 
                 ORDER BY timestamp DESC 
                 LIMIT ?1"
            )?
        } else {
            conn.prepare(
                "SELECT id, transfer_id, timestamp, direction, device_id, device_name, platform, total_size, status, verified 
                 FROM transfer_history 
                 ORDER BY timestamp DESC"
            )?
        };

        let rows = stmt.query_map(params![limit.unwrap_or(-1)], |row| {
            Ok(TransferHistory {
                id: row.get(0)?,
                transfer_id: row.get(1)?,
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .unwrap()
                    .with_timezone(&Utc),
                direction: row.get(3)?,
                device_id: row.get(4)?,
                device_name: row.get(5)?,
                platform: row.get(6)?,
                total_size: row.get(7)?,
                status: row.get(8)?,
                verified: row.get(9)?,
            })
        })?;

        let mut history = Vec::new();
        for row in rows {
            history.push(row?);
        }
        Ok(history)
    }

    pub fn add_or_update_device(&self, device: DeviceRecord) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO devices 
             (device_id, device_name, platform, trusted, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                device.device_id,
                device.device_name,
                device.platform,
                device.trusted,
                device.last_seen.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_trusted_devices(&self) -> Result<Vec<DeviceRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, device_id, device_name, platform, trusted, last_seen 
             FROM devices 
             WHERE trusted = 1
             ORDER BY last_seen DESC",
        )?;

        let rows = stmt.query_map(params![], |row| {
            Ok(DeviceRecord {
                id: row.get(0)?,
                device_id: row.get(1)?,
                device_name: row.get(2)?,
                platform: row.get(3)?,
                trusted: row.get(4)?,
                last_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .unwrap()
                    .with_timezone(&Utc),
            })
        })?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row?);
        }
        Ok(devices)
    }

    pub fn set_setting(&self, key: String, value: String) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: String) -> Result<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get(0))?;

        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    /// Get all settings as a JSON string
    pub fn get_all_settings(&self) -> Result<serde_json::Value> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;

        let rows = stmt.query_map(params![], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;

        let mut settings = serde_json::Map::new();
        for row in rows {
            let (key, value) = row?;
            settings.insert(key, serde_json::Value::String(value));
        }

        Ok(serde_json::Value::Object(settings))
    }

    /// Clear all transfer history records
    pub fn clear_transfer_history(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::format_err!("Failed to lock database connection: {}", e))?;
        conn.execute("DELETE FROM transfer_history", [])?;
        Ok(())
    }
}

// Initialize database
pub fn init_database() -> Result<Database> {
    // Get platform-appropriate data directory
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?
        .join("Bridge");

    // Create directory if it doesn't exist
    fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("bridge.db");
    Database::new(db_path)
}

#[tauri::command]
pub fn get_transfer_history(
    state: State<'_, Database>,
    limit: Option<i64>,
) -> Result<String, String> {
    let history = state
        .get_transfer_history(limit)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_string(&history).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn add_transfer_record(
    state: State<'_, Database>,
    transfer_id: String,
    direction: String,
    device_id: String,
    device_name: String,
    platform: String,
    total_size: u64,
    status: String,
    verified: bool,
) -> Result<(), String> {
    let record = TransferHistory {
        id: 0, // Will be set by database
        transfer_id,
        timestamp: Utc::now(),
        direction,
        device_id,
        device_name,
        platform,
        total_size,
        status,
        verified,
    };

    state
        .add_transfer_record(record)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_trusted_devices(state: State<'_, Database>) -> Result<String, String> {
    let devices = state.get_trusted_devices().map_err(|e| e.to_string())?;
    Ok(serde_json::to_string(&devices).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn set_setting(state: State<'_, Database>, key: String, value: String) -> Result<(), String> {
    state.set_setting(key, value).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_setting(state: State<'_, Database>, key: String) -> Result<Option<String>, String> {
    match state.get_setting(key) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn get_all_settings(state: State<'_, Database>) -> Result<String, String> {
    let settings = state.get_all_settings().map_err(|e| e.to_string())?;
    Ok(serde_json::to_string(&settings).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn clear_transfer_history(state: State<'_, Database>) -> Result<(), String> {
    state.clear_transfer_history().map_err(|e| e.to_string())?;
    Ok(())
}
