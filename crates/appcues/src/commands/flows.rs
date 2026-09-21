use super::composite::{Period, pct_change, rate_pct, round1};
use super::{Ctx, as_list, emit_meta};
use crate::output::{Format, render_item, render_list};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::SystemTime;

pub fn list(ctx: &Ctx) -> Result<String> {
    Ok(render_list(
        &fetch_flows(ctx)?,
        ctx.format,
        &["id", "name", "published", "updated_at"],
    ))
}

fn fetch_flows(ctx: &Ctx) -> Result<Vec<Value>> {
    Ok(as_list(ctx.client.get(&ctx.path("flows"))?))
}

pub fn get(ctx: &Ctx, flow_id: &str) -> Result<String> {
    let v = ctx.client.get(&ctx.path(&format!("flows/{flow_id}")))?;
    Ok(render_item(&v, ctx.format))
}

pub fn publish(ctx: &Ctx, flow_id: &str) -> Result<String> {
    let path = ctx.path(&format!("flows/{flow_id}/publish"));
    if let Some(msg) = ctx.dry_run_output("POST", &path, None) {
        return Ok(msg);
    }
    ctx.client.post(&path, None)?;
    Ok(format!("Published flow {flow_id}"))
}

pub fn unpublish(ctx: &Ctx, flow_id: &str) -> Result<String> {
    let path = ctx.path(&format!("flows/{flow_id}/unpublish"));
    if let Some(msg) = ctx.dry_run_output("POST", &path, None) {
        return Ok(msg);
    }
    ctx.client.post(&path, None)?;
    Ok(format!("Unpublished flow {flow_id}"))
}

/// Events the digest aggregates, from the canonical vocabulary
/// (Domaincues.Constants.Events). There is no flow-level error event;
/// errors are step-level, grouped here by flow_id. The server does not
/// validate condition values — a wrong name means zero rows, not a 400.
const FLOW_EVENTS: [&str; 4] = [
    "appcues:flow_started",
    "appcues:flow_completed",
    "appcues:flow_skipped",
    "appcues:step_error",
];

fn digest_spec(period: &Period) -> Value {
    json!({
        "metrics": ["events", "users"],
        "dimensions": ["flow_id", "name"],
        "conditions": [["name", "in", FLOW_EVENTS]],
        "start_time": period.start_str(),
        "end_time": period.end_str(),
    })
}

#[derive(Default, Clone, Copy)]
struct Counts {
    shown: u64,
    completed: u64,
    skipped: u64,
    errors: u64,
    unique_users: u64,
}

impl Counts {
    fn completion_rate(&self) -> Option<f64> {
        rate_pct(self.completed, self.shown)
    }
}

/// Fold (flow_id, event name) aggregate rows into per-flow counters.
fn tally(rows: &[Value]) -> HashMap<String, Counts> {
    let mut map: HashMap<String, Counts> = HashMap::new();
    for r in rows {
        let Some(flow_id) = r["flow_id"].as_str() else {
            continue;
        };
        let events = r["events"].as_u64().unwrap_or(0);
        let c = map.entry(flow_id.to_string()).or_default();
        match r["name"].as_str().unwrap_or("") {
            "appcues:flow_started" => {
                c.shown = events;
                c.unique_users = r["users"].as_u64().unwrap_or(0);
            }
            "appcues:flow_completed" => c.completed = events,
            "appcues:flow_skipped" => c.skipped = events,
            "appcues:step_error" => c.errors = events,
            _ => {}
        }
    }
    map
}

fn opt(v: Option<f64>) -> Value {
    v.map_or(Value::Null, Value::from)
}

fn digest_row(flow: &Value, cur: &Counts, prev: &Counts) -> Value {
    let rate = cur.completion_rate();
    let prev_rate = prev.completion_rate();
    let rate_pts = match (rate, prev_rate) {
        (Some(a), Some(b)) => Some(round1(a - b)),
        _ => None,
    };
    json!({
        "flow_id": flow["id"],
        "name": flow["name"],
        "shown": cur.shown,
        "completed": cur.completed,
        "skipped": cur.skipped,
        "errors": cur.errors,
        "unique_users": cur.unique_users,
        "completion_rate": opt(rate),
        "previous": {
            "shown": prev.shown,
            "completed": prev.completed,
            "skipped": prev.skipped,
            "errors": prev.errors,
            "unique_users": prev.unique_users,
            "completion_rate": opt(prev_rate),
        },
        "deltas": {
            "shown_pct": opt(pct_change(cur.shown as f64, prev.shown as f64)),
            "completion_rate_pts": opt(rate_pts),
            "unique_users_pct": opt(pct_change(
                cur.unique_users as f64,
                prev.unique_users as f64,
            )),
        },
    })
}

/// `flows +digest`: published flows' performance over the last `days`,
/// with deltas against the same-length window immediately before.
/// Composes GET flows + two analytics queries client-side.
// ponytail: sequential calls (3 requests behind the 20ms throttle);
// parallelize only if a composite ever fans out to dozens of calls.
pub fn digest(ctx: &Ctx, days: u32, now: SystemTime) -> Result<String> {
    let period = Period::last_days(days, now);
    let prev = period.previous();
    let query_path = ctx.path("analytics/query");
    let cur_spec = digest_spec(&period);
    let prev_spec = digest_spec(&prev);

    if let Some(msg) = ctx.dry_run_outputs(&[
        ("POST", &query_path, &cur_spec),
        ("POST", &query_path, &prev_spec),
    ]) {
        return Ok(msg);
    }

    let flows = fetch_flows(ctx)?;
    let cur_res = ctx.client.post_with_meta(&query_path, Some(&cur_spec))?;
    emit_meta(&cur_res);
    let prev_res = ctx.client.post_with_meta(&query_path, Some(&prev_spec))?;
    emit_meta(&prev_res);
    let cur = tally(&as_list(cur_res.value));
    let old = tally(&as_list(prev_res.value));

    let zero = Counts::default();
    let mut rows: Vec<Value> = flows
        .iter()
        .filter(|f| f["published"] == Value::Bool(true))
        .map(|f| {
            let id = f["id"].as_str().unwrap_or("");
            digest_row(
                f,
                cur.get(id).unwrap_or(&zero),
                old.get(id).unwrap_or(&zero),
            )
        })
        .collect();
    // completion_rate desc, null rates last
    rows.sort_by(|a, b| {
        let key = |v: &Value| v["completion_rate"].as_f64();
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let envelope = json!({
        "period": {"start_time": period.start_str(), "end_time": period.end_str()},
        "previous_period": {"start_time": prev.start_str(), "end_time": prev.end_str()},
        "flows": rows,
    });
    Ok(match ctx.format {
        Format::Json => {
            serde_json::to_string_pretty(&envelope).expect("serializing Value never fails")
        }
        Format::Table => render_digest_table(&envelope),
    })
}

/// Table mode: one period line, then flat columns (deltas surfaced as a
/// top-level column because render_list reads only top-level keys).
fn render_digest_table(envelope: &Value) -> String {
    let flat: Vec<Value> = envelope["flows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let mut row = r.clone();
                    row["rate_delta_pts"] = r["deltas"]["completion_rate_pts"].clone();
                    row
                })
                .collect()
        })
        .unwrap_or_default();
    format!(
        "{} → {} (vs previous period)\n{}",
        envelope["period"]["start_time"].as_str().unwrap_or(""),
        envelope["period"]["end_time"].as_str().unwrap_or(""),
        render_list(
            &flat,
            Format::Table,
            &[
                "flow_id",
                "name",
                "shown",
                "completed",
                "completion_rate",
                "rate_delta_pts",
                "unique_users",
            ],
        )
    )
}
