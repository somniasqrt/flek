//! M6.6: run with `cargo bench -p flek-core --bench budgets` (no live services).
mod support;

use std::{hint::black_box, time::Instant};

use flek_core::{
    rail,
    store::{directory, emoji, messages, search, Store},
    Result,
};
use serde::Serialize;
use support::{Fixture, CHANNEL, TEAM};

#[derive(Serialize)]
struct Measurement {
    name: String,
    samples: usize,
    rows: usize,
    first_ms: f64,
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    budget_ms: Option<f64>,
    p95_within_budget: Option<bool>,
}

fn measure(
    name: &str,
    samples: usize,
    budget: Option<f64>,
    mut run: impl FnMut() -> Result<usize>,
) -> Result<Measurement> {
    let mut times = Vec::with_capacity(samples);
    let mut rows = 0;
    // The first observation is retained separately; no sample is discarded.
    for _ in 0..samples {
        let start = Instant::now();
        rows = black_box(run()?);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let first_ms = times[0];
    times.sort_by(f64::total_cmp);
    let percentile = |p: f64| times[(samples as f64 * p).ceil() as usize - 1];
    let p95 = percentile(0.95);
    Ok(Measurement {
        name: name.into(),
        samples,
        rows,
        first_ms,
        min_ms: times[0],
        p50_ms: percentile(0.5),
        p95_ms: p95,
        max_ms: times[samples - 1],
        budget_ms: budget,
        p95_within_budget: budget.map(|b| p95 < b),
    })
}

fn cached_startup(db: &Store) -> Result<usize> {
    db.with_read(|c| {
        black_box(directory::stored_identity(c, TEAM)?);
        let rows = directory::list_conversations(c, TEAM)?.len();
        black_box(rail::direct_messages(c, TEAM)?);
        black_box(rail::activity(c, TEAM, "all", true, 1, 0)?);
        black_box(directory::team_icon(c, TEAM)?);
        black_box(directory::people(c, TEAM)?);
        black_box(directory::avatars(c, TEAM)?);
        black_box(emoji::custom_emoji(c, TEAM)?);
        Ok(rows)
    })
}

fn main() -> Result<()> {
    let samples = std::env::var("FLEK_BENCH_SAMPLES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(30)
        .clamp(5, 1000);
    let start = Instant::now();
    let fixture = Fixture::new()?;
    let generation_ms = start.elapsed().as_secs_f64() * 1000.;
    // Keep the synthetic DB for manual EXPLAIN QUERY PLAN work (it is ~170 MB).
    if let Some(keep) = std::env::var_os("FLEK_BENCH_KEEP_DB") {
        std::fs::copy(&fixture.path, keep)?;
    }
    let mut output = Vec::new();
    output.push(measure("connection_cold_open", samples, None, || {
        black_box(fixture.open()?);
        Ok(0)
    })?);
    output.push(measure(
        "connection_cold_open_and_cached_startup",
        samples,
        Some(300.),
        || {
            let db = fixture.open()?;
            cached_startup(&db)
        },
    )?);
    let db = fixture.open()?;
    output.push(measure(
        "cached_startup_reads",
        samples,
        Some(300.),
        || cached_startup(&db),
    )?);
    macro_rules! read {
        ($name:expr, $budget:expr, $run:expr) => {
            output.push(measure($name, samples, $budget, || db.with_read($run))?);
        };
    }
    read!("stored_identity", None, |c| Ok(usize::from(
        directory::stored_identity(c, TEAM)?.is_some()
    )));
    read!("sidebar", None, |c| Ok(directory::list_conversations(
        c, TEAM
    )?
    .len()));
    read!("people", None, |c| Ok(directory::people(c, TEAM)?.len()));
    read!("direct_messages", None, |c| Ok(rail::direct_messages(
        c, TEAM
    )?
    .len()));
    read!("activity_badge", None, |c| Ok(rail::activity(
        c, TEAM, "all", true, 1, 0
    )?
    .items
    .len()));
    read!("activity_page_50", None, |c| Ok(rail::activity(
        c, TEAM, "all", false, 50, 0
    )?
    .items
    .len()));
    read!("timeline_latest_60", None, |c| Ok(messages::page(
        c, CHANNEL, None, 60
    )?
    .len()));
    let middle = support::timestamp(5000);
    read!("timeline_older_60", None, |c| Ok(messages::page(
        c,
        CHANNEL,
        Some(&middle),
        60
    )?
    .len()));
    read!("timeline_window_120", None, |c| Ok(
        messages::timeline_window(c, CHANNEL, &middle, 120)?
            .rows
            .len()
    ));
    read!("timeline_after_60", None, |c| Ok(messages::timeline_after(
        c, CHANNEL, &middle, 60
    )?
    .len()));
    for query in [
        "needle",
        "grades",
        "звіт дедлайн",
        "from:person001 in:#channel000 deploy",
        "таблиц*",
        "has:link",
    ] {
        for (name, sort) in [
            ("relevance", search::Sort::Relevance),
            ("recent", search::Sort::Recent),
        ] {
            let label = format!("search_{name}: {query}");
            read!(&label, Some(50.), |c| {
                // Include parsing and snippets, as the real local search path does.
                let parsed = search::parse(query)?;
                Ok(search::search(c, TEAM, &parsed, sort, 50, ("[", "]"))?.len())
            });
        }
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "profile": if cfg!(debug_assertions) { "debug (do not compare budgets)" } else { "release" },
        "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
        "messages":support::MESSAGES, "users":650, "conversations":100,
        "seed":support::SEED, "raw_json_padding_bytes":768, "generation_ms":generation_ms,
        "database_bytes":std::fs::metadata(&fixture.path)?.len(),
        "cache":"connection cold is reopened; OS file cache is NOT flushed; subsequent query samples reuse a reader",
        "measurements":output,
    })).expect("benchmark report is serializable"));
    Ok(())
}
