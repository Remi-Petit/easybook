//! Inspection ponctuelle d'une base DuckDB produite par le banc.
//!
//!     cargo run --release --no-default-features --features duckdb \
//!         --bin duck-inspect -- <dossier> <from> <to> <step>

use anyhow::Result;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| ".data/duckdb".into());
    let from: i64 = args.next().unwrap_or_else(|| "0".into()).parse()?;
    let to: i64 = args.next().unwrap_or_else(|| "9999999999999".into()).parse()?;
    let step: i64 = args.next().unwrap_or_else(|| "60000".into()).parse()?;
    let conn = duckdb::Connection::open(format!("{dir}/duck.duckdb"))?;
    let mut stmt = conn.prepare("SELECT count(*), min(at_ms), max(at_ms), avg(mem_total), max(mem_total) FROM samples")?;
    let mut rows = stmt.query([])?;
    let row = rows.next()?.expect("une ligne");
    println!(
        "total: count={} min={} max={} avg_mem={} max_mem={}",
        row.get::<_, i64>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, f64>(3)?,
        row.get::<_, i64>(4)?,
    );
    let mut stmt = conn.prepare(
        "SELECT count(*), COALESCE(avg(mem_total), 0), COALESCE(max(mem_total), 0) FROM samples WHERE at_ms >= ? AND at_ms < ?",
    )?;
    let mut rows = stmt.query(duckdb::params![from, to])?;
    let row = rows.next()?.expect("une ligne");
    println!(
        "plage: count={} avg_mem={} max_mem={}",
        row.get::<_, i64>(0)?,
        row.get::<_, f64>(1)?,
        row.get::<_, i64>(2)?,
    );
    let mut stmt = conn.prepare(
        "SELECT CAST(at_ms / ? AS BIGINT) * ? AS b, count(*), avg(mem_total), max(mem_total)
         FROM samples WHERE at_ms >= ? AND at_ms < ? GROUP BY b ORDER BY b LIMIT 3",
    )?;
    let mut rows = stmt.query(duckdb::params![step, step, from, to])?;
    while let Some(row) = rows.next()? {
        println!(
            "bucket {} count={} avg={} max={}",
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, i64>(3)?,
        );
    }
    let mut stmt = conn.prepare(
        "SELECT min(at_ms), max(at_ms), count(*) FROM samples
         WHERE CAST(at_ms / ? AS BIGINT) * ? = ?",
    )?;
    let mut rows = stmt.query(duckdb::params![step, step, 1792666800000_i64])?;
    if let Some(row) = rows.next()? {
        println!(
            "premier bucket: min={} max={} count={}",
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        );
    }
    let mut stmt = conn.prepare(
        "SELECT at_ms, at_ms / ?, CAST(at_ms / ? AS BIGINT) * ? AS b
         FROM samples ORDER BY at_ms LIMIT 40",
    )?;
    let mut rows = stmt.query(duckdb::params![step, step, step])?;
    while let Some(row) = rows.next()? {
        println!(
            "at={} quotient={} bucket={}",
            row.get::<_, i64>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?,
        );
    }
    Ok(())
}
