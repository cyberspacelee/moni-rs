use serde::Deserialize;
use worker::{
    Result, SqlStorage, Storage, js_sys,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

const VERSION: &str = "5";

#[derive(Deserialize)]
struct Version {
    v: String,
}

fn unsupported() -> worker::Error {
    worker::Error::RustError("Unsupported schema version".into())
}

pub(super) fn ready(sql: &SqlStorage) -> Result<()> {
    let versions = sql
        .exec("SELECT v FROM meta WHERE k = 'schema_version'", None)?
        .to_array::<Version>()?;
    if versions.first().is_none_or(|version| version.v != VERSION) {
        return Err(unsupported());
    }
    Ok(())
}

pub(super) fn migrate(storage: &Storage) -> Result<()> {
    let raw = storage.as_raw();
    let transaction = js_sys::Reflect::get(raw, &JsValue::from_str("transactionSync"))?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| {
            worker::Error::RustError("Synchronous storage transactions unavailable".into())
        })?;
    let sql = storage.sql();
    // Throw across the JS callback so a caught migration error cannot commit partial initialization.
    let callback =
        Closure::once_assert_unwind_safe(move || migrate_schema(&sql).map_err(JsValue::from));
    transaction.call1(raw, callback.as_ref())?;
    Ok(())
}

fn migrate_schema(sql: &SqlStorage) -> Result<()> {
    #[derive(Deserialize)]
    struct Layout {
        tables: i64,
        has_meta: i64,
    }
    let layout = sql
        .exec(
            "SELECT COUNT(*) AS tables, COALESCE(SUM(name = 'meta'),0) AS has_meta
             FROM sqlite_master WHERE type = 'table'
             AND name NOT GLOB '_cf_*' AND name NOT GLOB '__cf_*' AND name NOT GLOB 'sqlite_*'",
            None,
        )?
        .one::<Layout>()?;
    let fresh = layout.tables == 0;
    if !fresh {
        if layout.has_meta == 0 {
            return Err(unsupported());
        }
        let versions = sql
            .exec("SELECT v FROM meta WHERE k = 'schema_version'", None)?
            .to_array::<Version>()?;
        match versions.first().map(|version| version.v.as_str()) {
            Some(VERSION) => return Ok(()),
            Some("4") => {}
            _ => return Err(unsupported()),
        }
    }
    if fresh {
        sql.exec(
            "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT);
             CREATE TABLE IF NOT EXISTS servers (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL,
                created_ms INTEGER NOT NULL, last_seen_ms INTEGER, country TEXT,
                host_json TEXT, agent_version TEXT
             )",
            None,
        )?;
        for table in ["metrics_1m", "metrics_1h"] {
            sql.exec(
                &format!(
                    "CREATE TABLE IF NOT EXISTS {table} (
                        server_id TEXT NOT NULL, ts_ms INTEGER NOT NULL,
                        cpu_avg REAL, cpu_max REAL, mem_pct REAL, swap_pct REAL, disk_pct REAL,
                        rx_bytes INTEGER, tx_bytes INTEGER, rx_bps_max INTEGER, tx_bps_max INTEGER, load1 REAL,
                        PRIMARY KEY (server_id, ts_ms)
                     ) WITHOUT ROWID"
                ),
                None,
            )?;
        }
        sql.exec(
            "CREATE TABLE IF NOT EXISTS dirty_hours (
                server_id TEXT, hour_ms INTEGER, PRIMARY KEY (server_id, hour_ms)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS alert_rules (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, server_id TEXT,
                metric TEXT NOT NULL, op TEXT NOT NULL, threshold REAL NOT NULL, recover_threshold REAL,
                duration_s INTEGER NOT NULL, enabled INTEGER NOT NULL, created_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS alert_state (
                server_id TEXT, rule_id TEXT, since_ms INTEGER, value REAL,
                PRIMARY KEY (server_id, rule_id)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS alert_events (
                id INTEGER PRIMARY KEY, rule_id TEXT, server_id TEXT, kind TEXT,
                value REAL, ts_ms INTEGER, rule_name TEXT, server_name TEXT
             );
             CREATE TABLE IF NOT EXISTS notification_queue (
                id INTEGER PRIMARY KEY, server_id TEXT NOT NULL, notification_json TEXT NOT NULL
             )",
            None,
        )?;
        for (id, name, metric, duration) in [
            ("default-offline", "服务器离线", "offline", 60),
            ("default-cpu", "CPU 使用率过高", "cpu", 300),
            ("default-mem", "内存使用率过高", "mem", 180),
            ("default-disk", "磁盘使用率过高", "disk", 0),
        ] {
            sql.exec(
                "INSERT INTO alert_rules
                 (id,name,server_id,metric,op,threshold,recover_threshold,duration_s,enabled,created_ms)
                 VALUES (?,?,NULL,?,'gt',90,NULL,?,1,?)",
                vec![
                    id.into(),
                    name.into(),
                    metric.into(),
                    duration.into(),
                    crate::auth::now_ms().into(),
                ],
            )?;
        }
    }
    sql.exec(
        "CREATE TABLE IF NOT EXISTS agent_updates (
            server_id TEXT PRIMARY KEY, job_id TEXT NOT NULL, repo TEXT NOT NULL,
            version TEXT NOT NULL, state TEXT NOT NULL, error TEXT,
            requested_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL
         ) WITHOUT ROWID",
        None,
    )?;
    sql.exec(
        "INSERT INTO meta (k,v) VALUES ('schema_version','5')
         ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        None,
    )?;
    Ok(())
}
