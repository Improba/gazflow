//! Persistance SQLite pour scénarios topologiques, nominations importées et batchs.
//!
//! Dépôt unique (`scenarios.db`) accédé via un `Arc<Mutex<Connection>>` : la concurrence
//! est faible (écritures ponctuelles, lectures par requête) et rusqlite `Connection`
//! n'est pas `Sync`. Les opérations DB sont appelées depuis `spawn_blocking` côté API.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS topological_scenarios (
    id           TEXT PRIMARY KEY,
    dataset_id   TEXT NOT NULL,
    name         TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    diff_json    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_topo_dataset ON topological_scenarios(dataset_id);

CREATE TABLE IF NOT EXISTS imported_nominations (
    id            TEXT PRIMARY KEY,
    dataset_id    TEXT NOT NULL,
    filename      TEXT NOT NULL,
    source        TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    xml           TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_nominations_dataset ON imported_nominations(dataset_id);

CREATE TABLE IF NOT EXISTS batch_runs (
    id            TEXT PRIMARY KEY,
    dataset_id    TEXT NOT NULL,
    name          TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    config_json   TEXT NOT NULL,
    status        TEXT NOT NULL,
    results_json  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_batch_dataset ON batch_runs(dataset_id);
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologicalScenarioRecord {
    pub id: String,
    pub dataset_id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub diff_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedNominationRecord {
    pub id: String,
    pub dataset_id: String,
    pub filename: String,
    pub source: String,
    pub created_at_ms: u64,
    pub xml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunRecord {
    pub id: String,
    pub dataset_id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub config_json: String,
    pub status: String,
    pub results_json: String,
}

/// Dépôt SQLite partagé. Cloneable (l'`Arc<Mutex>` est partagé).
#[derive(Clone)]
pub struct ScenarioRepo {
    conn: Arc<Mutex<Connection>>,
}

impl ScenarioRepo {
    /// Ouvre (ou crée) la base à `path`. Si `path` est `None`, utilise un DB en mémoire
    /// (utile pour les tests).
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let conn = match path {
            Some(p) => {
                if let Some(parent) = p.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).ok();
                    }
                }
                Connection::open(p).with_context(|| format!("ouverture DB {:?}", p))?
            }
            None => Connection::open_in_memory()?,
        };
        conn.execute_batch(SCHEMA)
            .with_context(|| "initialisation du schéma SQLite")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn default_path(data_dir: &Path) -> PathBuf {
        data_dir.join("scenarios.db")
    }

    // --- Scénarios topologiques ---

    pub fn list_topological_scenarios(&self, dataset_id: &str) -> Result<Vec<TopologicalScenarioRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, name, created_at_ms, diff_json FROM topological_scenarios WHERE dataset_id = ?1 ORDER BY created_at_ms ASC",
        )?;
        let rows = stmt.query_map(params![dataset_id], |row| {
            Ok(TopologicalScenarioRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                name: row.get(2)?,
                created_at_ms: row.get(3)?,
                diff_json: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_topological_scenario(&self, id: &str) -> Result<Option<TopologicalScenarioRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, name, created_at_ms, diff_json FROM topological_scenarios WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(TopologicalScenarioRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                name: row.get(2)?,
                created_at_ms: row.get(3)?,
                diff_json: row.get(4)?,
            })
        })?;
        match rows.next() {
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(e)) => Err(anyhow!(e)),
            None => Ok(None),
        }
    }

    pub fn insert_topological_scenario(&self, record: &TopologicalScenarioRecord) -> Result<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT OR REPLACE INTO topological_scenarios (id, dataset_id, name, created_at_ms, diff_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.id,
                record.dataset_id,
                record.name,
                record.created_at_ms as i64,
                record.diff_json,
            ],
        )?;
        Ok(())
    }

    pub fn delete_topological_scenario(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("db lock");
        let affected = conn.execute(
            "DELETE FROM topological_scenarios WHERE id = ?1",
            params![id],
        )?;
        Ok(affected > 0)
    }

    // --- Nominations importées ---

    pub fn list_imported_nominations(&self, dataset_id: &str) -> Result<Vec<ImportedNominationRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, filename, source, created_at_ms, xml FROM imported_nominations WHERE dataset_id = ?1 ORDER BY created_at_ms ASC",
        )?;
        let rows = stmt.query_map(params![dataset_id], |row| {
            Ok(ImportedNominationRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                filename: row.get(2)?,
                source: row.get(3)?,
                created_at_ms: row.get(4)?,
                xml: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_imported_nomination(&self, id: &str) -> Result<Option<ImportedNominationRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, filename, source, created_at_ms, xml FROM imported_nominations WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ImportedNominationRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                filename: row.get(2)?,
                source: row.get(3)?,
                created_at_ms: row.get(4)?,
                xml: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(e)) => Err(anyhow!(e)),
            None => Ok(None),
        }
    }

    /// Recherche une nomination importée par `id` (recherche dans tous les datasets,
    /// utile car les `.scn` bundlés ne sont pas filtrés par dataset actuellement).
    pub fn find_imported_nomination(&self, id: &str) -> Result<Option<ImportedNominationRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, filename, source, created_at_ms, xml FROM imported_nominations WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ImportedNominationRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                filename: row.get(2)?,
                source: row.get(3)?,
                created_at_ms: row.get(4)?,
                xml: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(e)) => Err(anyhow!(e)),
            None => Ok(None),
        }
    }

    pub fn insert_imported_nomination(&self, record: &ImportedNominationRecord) -> Result<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT OR REPLACE INTO imported_nominations (id, dataset_id, filename, source, created_at_ms, xml) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                record.dataset_id,
                record.filename,
                record.source,
                record.created_at_ms as i64,
                record.xml,
            ],
        )?;
        Ok(())
    }

    pub fn delete_imported_nomination(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("db lock");
        let affected = conn.execute("DELETE FROM imported_nominations WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    // --- Batch runs ---

    pub fn list_batch_runs(&self, dataset_id: &str) -> Result<Vec<BatchRunRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, name, created_at_ms, config_json, status, results_json FROM batch_runs WHERE dataset_id = ?1 ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt.query_map(params![dataset_id], |row| {
            Ok(BatchRunRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                name: row.get(2)?,
                created_at_ms: row.get(3)?,
                config_json: row.get(4)?,
                status: row.get(5)?,
                results_json: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_batch_run(&self, id: &str) -> Result<Option<BatchRunRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, dataset_id, name, created_at_ms, config_json, status, results_json FROM batch_runs WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(BatchRunRecord {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                name: row.get(2)?,
                created_at_ms: row.get(3)?,
                config_json: row.get(4)?,
                status: row.get(5)?,
                results_json: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(e)) => Err(anyhow!(e)),
            None => Ok(None),
        }
    }

    pub fn insert_batch_run(&self, record: &BatchRunRecord) -> Result<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT OR REPLACE INTO batch_runs (id, dataset_id, name, created_at_ms, config_json, status, results_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.dataset_id,
                record.name,
                record.created_at_ms as i64,
                record.config_json,
                record.status,
                record.results_json,
            ],
        )?;
        Ok(())
    }

    pub fn delete_batch_run(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("db lock");
        let affected = conn.execute("DELETE FROM batch_runs WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_scenario_roundtrip() {
        let repo = ScenarioRepo::open(None).unwrap();
        let rec = TopologicalScenarioRecord {
            id: "scn-1".into(),
            dataset_id: "GasLib-11".into(),
            name: "Variante A".into(),
            created_at_ms: 123,
            diff_json: "{\"nodes\":{}}".into(),
        };
        repo.insert_topological_scenario(&rec).unwrap();
        let listed = repo.list_topological_scenarios("GasLib-11").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Variante A");
        let fetched = repo.get_topological_scenario("scn-1").unwrap().unwrap();
        assert_eq!(fetched.diff_json, "{\"nodes\":{}}");
        assert!(repo.delete_topological_scenario("scn-1").unwrap());
        assert!(repo.list_topological_scenarios("GasLib-11").unwrap().is_empty());
    }

    #[test]
    fn imported_nomination_find_across_datasets() {
        let repo = ScenarioRepo::open(None).unwrap();
        let rec = ImportedNominationRecord {
            id: "nom-custom-1".into(),
            dataset_id: "GasLib-582".into(),
            filename: "custom.scn".into(),
            source: "imported".into(),
            created_at_ms: 42,
            xml: "<scenario/>".into(),
        };
        repo.insert_imported_nomination(&rec).unwrap();
        let found = repo.find_imported_nomination("nom-custom-1").unwrap().unwrap();
        assert_eq!(found.xml, "<scenario/>");
        assert!(repo.delete_imported_nomination("nom-custom-1").unwrap());
    }

    #[test]
    fn batch_run_roundtrip() {
        let repo = ScenarioRepo::open(None).unwrap();
        let rec = BatchRunRecord {
            id: "batch-1".into(),
            dataset_id: "GasLib-11".into(),
            name: "Sweep".into(),
            created_at_ms: 7,
            config_json: "{}".into(),
            status: "done".into(),
            results_json: "[]".into(),
        };
        repo.insert_batch_run(&rec).unwrap();
        let got = repo.get_batch_run("batch-1").unwrap().unwrap();
        assert_eq!(got.status, "done");
        assert!(repo.delete_batch_run("batch-1").unwrap());
    }
}
