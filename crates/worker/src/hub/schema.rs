use serde::Deserialize;
use worker::{Result, SqlStorage};

const VERSION: &str = "2";

#[derive(Deserialize)]
struct Column {
    name: String,
}

fn columns(sql: &SqlStorage, table: &str) -> Result<Vec<Column>> {
    sql.exec(&format!("PRAGMA table_info({table})"), None)?
        .to_array()
}

pub(super) fn ready(sql: &SqlStorage) -> Result<()> {
    #[derive(Deserialize)]
    struct Version {
        v: String,
    }
    let version = sql
        .exec("SELECT v FROM meta WHERE k = 'schema_version'", None)?
        .one::<Version>()?;
    if version.v != VERSION {
        return Err(worker::Error::RustError(
            "Unsupported schema version".into(),
        ));
    }
    Ok(())
}

pub(super) fn migrate(sql: &SqlStorage) -> Result<()> {
    let fresh = columns(sql, "meta")?.is_empty() && columns(sql, "servers")?.is_empty();
    sql.exec(
        "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT)",
        None,
    )?;
    #[derive(Deserialize)]
    struct Version {
        v: String,
    }
    let version = sql
        .exec("SELECT v FROM meta WHERE k = 'schema_version'", None)?
        .to_array::<Version>()?;
    match version.first().map(|v| v.v.as_str()) {
        Some(VERSION) => return Ok(()),
        Some("1") => return migrate_alerts(sql, false),
        Some("0") | None => {}
        Some(_) => {
            return Err(worker::Error::RustError(
                "Unsupported schema version".into(),
            ));
        }
    }

    sql.exec(
        "CREATE TABLE IF NOT EXISTS servers (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL,
        created_ms INTEGER NOT NULL, last_seen_ms INTEGER, host_json TEXT, agent_version TEXT
    )",
        None,
    )?;
    let server_columns = columns(sql, "servers")?;
    if !server_columns.iter().any(|c| c.name == "name") {
        sql.exec(
            "ALTER TABLE servers ADD COLUMN name TEXT NOT NULL DEFAULT '';
                  UPDATE servers SET name = id",
            None,
        )?;
    }
    if !server_columns.iter().any(|c| c.name == "token_hash") {
        // PoC shared credentials cannot become per-server credentials; rotate them in the UI.
        sql.exec(
            "ALTER TABLE servers ADD COLUMN token_hash TEXT NOT NULL DEFAULT ''",
            None,
        )?;
    }
    if !server_columns.iter().any(|c| c.name == "agent_version") {
        sql.exec("ALTER TABLE servers ADD COLUMN agent_version TEXT", None)?;
    }
    if columns(sql, "metrics_1m")?.iter().any(|c| c.name == "ts") {
        sql.exec("ALTER TABLE metrics_1m RENAME TO metrics_poc", None)?;
    }
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
    if !columns(sql, "metrics_poc")?.is_empty() {
        // PoC stored seconds and rates, so byte totals and swap usage remain unknown (NULL).
        sql.exec("INSERT OR IGNORE INTO metrics_1m
            (server_id, ts_ms, cpu_avg, cpu_max, mem_pct, disk_pct, rx_bps_max, tx_bps_max, load1)
            SELECT server_id, ts * 1000, cpu_avg, cpu_max, mem_pct, disk_pct, rx_bps_max, tx_bps_max, load1
            FROM metrics_poc;
            DROP TABLE metrics_poc", None)?;
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
        rule_id TEXT, server_id TEXT, since_ms INTEGER, value REAL,
        PRIMARY KEY (rule_id, server_id)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS alert_events (
        id INTEGER PRIMARY KEY, rule_id TEXT, server_id TEXT, kind TEXT, value REAL, ts_ms INTEGER
    );
    DROP TABLE IF EXISTS poc_meta;
    INSERT INTO meta (k, v) VALUES ('schema_version', '1')
      ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        None,
    )?;
    migrate_alerts(sql, fresh)
}

fn migrate_alerts(sql: &SqlStorage, fresh: bool) -> Result<()> {
    let existing = columns(sql, "alert_events")?;
    for (name, definition) in [
        ("rule_name", "TEXT"),
        ("server_name", "TEXT"),
        ("notification_json", "TEXT"),
    ] {
        if !existing.iter().any(|column| column.name == name) {
            sql.exec(
                &format!("ALTER TABLE alert_events ADD COLUMN {name} {definition}"),
                None,
            )?;
        }
    }
    if fresh {
        for (id, name, metric, duration) in [
            ("default-offline", "服务器离线", "offline", 60),
            ("default-cpu", "CPU 使用率过高", "cpu", 300),
            ("default-mem", "内存使用率过高", "mem", 180),
            ("default-disk", "磁盘使用率过高", "disk", 0),
        ] {
            sql.exec("INSERT OR IGNORE INTO alert_rules
                (id,name,server_id,metric,op,threshold,recover_threshold,duration_s,enabled,created_ms)
                VALUES (?,?,NULL,?,'gt',90,NULL,?,1,?)",
                vec![id.into(),name.into(),metric.into(),duration.into(),crate::auth::now_ms().into()])?;
        }
    }
    sql.exec("UPDATE meta SET v = '2' WHERE k = 'schema_version'", None)?;
    Ok(())
}
