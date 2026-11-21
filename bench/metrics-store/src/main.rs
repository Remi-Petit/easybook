//! Banc d'essai du stockage de l'historique de performances.
//!
//! Charge : 30 jours de mesures à la fréquence d'échantillonnage choisie
//! (10 s par défaut = 259 200 mesures, 1 036 800 lignes de groupes).
//! Mesures : remplissage, écriture unitaire durable (p50/p99), lectures
//! agrégées des fenêtres affichées par la page, purge de rétention, taille
//! sur disque, RAM du process, ouverture. Les agrégats rendus par chaque
//! moteur sont comparés à un calcul de référence en Rust.
//!
//! Usage :
//!     cargo run --release --features sqlite,redb,heed -- --engine sqlite-wal
//!     cargo run --release --features duckdb -- --engine duckdb

mod engines;
mod model;

use anyhow::{Context, Result};
use engines::Store;
use model::{generate, reference_buckets, reference_proc_buckets, Bucket, SampleSet};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SEC: i64 = 1000;
const MIN: i64 = 60 * SEC;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

struct Args {
    engine: String,
    days: i64,
    interval_ms: i64,
    dir: PathBuf,
    writes: usize,
    batch: usize,
    trim: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        engine: String::new(),
        days: 30,
        interval_ms: 10 * SEC,
        dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".data"),
        writes: 500,
        batch: 1000,
        trim: true,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = || -> Result<String> {
            iter.next()
                .with_context(|| format!("valeur manquante après {arg}"))
        };
        match arg.as_str() {
            "--engine" => args.engine = value()?,
            "--days" => args.days = value()?.parse()?,
            "--interval" => args.interval_ms = value()?.parse::<i64>()? * SEC,
            "--dir" => args.dir = PathBuf::from(value()?),
            "--writes" => args.writes = value()?.parse()?,
            "--batch" => args.batch = value()?.parse()?,
            "--no-trim" => args.trim = false,
            "--list" => {
                println!("{}", engines::names().join("\n"));
                std::process::exit(0);
            }
            other => anyhow::bail!("argument inconnu : {other}"),
        }
    }
    if args.engine.is_empty() {
        anyhow::bail!(
            "--engine requis (disponibles : {})",
            engines::names().join(", ")
        );
    }
    if args.interval_ms <= 0 {
        anyhow::bail!("--interval doit être positif");
    }
    Ok(args)
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let count = (args.days * DAY / args.interval_ms) as usize;
    let start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64
        - args.days * DAY;
    let start = start / args.interval_ms * args.interval_ms;
    let data = generate(count, args.interval_ms, start, 0x9e37_79b9_7f4a_7c15);

    let dir = args.dir.join(&args.engine);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("vider {}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir)?;

    let open_at = Instant::now();
    let mut store = engines::open(&args.engine, &dir)?;
    let open_ms = open_at.elapsed().as_secs_f64() * 1000.0;

    // Remplissage par lots : une transaction durable par lot.
    let fill_at = Instant::now();
    for chunk in data.chunks(args.batch) {
        store.append(chunk)?;
    }
    store.flush()?;
    let fill_ms = fill_at.elapsed().as_secs_f64() * 1000.0;

    // Écriture unitaire : ce que fera l'échantillonneur (une mesure, 5 lignes),
    // à des instants **nouveaux** — comme en production.
    let end = data.last().unwrap().sample.at_ms + args.interval_ms;
    let mut durations = Vec::with_capacity(args.writes);
    for index in 0..args.writes {
        let mut set = data[index % data.len()];
        let at_ms = end + index as i64 * args.interval_ms;
        set.sample.at_ms = at_ms;
        for proc in &mut set.procs {
            proc.at_ms = at_ms;
        }
        let at = Instant::now();
        store.append(std::slice::from_ref(&set))?;
        durations.push(at.elapsed().as_secs_f64() * 1e6);
    }
    durations.sort_by(f64::total_cmp);
    let p50 = percentile(&durations, 0.50);
    let p99 = percentile(&durations, 0.99);

    let raw_3m = median(|| {
        let points = store
            .raw(end - 3 * MIN, end)
            .expect("lecture brute");
        assert!(!points.is_empty());
        points.len()
    });
    let (bucket_24h, actual_24h, ref_24h) =
        timed_buckets(&mut *store, &data, end, DAY, MIN, None)?;
    let (bucket_7d, actual_7d, ref_7d) =
        timed_buckets(&mut *store, &data, end, 7 * DAY, 10 * MIN, None)?;
    let (bucket_30d, actual_30d, ref_30d) =
        timed_buckets(&mut *store, &data, end, 30 * DAY, HOUR, None)?;
    let (proc_30d, actual_proc, ref_proc) =
        timed_buckets(&mut *store, &data, end, 30 * DAY, HOUR, Some(0))?;
    let groups_30d = median(|| {
        store
            .group_buckets(end - 30 * DAY, end, HOUR)
            .expect("agrégat de tous les groupes")
            .len()
    });

    let mut mismatches = 0;
    for (actual, expected) in [
        (&actual_24h, &ref_24h),
        (&actual_7d, &ref_7d),
        (&actual_30d, &ref_30d),
        (&actual_proc, &ref_proc),
    ] {
        mismatches += compare(actual, expected);
    }
    // La charge de test ne doit pas peser dans la RAM mesurée du process.
    if mismatches > 0 {
        eprintln!(
            "écarts : 24h {} vs {}, 7j {} vs {}, 30j {} vs {}, process {} vs {}",
            actual_24h.len(),
            ref_24h.len(),
            actual_7d.len(),
            ref_7d.len(),
            actual_30d.len(),
            ref_30d.len(),
            actual_proc.len(),
            ref_proc.len()
        );
        if let (Some(actual), Some(expected)) = (actual_30d.first(), ref_30d.first()) {
            eprintln!("premier agrégat : {actual:?}\nattendu         : {expected:?}");
        }
    }
    drop(data);

    // Purge de rétention : un jour de mesures, comme le balayage quotidien.
    let mut removed = 0;
    let trim_ms = if args.trim {
        let trim_at = Instant::now();
        removed = store.trim(start + DAY)?;
        store.flush()?;
        eprintln!("purge : {removed} mesures");
        trim_at.elapsed().as_secs_f64() * 1000.0
    } else {
        0.0
    };

    let disk_mb = engines::disk_bytes(&dir) as f64 / (1024.0 * 1024.0);
    let rss_mb = memory_stats::memory_stats()
        .map(|stats| stats.physical_mem as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0);

    println!(
        "{{\"engine\":\"{}\",\"samples\":{},\"rows\":{},\"open_ms\":{:.1},\"fill_ms\":{:.1},\
         \"single_p50_us\":{:.1},\"single_p99_us\":{:.1},\"raw_3m_us\":{:.1},\
         \"bucket_24h_us\":{:.1},\"bucket_7d_us\":{:.1},\"bucket_30d_us\":{:.1},\
         \"proc_30d_us\":{:.1},\"groups_30d_us\":{:.1},\"trim_1d_ms\":{:.1},\"trimmed\":{},\"disk_mb\":{:.1},\
         \"rss_mb\":{:.1},\"mismatches\":{},\"start_ms\":{},\"end_ms\":{}}}",
        args.engine,
        count,
        if args.engine == "sqlite-blob" {
            count
        } else {
            count * (1 + engines::group_count())
        },
        open_ms,
        fill_ms,
        p50,
        p99,
        raw_3m,
        bucket_24h,
        bucket_7d,
        bucket_30d,
        proc_30d,
        groups_30d,
        trim_ms,
        removed,
        disk_mb,
        rss_mb,
        mismatches,
        start,
        end
    );
    Ok(())
}

/// Médiane, en microsecondes, du temps d'une requête (5 passes).
fn median<F>(mut run: F) -> f64
where
    F: FnMut() -> usize,
{
    let mut times = Vec::with_capacity(5);
    for _ in 0..5 {
        let at = Instant::now();
        let _ = run();
        times.push(at.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index]
}

/// Mesure une agrégation et rend (durée médiane en µs, résultat, référence).
fn timed_buckets(
    store: &mut dyn Store,
    data: &[SampleSet],
    end: i64,
    range: i64,
    step: i64,
    proc_idx: Option<u16>,
) -> Result<(f64, Vec<Bucket>, Vec<Bucket>)> {
    let from = end - range;
    let to = end;
    let mut actual = Vec::new();
    let elapsed = median(|| {
        actual = match proc_idx {
            Some(idx) => store
                .proc_buckets(from, to, step, idx)
                .expect("agrégat process"),
            None => store.buckets(from, to, step).expect("agrégat total"),
        };
        actual.len()
    });
    let expected = reference(data, end, range, step, proc_idx);
    Ok((elapsed, actual, expected))
}

fn compare(actual: &[Bucket], expected: &[Bucket]) -> usize {
    if actual.len() != expected.len() {
        return actual.len().max(expected.len());
    }
    actual
        .iter()
        .zip(expected)
        .filter(|(a, e)| {
            a.at_ms != e.at_ms
                || a.max_mem != e.max_mem
                || a.avg_mem != e.avg_mem
                || (a.avg_cpu - e.avg_cpu).abs() > 1e-4
                || (a.max_cpu - e.max_cpu).abs() > 1e-4
        })
        .count()
}

/// Référence des agrégats, calculée une fois sur les données générées.
fn reference(
    data: &[SampleSet],
    end: i64,
    range: i64,
    step: i64,
    proc_idx: Option<u16>,
) -> Vec<Bucket> {
    match proc_idx {
        Some(idx) => reference_proc_buckets(data, end - range, end, step, idx),
        None => reference_buckets(data, end - range, end, step),
    }
}
