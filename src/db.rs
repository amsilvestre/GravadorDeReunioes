use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn init() -> Result<Self> {
        let db_path = Self::db_path()?;

        // Garante que o diretorio existe
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;

        // Cria tabelas
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS recordings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                created_at TEXT NOT NULL,
                duration_secs INTEGER DEFAULT 0,
                transcription_status TEXT DEFAULT 'none',
                transcription_text TEXT
            );",
        )?;

        // Migration: adiciona coluna display_name se nao existir
        let _ = conn.execute("ALTER TABLE recordings ADD COLUMN display_name TEXT", []);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn db_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().ok_or_else(|| {
            anyhow::anyhow!("Nao foi possivel encontrar o diretorio de configuracao")
        })?;
        Ok(config_dir.join("GravadorDeReunioes").join("gravador.db"))
    }

    pub fn get_config(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM config WHERE key = ?1")?;
        let result = stmt.query_row([key], |row| row.get::<_, String>(0));
        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    pub fn add_recording(&self, file_path: &str, created_at: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO recordings (file_path, created_at) VALUES (?1, ?2)",
            [file_path, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_recording_duration(&self, id: i64, duration_secs: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE recordings SET duration_secs = ?1 WHERE id = ?2",
            rusqlite::params![duration_secs, id],
        )?;
        Ok(())
    }

    pub fn update_recording_transcription(
        &self,
        id: i64,
        status: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE recordings SET transcription_status = ?1, transcription_text = ?2 WHERE id = ?3",
            rusqlite::params![status, text, id],
        )?;
        Ok(())
    }

    pub fn get_all_recordings(&self) -> Result<Vec<RecordingRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, file_path, created_at, duration_secs, transcription_status, transcription_text, display_name
             FROM recordings ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RecordingRow {
                id: row.get(0)?,
                file_path: row.get(1)?,
                created_at: row.get(2)?,
                duration_secs: row.get(3)?,
                transcription_status: row.get(4)?,
                transcription_text: row.get(5)?,
                display_name: row.get(6)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn delete_recording(&self, id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT file_path FROM recordings WHERE id = ?1")?;
        let file_path: Option<String> = stmt.query_row([id], |row| row.get(0)).ok();
        conn.execute(
            "DELETE FROM recordings WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(file_path)
    }

    pub fn rename_recording(&self, id: i64, display_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE recordings SET display_name = ?1 WHERE id = ?2",
            rusqlite::params![display_name, id],
        )?;
        Ok(())
    }
}

pub struct RecordingRow {
    pub id: i64,
    pub file_path: String,
    pub created_at: String,
    pub duration_secs: i64,
    pub transcription_status: String,
    pub transcription_text: Option<String>,
    pub display_name: Option<String>,
}
