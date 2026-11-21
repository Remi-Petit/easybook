//! Charge synthétique et calculs de référence.
//!
//! Une mesure = une ligne « cgroup » (mémoire totale, anonyme, cache, CPU) et
//! une ligne par groupe de programmes (API, interface, nginx, superviseur),
//! exactement ce que l'échantillonneur serveur écrira toutes les N secondes.

use std::collections::BTreeMap;

/// Groupes de programmes suivis (comme la page Performances).
pub const GROUPS: [&str; 4] = ["easybook", "node", "nginx", "bash"];

/// Une mesure agrégée du conteneur.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub at_ms: i64,
    pub mem_total: i64,
    pub mem_anon: i64,
    pub mem_file: i64,
    pub cpu_pct: f32,
}

/// La part d'un groupe de programmes dans une mesure.
#[derive(Clone, Copy, Debug)]
pub struct ProcSample {
    pub at_ms: i64,
    pub idx: u16,
    pub rss: i64,
    pub cpu_pct: f32,
}

/// Une mesure complète : le total et ses groupes.
#[derive(Clone, Copy, Debug)]
pub struct SampleSet {
    pub sample: Sample,
    pub procs: [ProcSample; GROUPS.len()],
}

#[derive(Clone, Copy, Debug)]
pub struct RawPoint {
    pub at_ms: i64,
    pub mem_total: i64,
    pub cpu_pct: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bucket {
    pub at_ms: i64,
    pub avg_mem: f64,
    pub max_mem: i64,
    pub avg_cpu: f64,
    pub max_cpu: f32,
}

/// Valeurs de base par groupe : (RSS moyen, part CPU moyenne).
const BASE: [(i64, f32); GROUPS.len()] = [
    (35 * 1024 * 1024, 2.5), // API Rust
    (62 * 1024 * 1024, 0.8), // interface Node
    (9 * 1024 * 1024, 0.4),  // nginx (deux workers, regroupés)
    (3 * 1024 * 1024, 0.05), // script d'entrée
];

/// Générateur déterministe (xorshift) : mêmes données d'un moteur à l'autre.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Bruit dans [-1, 1].
    fn noise(&mut self) -> f64 {
        (self.next() % 2001) as f64 / 1000.0 - 1.0
    }
}

/// Construit `count` mesures espacées de `interval_ms`, avec une variation
/// lente (marée) et du bruit, comme une vraie journée d'utilisation.
pub fn generate(count: usize, interval_ms: i64, start_ms: i64, seed: u64) -> Vec<SampleSet> {
    let mut rng = Rng(seed);
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let at_ms = start_ms + index as i64 * interval_ms;
        // Période de ~2 heures : plusieurs marées dans une fenêtre de 30 jours.
        let wave = (index as f64 * interval_ms as f64 / 7_200_000.0 * std::f64::consts::TAU).sin();
        let mut procs = [ProcSample {
            at_ms,
            idx: 0,
            rss: 0,
            cpu_pct: 0.0,
        }; GROUPS.len()];
        let mut total_rss = 0;
        for (idx, (rss_base, cpu_base)) in BASE.iter().enumerate() {
            let jitter = 1.0 + 0.18 * wave + 0.06 * rng.noise();
            let rss = (*rss_base as f64 * jitter) as i64;
            let cpu = ((*cpu_base as f64) * (1.0 + 0.45 * wave + 0.6 * rng.noise())).max(0.01);
            procs[idx] = ProcSample {
                at_ms,
                idx: idx as u16,
                rss,
                cpu_pct: cpu as f32,
            };
            total_rss += rss;
        }
        // Cache de fichiers (binaires, livres lus) : 1,4× la mémoire privée.
        let file = (total_rss as f64 * 0.4) as i64;
        out.push(SampleSet {
            sample: Sample {
                at_ms,
                mem_total: total_rss + file,
                mem_anon: total_rss,
                mem_file: file,
                cpu_pct: procs.iter().map(|p| p.cpu_pct).sum(),
            },
            procs,
        });
    }
    out
}

/// Agrégation de référence (ce que la requête doit rendre), calculée en Rust.
pub fn reference_buckets(data: &[SampleSet], from: i64, to: i64, step: i64) -> Vec<Bucket> {
    let mut buckets: BTreeMap<i64, Accum> = BTreeMap::new();
    for set in data {
        let at = set.sample.at_ms;
        if at < from || at >= to {
            continue;
        }
        let bucket = buckets.entry(at / step * step).or_default();
        bucket.push(set.sample.mem_total, set.sample.cpu_pct);
    }
    buckets
        .into_iter()
        .map(|(at_ms, acc)| acc.finish(at_ms))
        .collect()
}

pub fn reference_proc_buckets(
    data: &[SampleSet],
    from: i64,
    to: i64,
    step: i64,
    idx: u16,
) -> Vec<Bucket> {
    let mut buckets: BTreeMap<i64, Accum> = BTreeMap::new();
    for set in data {
        let at = set.sample.at_ms;
        if at < from || at >= to {
            continue;
        }
        let proc = set.procs[idx as usize];
        let bucket = buckets.entry(at / step * step).or_default();
        bucket.push(proc.rss, proc.cpu_pct);
    }
    buckets
        .into_iter()
        .map(|(at_ms, acc)| acc.finish(at_ms))
        .collect()
}

#[derive(Default)]
struct Accum {
    mem_sum: f64,
    mem_max: i64,
    cpu_sum: f64,
    cpu_max: f32,
    count: u64,
}

impl Accum {
    fn push(&mut self, mem: i64, cpu: f32) {
        self.mem_sum += mem as f64;
        self.mem_max = self.mem_max.max(mem);
        self.cpu_sum += cpu as f64;
        self.cpu_max = self.cpu_max.max(cpu);
        self.count += 1;
    }

    fn finish(self, at_ms: i64) -> Bucket {
        let count = self.count.max(1) as f64;
        Bucket {
            at_ms,
            avg_mem: self.mem_sum / count,
            max_mem: self.mem_max,
            avg_cpu: self.cpu_sum / count,
            max_cpu: self.cpu_max,
        }
    }
}
