use super::composite::{Period, pct_change};
use super::{Ctx, as_list, emit_meta};
use crate::output::{Format, render_item, render_list};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::time::SystemTime;

/// Load a spec from a file path, or from stdin when the arg is "-".
/// JSON well-formedness is the only check the CLI does: the server's
/// normalize layer is the single semantic validator, so the CLI can
/// never drift from the contract.
pub fn load_spec(arg: &str) -> Result<Value> {
    load_spec_from(arg, &mut std::io::stdin().lock())
}

fn load_spec_from(arg: &str, stdin: &mut dyn std::io::Read) -> Result<Value> {
    let text = if arg == "-" {
        let mut s = String::new();
        stdin
            .read_to_string(&mut s)
            .context("failed to read spec from stdin")?;
        s
    } else {
        std::fs::read_to_string(arg).with_context(|| format!("cannot read spec file {arg}"))?
    };
    serde_json::from_str(&text).map_err(|e| anyhow!("spec is not valid JSON: {e}"))
}

/// POST the spec verbatim: sync to analytics/query (rows inline), async
/// to analytics/exports (202 + job_id). account_id lives only in the
/// route path; the server ignores any body value.
pub fn query(ctx: &Ctx, spec: &Value, submit_async: bool) -> Result<String> {
    let path = if submit_async {
        ctx.path("analytics/exports")
    } else {
        ctx.path("analytics/query")
    };
    if let Some(msg) = ctx.dry_run_output("POST", &path, Some(spec)) {
        return Ok(msg);
    }
    let res = ctx.client.post_with_meta(&path, Some(spec))?;
    emit_meta(&res);
    Ok(render_item(&res.value, ctx.format))
}

/// `analytics +compare`: run one aggregate spec over the last `days` and
/// over the same-length window before it, then join the two row sets on
/// their dimension values and emit per-metric deltas. The spec's own
/// `start_time`/`end_time` are replaced by the derived windows.
// ponytail: sequential calls (2 requests); same reasoning as flows::digest.
pub fn compare(ctx: &Ctx, spec: &Value, days: u32, now: SystemTime) -> Result<String> {
    let metrics: Vec<String> = spec["metrics"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if metrics.is_empty() {
        bail!(
            "+compare needs an aggregate spec with a non-empty `metrics` array (raw `columns` specs have nothing to compare)"
        );
    }
    let period = Period::last_days(days, now);
    let prev = period.previous();
    let path = ctx.path("analytics/query");
    let cur_spec = windowed(spec, &period);
    let prev_spec = windowed(spec, &prev);

    if let Some(msg) =
        ctx.dry_run_outputs(&[("POST", &path, &cur_spec), ("POST", &path, &prev_spec)])
    {
        return Ok(msg);
    }

    let cur_res = ctx.client.post_with_meta(&path, Some(&cur_spec))?;
    emit_meta(&cur_res);
    let prev_res = ctx.client.post_with_meta(&path, Some(&prev_spec))?;
    emit_meta(&prev_res);

    let rows = join_rows(&as_list(cur_res.value), &as_list(prev_res.value), &metrics);
    let envelope = json!({
        "period": {"start_time": period.start_str(), "end_time": period.end_str()},
        "previous_period": {"start_time": prev.start_str(), "end_time": prev.end_str()},
        "metrics": metrics,
        "rows": rows,
    });
    Ok(match ctx.format {
        Format::Json => {
            serde_json::to_string_pretty(&envelope).expect("serializing Value never fails")
        }
        Format::Table => render_compare_table(&envelope),
    })
}

fn windowed(spec: &Value, period: &Period) -> Value {
    let mut s = spec.clone();
    s["start_time"] = Value::from(period.start_str());
    s["end_time"] = Value::from(period.end_str());
    s
}

/// Everything in a row that is not a metric is a dimension value; the
/// dimension values are the join key. Rows missing from one window count
/// as zero there. Sorted by the first metric's current value, descending.
fn join_rows(cur: &[Value], prev: &[Value], metrics: &[String]) -> Vec<Value> {
    let split = |row: &Value| -> (Map<String, Value>, Map<String, Value>) {
        let mut dims = Map::new();
        let mut vals = Map::new();
        if let Some(obj) = row.as_object() {
            for (k, v) in obj {
                if metrics.iter().any(|m| m == k) {
                    vals.insert(k.clone(), v.clone());
                } else {
                    dims.insert(k.clone(), v.clone());
                }
            }
        }
        (dims, vals)
    };
    // (dims, current metric values, previous metric values), keyed by the
    // serialized dims: deterministic order, no hashing of Value.
    type Joined = (Map<String, Value>, Map<String, Value>, Map<String, Value>);
    let mut joined: BTreeMap<String, Joined> = BTreeMap::new();
    for r in cur {
        let (dims, vals) = split(r);
        let key = Value::Object(dims.clone()).to_string();
        joined
            .entry(key)
            .or_insert((dims, Map::new(), Map::new()))
            .1 = vals;
    }
    for r in prev {
        let (dims, vals) = split(r);
        let key = Value::Object(dims.clone()).to_string();
        joined
            .entry(key)
            .or_insert((dims, Map::new(), Map::new()))
            .2 = vals;
    }
    let num = |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let mut rows: Vec<Value> = joined
        .into_values()
        .map(|(dims, c, p)| {
            let mut row = dims;
            let mut current = Map::new();
            let mut previous = Map::new();
            let mut deltas = Map::new();
            for m in metrics {
                deltas.insert(
                    format!("{m}_pct"),
                    pct_change(num(&c, m), num(&p, m)).map_or(Value::Null, Value::from),
                );
                current.insert(m.clone(), c.get(m).cloned().unwrap_or(Value::from(0)));
                previous.insert(m.clone(), p.get(m).cloned().unwrap_or(Value::from(0)));
            }
            row.insert("current".into(), Value::Object(current));
            row.insert("previous".into(), Value::Object(previous));
            row.insert("deltas".into(), Value::Object(deltas));
            Value::Object(row)
        })
        .collect();
    let first = &metrics[0];
    rows.sort_by(|a, b| {
        let key = |v: &Value| v["current"][first].as_f64().unwrap_or(0.0);
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

/// Table mode: one period line, then dims followed by `m`, `m_prev`,
/// `m_pct` per metric (render_list reads only top-level keys).
fn render_compare_table(envelope: &Value) -> String {
    let metrics: Vec<&str> = envelope["metrics"]
        .as_array()
        .map(|m| m.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let rows = envelope["rows"].as_array().cloned().unwrap_or_default();
    let mut columns: Vec<String> = rows
        .first()
        .and_then(Value::as_object)
        .map(|o| {
            o.keys()
                .filter(|k| !matches!(k.as_str(), "current" | "previous" | "deltas"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    for m in &metrics {
        columns.extend([m.to_string(), format!("{m}_prev"), format!("{m}_pct")]);
    }
    let flat: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut row = r.clone();
            for m in &metrics {
                row[*m] = r["current"][*m].clone();
                row[format!("{m}_prev")] = r["previous"][*m].clone();
                row[format!("{m}_pct")] = r["deltas"][format!("{m}_pct")].clone();
            }
            row
        })
        .collect();
    let cols: Vec<&str> = columns.iter().map(String::as_str).collect();
    format!(
        "{} → {} (vs previous period)\n{}",
        envelope["period"]["start_time"].as_str().unwrap_or(""),
        envelope["period"]["end_time"].as_str().unwrap_or(""),
        render_list(&flat, Format::Table, &cols)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_spec_reads_stdin_when_dash() {
        let mut input = r#"{"metrics":["nps_computed_score"]}"#.as_bytes();
        let spec = load_spec_from("-", &mut input).unwrap();
        assert_eq!(spec["metrics"][0], "nps_computed_score");
    }

    #[test]
    fn load_spec_rejects_invalid_stdin_json() {
        let mut input = "not json".as_bytes();
        let err = load_spec_from("-", &mut input).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
    }
}
