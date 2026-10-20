//! Bring data over from the official CC Switch directory (`~/.cc-switch`).
//!
//! MultiClaude keeps its own `~/.multiclaude`, so a machine that ran CC Switch
//! before starts with an empty usage history and none of its providers. The
//! import is additive: nothing already in MultiClaude is changed or removed.
//!
//! - providers: one with the same app and name already here is kept as is and
//!   the old one maps onto it; the rest are copied with their keys and
//!   endpoints (never as the current provider). An id taken by a different
//!   provider gets a `-cc-switch` suffix.
//! - request logs and daily rollups: rows missing here, with provider ids mapped.
//! - MCP servers, prompts, skills, skill repos, pricing and session-log sync
//!   state: rows whose key is missing here.
//! - files: skill folders and database backups that are not here yet.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;

use super::{lock_conn, Database};
use crate::error::AppError;

/// Tables copied row by row with `INSERT OR IGNORE`, keyed by their own primary key.
const PLAIN_TABLES: &[&str] = &[
    "mcp_servers",
    "prompts",
    "skills",
    "skill_repos",
    "model_pricing",
    "session_log_sync",
    "session_usage_dedup",
];

/// Tables whose `provider_id` follows the provider mapping.
const PROVIDER_TABLES: &[&str] = &["proxy_request_logs", "usage_daily_rollups"];

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportReport {
    /// `~/.cc-switch/cc-switch.db` exists.
    pub found: bool,
    pub path: String,
    pub providers: usize,
    pub request_logs: usize,
    pub rollups: usize,
    pub other_rows: usize,
    pub skill_folders: usize,
    pub backups: usize,
    /// Safety backup of the MultiClaude database taken before importing.
    pub backup_file: Option<String>,
}

impl LegacyImportReport {
    fn nothing_new(&self) -> bool {
        self.providers
            + self.request_logs
            + self.rollups
            + self.other_rows
            + self.skill_folders
            + self.backups
            == 0
    }
}

pub fn legacy_dir() -> PathBuf {
    crate::config::get_home_dir().join(".cc-switch")
}

impl Database {
    /// What an import from `dir` would bring, without writing anything.
    pub fn legacy_import_preview(&self, dir: &Path) -> Result<LegacyImportReport, AppError> {
        let mut report = base_report(dir);
        if !report.found || same_dir(dir) {
            report.found = false;
            return Ok(report);
        }
        let conn = lock_conn!(self.conn);
        with_legacy_attached(&conn, &dir.join("cc-switch.db"), |conn| {
            let plan = plan_providers(conn)?;
            report.providers = plan.copies.len();
            for table in PROVIDER_TABLES {
                report_rows(&mut report, table, count_missing(conn, table)?);
            }
            for table in PLAIN_TABLES {
                report.other_rows += count_missing(conn, table)?;
            }
            Ok(())
        })?;
        report.skill_folders = missing_entries(&dir.join("skills"), &skills_dir(), false).len();
        report.backups = missing_entries(&dir.join("backups"), &backups_dir(), true).len();
        Ok(report)
    }

    /// Import everything from `dir` that MultiClaude does not have yet.
    pub fn legacy_import_apply(&self, dir: &Path) -> Result<LegacyImportReport, AppError> {
        let preview = self.legacy_import_preview(dir)?;
        if !preview.found || preview.nothing_new() {
            return Ok(preview);
        }
        let mut report = base_report(dir);
        report.backup_file = self
            .backup_database_file()?
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));

        {
            let mut conn = lock_conn!(self.conn);
            attach(&conn, &dir.join("cc-switch.db"))?;
            let result = import_rows(&mut conn, &mut report);
            detach(&conn);
            result?;
        }
        *lock_conn!(self.log_count_cache) = None;

        report.skill_folders = copy_missing(&dir.join("skills"), &skills_dir(), false)?;
        report.backups = copy_missing(&dir.join("backups"), &backups_dir(), true)?;
        log::info!("Imported CC Switch data from {}: {report:?}", dir.display());
        Ok(report)
    }
}

fn base_report(dir: &Path) -> LegacyImportReport {
    let db = dir.join("cc-switch.db");
    LegacyImportReport {
        found: db.is_file(),
        path: dir.display().to_string(),
        ..Default::default()
    }
}

/// The custom data directory may itself point at `~/.cc-switch`.
fn same_dir(dir: &Path) -> bool {
    let ours = crate::config::get_app_config_dir();
    match (fs::canonicalize(dir), fs::canonicalize(&ours)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn skills_dir() -> PathBuf {
    crate::config::get_app_config_dir().join("skills")
}

fn backups_dir() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("backups")
        .join("from-cc-switch")
}

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

fn attach(conn: &Connection, db: &Path) -> Result<(), AppError> {
    conn.execute(
        "ATTACH DATABASE ?1 AS legacy",
        params![db.to_string_lossy()],
    )
    .map_err(db_err)?;
    Ok(())
}

fn detach(conn: &Connection) {
    let _ = conn.execute_batch("DROP TABLE IF EXISTS temp.legacy_provider_map");
    if let Err(e) = conn.execute_batch("DETACH DATABASE legacy") {
        log::warn!("Detaching the CC Switch database failed: {e}");
    }
}

fn with_legacy_attached<T>(
    conn: &Connection,
    db: &Path,
    f: impl FnOnce(&Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    attach(conn, db)?;
    let result = f(conn);
    detach(conn);
    result
}

fn columns(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA {schema}.table_info(\"{table}\")"))
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

fn primary_key(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA main.table_info(\"{table}\")"))
        .map_err(db_err)?;
    let mut keyed = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(5)?, r.get::<_, String>(1)?)))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    keyed.retain(|(pk, _)| *pk > 0);
    keyed.sort();
    Ok(keyed.into_iter().map(|(_, name)| name).collect())
}

/// Columns both databases have; empty when either lacks the table.
fn shared_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
    let ours = columns(conn, "main", table)?;
    let theirs: HashSet<String> = columns(conn, "legacy", table)?.into_iter().collect();
    Ok(ours.into_iter().filter(|c| theirs.contains(c)).collect())
}

fn quote(col: &str) -> String {
    format!("\"{col}\"")
}

/// Rows of `legacy.table` whose primary key is not in `main.table`, after
/// mapping `provider_id` where the table has one.
fn missing_rows_query(
    conn: &Connection,
    table: &str,
) -> Result<Option<(Vec<String>, String)>, AppError> {
    let cols = shared_columns(conn, table)?;
    let key = primary_key(conn, table)?;
    if cols.is_empty() || key.is_empty() || !key.iter().all(|k| cols.contains(k)) {
        return Ok(None);
    }
    let mapped = PROVIDER_TABLES.contains(&table) && cols.iter().any(|c| c == "provider_id");
    let value = |c: &str| {
        if mapped && c == "provider_id" {
            "COALESCE(m.new_id, l.provider_id)".to_string()
        } else {
            format!("l.{}", quote(c))
        }
    };
    let select = cols.iter().map(|c| value(c)).collect::<Vec<_>>().join(", ");
    let join = if mapped {
        " LEFT JOIN temp.legacy_provider_map m ON m.old_id = l.provider_id AND m.app_type = l.app_type"
    } else {
        ""
    };
    let matches = key
        .iter()
        .map(|k| format!("o.{} IS {}", quote(k), value(k)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "SELECT {select} FROM legacy.\"{table}\" l{join} \
         WHERE NOT EXISTS (SELECT 1 FROM main.\"{table}\" o WHERE {matches})"
    );
    Ok(Some((cols, sql)))
}

fn count_missing(conn: &Connection, table: &str) -> Result<usize, AppError> {
    let Some((_, sql)) = missing_rows_query(conn, table)? else {
        return Ok(0);
    };
    let n: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM ({sql})"), [], |r| r.get(0))
        .map_err(db_err)?;
    Ok(n as usize)
}

fn report_rows(report: &mut LegacyImportReport, table: &str, n: usize) {
    match table {
        "proxy_request_logs" => report.request_logs += n,
        "usage_daily_rollups" => report.rollups += n,
        _ => report.other_rows += n,
    }
}

struct ProviderPlan {
    /// (old id, app type, id here) for every old provider.
    map: Vec<(String, String, String)>,
    /// (old id, app type, new id) for the providers to copy.
    copies: Vec<(String, String, String)>,
}

/// Maps each old provider onto a same-named one here, or picks the id it is
/// copied under. Also fills `temp.legacy_provider_map` for the row queries.
fn plan_providers(conn: &Connection) -> Result<ProviderPlan, AppError> {
    let load = |schema: &str| -> Result<Vec<(String, String, String)>, AppError> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, app_type, name FROM {schema}.providers"
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    };
    let ours = load("main")?;
    let mut taken: HashSet<(String, String)> = ours
        .iter()
        .map(|(id, app, _)| (id.clone(), app.clone()))
        .collect();
    let by_name: HashMap<(String, String), String> = ours
        .into_iter()
        .map(|(id, app, name)| ((app, name), id))
        .collect();

    let mut plan = ProviderPlan {
        map: Vec::new(),
        copies: Vec::new(),
    };
    for (id, app, name) in load("legacy")? {
        let target = match by_name.get(&(app.clone(), name)) {
            Some(existing) => existing.clone(),
            None => {
                let mut candidate = id.clone();
                let mut n = 1;
                while taken.contains(&(candidate.clone(), app.clone())) {
                    candidate = if n == 1 {
                        format!("{id}-cc-switch")
                    } else {
                        format!("{id}-cc-switch-{n}")
                    };
                    n += 1;
                }
                taken.insert((candidate.clone(), app.clone()));
                plan.copies
                    .push((id.clone(), app.clone(), candidate.clone()));
                candidate
            }
        };
        plan.map.push((id, app, target));
    }

    conn.execute_batch(
        "DROP TABLE IF EXISTS temp.legacy_provider_map;
         CREATE TEMP TABLE legacy_provider_map (old_id TEXT, app_type TEXT, new_id TEXT)",
    )
    .map_err(db_err)?;
    let mut insert = conn
        .prepare("INSERT INTO temp.legacy_provider_map VALUES (?1, ?2, ?3)")
        .map_err(db_err)?;
    for (old, app, new) in &plan.map {
        insert.execute(params![old, app, new]).map_err(db_err)?;
    }
    Ok(plan)
}

fn import_rows(conn: &mut Connection, report: &mut LegacyImportReport) -> Result<(), AppError> {
    let plan = plan_providers(conn)?;
    let tx = conn.transaction().map_err(db_err)?;

    let provider_cols = shared_columns(&tx, "providers")?;
    let endpoint_cols: Vec<String> = shared_columns(&tx, "provider_endpoints")?
        .into_iter()
        .filter(|c| c != "id")
        .collect();
    for (old, app, new) in &plan.copies {
        let select = provider_cols
            .iter()
            .map(|c| match c.as_str() {
                "id" => "?1".to_string(),
                "is_current" => "0".to_string(),
                "in_failover_queue" => "0".to_string(),
                other => quote(other),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let insert_cols = provider_cols
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(", ");
        tx.execute(
            &format!(
                "INSERT INTO main.providers ({insert_cols}) SELECT {select} \
                 FROM legacy.providers WHERE id = ?2 AND app_type = ?3"
            ),
            params![new, old, app],
        )
        .map_err(db_err)?;
        if !endpoint_cols.is_empty() {
            let select = endpoint_cols
                .iter()
                .map(|c| {
                    if c == "provider_id" {
                        "?1".to_string()
                    } else {
                        quote(c)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let insert_cols = endpoint_cols
                .iter()
                .map(|c| quote(c))
                .collect::<Vec<_>>()
                .join(", ");
            tx.execute(
                &format!(
                    "INSERT INTO main.provider_endpoints ({insert_cols}) SELECT {select} \
                     FROM legacy.provider_endpoints WHERE provider_id = ?2 AND app_type = ?3"
                ),
                params![new, old, app],
            )
            .map_err(db_err)?;
        }
    }
    report.providers = plan.copies.len();

    for table in PROVIDER_TABLES.iter().chain(PLAIN_TABLES) {
        let Some((cols, sql)) = missing_rows_query(&tx, table)? else {
            continue;
        };
        let insert_cols = cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
        let n = tx
            .execute(
                &format!("INSERT OR IGNORE INTO main.\"{table}\" ({insert_cols}) {sql}"),
                [],
            )
            .map_err(db_err)?;
        report_rows(report, table, n);
    }

    tx.commit().map_err(db_err)?;
    Ok(())
}

/// Entries of `from` missing in `to`: directories for skills, `.db` files for backups.
fn missing_entries(from: &Path, to: &Path, files: bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(from) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            if files {
                p.is_file() && p.extension().is_some_and(|x| x == "db")
            } else {
                p.is_dir()
            }
        })
        .filter(|p| p.file_name().is_some_and(|name| !to.join(name).exists()))
        .collect()
}

fn copy_missing(from: &Path, to: &Path, files: bool) -> Result<usize, AppError> {
    let missing = missing_entries(from, to, files);
    if missing.is_empty() {
        return Ok(0);
    }
    fs::create_dir_all(to).map_err(|e| AppError::io(to, e))?;
    for src in &missing {
        let Some(name) = src.file_name() else {
            continue;
        };
        let dst = to.join(name);
        if files {
            fs::copy(src, &dst).map_err(|e| AppError::io(&dst, e))?;
        } else {
            copy_dir(src, &dst)?;
        }
    }
    Ok(missing.len())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), AppError> {
    fs::create_dir_all(to).map_err(|e| AppError::io(to, e))?;
    for entry in fs::read_dir(from)
        .map_err(|e| AppError::io(from, e))?
        .flatten()
    {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| AppError::io(&dst, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(conn: &Connection, schema: &str) {
        conn.execute_batch(&format!(
            "CREATE TABLE {schema}.providers (id TEXT, app_type TEXT, name TEXT, settings_config TEXT,
                 is_current INTEGER DEFAULT 0, PRIMARY KEY (id, app_type));
             CREATE TABLE {schema}.provider_endpoints (id INTEGER PRIMARY KEY AUTOINCREMENT,
                 provider_id TEXT, app_type TEXT, url TEXT);
             CREATE TABLE {schema}.proxy_request_logs (request_id TEXT PRIMARY KEY, provider_id TEXT,
                 app_type TEXT, output_tokens INTEGER);
             CREATE TABLE {schema}.prompts (id TEXT, app_type TEXT, content TEXT,
                 PRIMARY KEY (id, app_type));"
        ))
        .unwrap();
    }

    #[test]
    fn imports_missing_rows_and_maps_providers_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join("legacy.db");
        {
            let legacy = Connection::open(&legacy_path).unwrap();
            seed(&legacy, "main");
            legacy
                .execute_batch(
                    "INSERT INTO providers VALUES
                        ('a1', 'claude', 'DeepSeek', '{\"key\":\"old\"}', 1),
                        ('shared', 'claude', 'Mistral', '{\"key\":\"m\"}', 0),
                        ('b1', 'claude', 'Kimi', '{\"key\":\"k\"}', 0);
                     INSERT INTO provider_endpoints (provider_id, app_type, url)
                        VALUES ('shared', 'claude', 'https://mistral');
                     INSERT INTO proxy_request_logs VALUES
                        ('r1', 'a1', 'claude', 5), ('r2', 'shared', 'claude', 7), ('r3', 'b1', 'claude', 9);
                     INSERT INTO prompts VALUES ('p', 'claude', 'old prompt');",
                )
                .unwrap();
        }

        let mut conn = Connection::open_in_memory().unwrap();
        seed(&conn, "main");
        conn.execute_batch(
            "INSERT INTO providers VALUES
                ('d9', 'claude', 'DeepSeek', '{\"key\":\"new\"}', 1),
                ('shared', 'claude', 'Official', '{}', 0);
             INSERT INTO proxy_request_logs VALUES ('r1', 'd9', 'claude', 5);
             INSERT INTO prompts VALUES ('p', 'claude', 'new prompt');",
        )
        .unwrap();

        attach(&conn, &legacy_path).unwrap();
        let mut report = LegacyImportReport::default();
        import_rows(&mut conn, &mut report).unwrap();
        detach(&conn);

        assert_eq!(report.providers, 2);
        assert_eq!(report.request_logs, 2);
        assert_eq!(report.other_rows, 0);

        let key: String = conn
            .query_row(
                "SELECT settings_config FROM providers WHERE id = 'd9'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            key, "{\"key\":\"new\"}",
            "the provider already here is kept"
        );
        let (name, current): (String, i64) = conn
            .query_row(
                "SELECT name, is_current FROM providers WHERE id = 'shared-cc-switch'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), current), ("Mistral", 0));
        let url: String = conn
            .query_row(
                "SELECT url FROM provider_endpoints WHERE provider_id = 'shared-cc-switch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(url, "https://mistral");
        let logs: Vec<(String, String)> = conn
            .prepare("SELECT request_id, provider_id FROM proxy_request_logs ORDER BY request_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            logs,
            vec![
                ("r1".into(), "d9".into()),
                ("r2".into(), "shared-cc-switch".into()),
                ("r3".into(), "b1".into()),
            ]
        );
        let prompt: String = conn
            .query_row("SELECT content FROM prompts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(prompt, "new prompt");

        // A second run finds nothing new.
        attach(&conn, &legacy_path).unwrap();
        let mut again = LegacyImportReport::default();
        import_rows(&mut conn, &mut again).unwrap();
        detach(&conn);
        assert_eq!((again.providers, again.request_logs), (0, 0));
    }
}
