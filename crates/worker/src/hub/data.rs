use moni_protocol::{
    MinuteRollup,
    api::{History, HistoryPoint, Traffic},
};
use serde::Deserialize;
use worker::*;

use super::Hub;
use crate::{
    auth::now_ms,
    error,
    logic::{self, HOUR_MS},
    var_u32,
};

// COALESCE keeps migrated PoC NULLs compatible with the non-null API contract.
const COLUMNS: &str = "COALESCE(cpu_avg,0) AS cpu_avg, COALESCE(cpu_max,0) AS cpu_max,
    COALESCE(mem_pct,0) AS mem_pct, COALESCE(swap_pct,0) AS swap_pct,
    COALESCE(disk_pct,0) AS disk_pct, COALESCE(rx_bytes,0) AS rx_bytes,
    COALESCE(tx_bytes,0) AS tx_bytes, COALESCE(rx_bps_max,0) AS rx_bps_max,
    COALESCE(tx_bps_max,0) AS tx_bps_max, COALESCE(load1,0) AS load1";
const AGGREGATES: &str = "COALESCE(AVG(cpu_avg),0) AS cpu_avg, COALESCE(MAX(cpu_max),0) AS cpu_max,
    COALESCE(AVG(mem_pct),0) AS mem_pct, COALESCE(AVG(swap_pct),0) AS swap_pct,
    COALESCE(AVG(disk_pct),0) AS disk_pct, COALESCE(SUM(rx_bytes),0) AS rx_bytes,
    COALESCE(SUM(tx_bytes),0) AS tx_bytes, COALESCE(MAX(rx_bps_max),0) AS rx_bps_max,
    COALESCE(MAX(tx_bps_max),0) AS tx_bps_max, COALESCE(AVG(load1),0) AS load1";

#[derive(Deserialize)]
struct MetaRow {
    v: String,
}
#[derive(Deserialize)]
struct IdRow {
    id: String,
}
#[derive(Deserialize)]
struct DirtyHour {
    server_id: String,
    hour_ms: i64,
}
#[derive(Deserialize)]
struct Bytes {
    rx_bytes: u64,
    tx_bytes: u64,
}

impl Hub {
    fn last_hourly_ms(&self) -> Result<i64> {
        let rows = self
            .state
            .storage()
            .sql()
            .exec("SELECT v FROM meta WHERE k = 'last_hourly_ms'", None)?
            .to_array::<MetaRow>()?;
        match rows.first() {
            Some(row) => row
                .v
                .parse()
                .map_err(|_| Error::RustError("Invalid last_hourly_ms".into())),
            None => Ok(0),
        }
    }

    pub(super) fn store_rollup(&self, id: &str, rollup: &MinuteRollup) -> Result<()> {
        let now = now_ms();
        if !logic::reasonable_minute(
            rollup.minute_ms,
            now,
            var_u32(&self.env, "RETENTION_MINUTE_DAYS", 7),
        ) {
            return Ok(());
        }
        let sql = self.state.storage().sql();
        sql.exec(
            "INSERT OR REPLACE INTO metrics_1m
            (server_id, ts_ms, cpu_avg, cpu_max, mem_pct, swap_pct, disk_pct,
             rx_bytes, tx_bytes, rx_bps_max, tx_bps_max, load1)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                id.into(),
                rollup.minute_ms.into(),
                f64::from(rollup.cpu_avg).into(),
                f64::from(rollup.cpu_max).into(),
                logic::percentage(rollup.mem_used_avg, rollup.mem_total).into(),
                logic::percentage(rollup.swap_used_avg, rollup.swap_total).into(),
                logic::percentage(rollup.disk_used, rollup.disk_total).into(),
                (rollup.rx_bytes as f64).into(),
                (rollup.tx_bytes as f64).into(),
                (rollup.rx_bps_max as f64).into(),
                (rollup.tx_bps_max as f64).into(),
                f64::from(rollup.load1_avg).into(),
            ],
        )?;
        let hour = logic::bucket(rollup.minute_ms, HOUR_MS);
        // Backfills arriving before the first maintenance must survive its two-hour window too.
        if hour
            < self
                .last_hourly_ms()?
                .max(logic::bucket(now, HOUR_MS) - 2 * HOUR_MS)
        {
            sql.exec(
                "INSERT OR IGNORE INTO dirty_hours (server_id, hour_ms) VALUES (?, ?)",
                vec![id.into(), hour.into()],
            )?;
        }
        Ok(())
    }

    pub(super) fn history(&self, id: &str, req: &Request) -> Result<Response> {
        let url = req.url()?;
        let range = url
            .query_pairs()
            .find(|(key, _)| key == "range")
            .map(|(_, value)| value.into_owned());
        let Some(plan) = logic::history_plan(range.as_deref()) else {
            return error::bad_request("Unknown history range");
        };
        if self.server_row(id)?.is_none() {
            return error::not_found();
        }
        let now = now_ms();
        let query = if plan.step_ms == 600_000 {
            format!(
                "SELECT (ts_ms/600000)*600000 AS ts_ms, {AGGREGATES}
                FROM metrics_1m WHERE server_id = ? AND ts_ms >= ? AND ts_ms <= ?
                GROUP BY ts_ms/600000 ORDER BY ts_ms"
            )
        } else {
            format!(
                "SELECT ts_ms, {COLUMNS} FROM {} WHERE server_id = ? AND ts_ms >= ? AND ts_ms <= ? ORDER BY ts_ms",
                plan.table
            )
        };
        let points = self
            .state
            .storage()
            .sql()
            .exec(
                &query,
                vec![id.into(), (now - plan.duration_ms).into(), now.into()],
            )?
            .to_array::<HistoryPoint>()?;
        Response::from_json(&History {
            range: plan.range,
            step_ms: plan.step_ms as u64,
            points,
        })
    }

    pub(super) fn traffic_month(&self, id: &str) -> Result<Traffic> {
        let now = now_ms();
        let since_ms = logic::month_start(now);
        let boundary = self.last_hourly_ms()?.max(since_ms);
        let sql = self.state.storage().sql();
        let hourly = sql
            .exec(
                "SELECT COALESCE(SUM(rx_bytes),0) AS rx_bytes,
            COALESCE(SUM(tx_bytes),0) AS tx_bytes FROM metrics_1h
            WHERE server_id = ? AND ts_ms >= ? AND ts_ms < ?",
                vec![id.into(), since_ms.into(), boundary.into()],
            )?
            .one::<Bytes>()?;
        let minutes = sql
            .exec(
                "SELECT COALESCE(SUM(rx_bytes),0) AS rx_bytes,
            COALESCE(SUM(tx_bytes),0) AS tx_bytes FROM metrics_1m
            WHERE server_id = ? AND ts_ms >= ? AND ts_ms <= ?",
                vec![id.into(), boundary.into(), now.into()],
            )?
            .one::<Bytes>()?;
        Ok(Traffic {
            rx_bytes: hourly.rx_bytes.saturating_add(minutes.rx_bytes),
            tx_bytes: hourly.tx_bytes.saturating_add(minutes.tx_bytes),
            since_ms,
        })
    }

    fn aggregate_hour(&self, id: &str, hour: i64) -> Result<()> {
        self.state.storage().sql().exec(
            &format!(
                "INSERT OR REPLACE INTO metrics_1h
            SELECT server_id, ? AS ts_ms, {AGGREGATES} FROM metrics_1m
            WHERE server_id = ? AND ts_ms >= ? AND ts_ms < ? GROUP BY server_id"
            ),
            vec![hour.into(), id.into(), hour.into(), (hour + HOUR_MS).into()],
        )?;
        Ok(())
    }

    pub(super) fn maintain(&self, now: i64) -> Result<()> {
        let hour = logic::bucket(now, HOUR_MS);
        let last_hourly_ms = self.last_hourly_ms()?;
        if last_hourly_ms >= hour {
            return Ok(());
        }
        let sql = self.state.storage().sql();
        let servers = sql
            .exec("SELECT id FROM servers", None)?
            .to_array::<IdRow>()?;
        let dirty = sql
            .exec("SELECT server_id, hour_ms FROM dirty_hours", None)?
            .to_array::<DirtyHour>()?;
        // After missed alarms, cover every retained hour before advancing the shared boundary.
        let start = logic::hourly_start(
            last_hourly_ms,
            now,
            var_u32(&self.env, "RETENTION_MINUTE_DAYS", 7),
        );
        for server in &servers {
            for completed in (start..hour).step_by(HOUR_MS as usize) {
                self.aggregate_hour(&server.id, completed)?;
            }
        }
        for item in dirty {
            // Hours covered by the regular pass need no second rebuild.
            if item.hour_ms < start {
                self.aggregate_hour(&item.server_id, item.hour_ms)?;
            }
            sql.exec(
                "DELETE FROM dirty_hours WHERE server_id = ? AND hour_ms = ?",
                vec![item.server_id.into(), item.hour_ms.into()],
            )?;
        }
        let minute_cutoff = logic::bucket(
            now - i64::from(var_u32(&self.env, "RETENTION_MINUTE_DAYS", 7)) * logic::DAY_MS,
            HOUR_MS,
        );
        let hour_cutoff =
            now - i64::from(var_u32(&self.env, "RETENTION_HOUR_DAYS", 90)) * logic::DAY_MS;
        for server in servers {
            // Keep the oldest hour whole so a late rollup cannot rebuild it from truncated minutes.
            sql.exec(
                "DELETE FROM metrics_1m WHERE server_id = ? AND ts_ms < ?",
                vec![server.id.as_str().into(), minute_cutoff.into()],
            )?;
            sql.exec(
                "DELETE FROM metrics_1h WHERE server_id = ? AND ts_ms < ?",
                vec![server.id.into(), hour_cutoff.into()],
            )?;
        }
        sql.exec(
            "INSERT INTO meta (k,v) VALUES ('last_hourly_ms', ?)
            ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            vec![hour.to_string().into()],
        )?;
        Ok(())
    }
}
