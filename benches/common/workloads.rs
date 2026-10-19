//! Deterministic workloads: UUID v4 / v7, dense per-tenant sequences and mixtures.

use std::fmt;

/// SplitMix64: tiny, fast, good enough statistical quality for benchmark data, and fully
/// deterministic across platforms.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x5DEE_CE66_D1CE_4E5B)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u128(&mut self) -> u128 {
        ((self.next_u64() as u128) << 64) | self.next_u64() as u128
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }

    /// Uniform-ish in `0..n` (`n > 0`).
    pub fn below_u128(&mut self, n: u128) -> u128 {
        if n <= u64::MAX as u128 {
            self.below(n as u64) as u128
        } else {
            self.next_u128() % n
        }
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// Number of tenants of the dense workloads.
pub const TENANTS: u64 = 4;

/// UUID version / variant bits in the big-endian `u128` representation (`Uuid::as_u128`).
const VERSION_SHIFT: u32 = 76;
const VARIANT_SHIFT: u32 = 62;

fn uuid_bits(random: u128, version: u128) -> u128 {
    let v = random & !(0xF << VERSION_SHIFT) & !(0b11 << VARIANT_SHIFT);
    v | (version << VERSION_SHIFT) | (0b10 << VARIANT_SHIFT)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Random UUID v4 (122 random bits): virtually one value per 112-bit key.
    UuidV4,
    /// UUID v7: 48-bit ms timestamp (about 4 values per ms), version/variant, random rest.
    UuidV7,
    /// `(tenant << 64) | seq` with long sequential runs (1k..61k values, gaps of 1..4000):
    /// bitmap containers after inserts, run containers after `optimize`.
    DenseRuns,
    /// `(tenant << 64) | seq`, every sequence number kept with probability 1/2: bitmap
    /// containers (~32k values each).
    DenseGaps50,
    /// `(tenant << 64) | seq` with gaps uniform in 1..=64 (~2000 values per container): arrays.
    DenseSmall,
    /// Union of v4, v7, runs, gaps50 and small-gap parts (n/5 each, disjoint tenants).
    Mixed,
    /// Uniform random `u64` values (fit in `u64`, for the `RoaringTreemap` comparison).
    RandomU64,
    /// [`Kind::DenseRuns`] with `tenant << 40`, so values fit in `u64`.
    DenseRunsU64,
    /// [`Kind::DenseGaps50`] with `tenant << 40`.
    DenseGaps50U64,
    /// [`Kind::DenseSmall`] with `tenant << 40`.
    DenseSmallU64,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::UuidV4 => "uuid_v4",
            Kind::UuidV7 => "uuid_v7",
            Kind::DenseRuns => "dense_runs",
            Kind::DenseGaps50 => "dense_gaps50",
            Kind::DenseSmall => "dense_small",
            Kind::Mixed => "mixed",
            Kind::RandomU64 => "random_u64",
            Kind::DenseRunsU64 => "dense_runs_u64",
            Kind::DenseGaps50U64 => "dense_gaps50_u64",
            Kind::DenseSmallU64 => "dense_small_u64",
        }
    }

    /// All values fit in `u64` (the `RoaringTreemap` baseline can store them directly).
    pub fn fits_u64(self) -> bool {
        matches!(
            self,
            Kind::RandomU64 | Kind::DenseRunsU64 | Kind::DenseGaps50U64 | Kind::DenseSmallU64
        )
    }

    pub const ALL: [Kind; 10] = [
        Kind::UuidV4,
        Kind::UuidV7,
        Kind::DenseRuns,
        Kind::DenseGaps50,
        Kind::DenseSmall,
        Kind::Mixed,
        Kind::RandomU64,
        Kind::DenseRunsU64,
        Kind::DenseGaps50U64,
        Kind::DenseSmallU64,
    ];

    /// Generates `n` distinct values (in generation order, not necessarily sorted).
    pub fn generate(self, n: usize, seed: u64) -> Vec<u128> {
        let mut rng = Rng::new(seed.wrapping_mul(0x1000_0000_01B3) ^ self as u64);
        match self {
            Kind::UuidV4 => (0..n).map(|_| uuid_bits(rng.next_u128(), 4)).collect(),
            Kind::UuidV7 => uuid_v7(n, &mut rng),
            Kind::DenseRuns => dense(n, &mut rng, 1, 64, Dense::Runs),
            Kind::DenseGaps50 => dense(n, &mut rng, 1, 64, Dense::Gaps50),
            Kind::DenseSmall => dense(n, &mut rng, 1, 64, Dense::Small),
            Kind::RandomU64 => (0..n).map(|_| rng.next_u64() as u128).collect(),
            Kind::DenseRunsU64 => dense(n, &mut rng, 1, 40, Dense::Runs),
            Kind::DenseGaps50U64 => dense(n, &mut rng, 1, 40, Dense::Gaps50),
            Kind::DenseSmallU64 => dense(n, &mut rng, 1, 40, Dense::Small),
            Kind::Mixed => {
                let part = n / 5;
                let mut v = Vec::with_capacity(n);
                v.extend((0..part).map(|_| uuid_bits(rng.next_u128(), 4)));
                v.extend(uuid_v7(part, &mut rng));
                v.extend(dense(part, &mut rng, 100, 64, Dense::Runs));
                v.extend(dense(part, &mut rng, 200, 64, Dense::Gaps50));
                v.extend(dense(n - 4 * part, &mut rng, 300, 64, Dense::Small));
                v
            }
        }
    }
}

fn uuid_v7(n: usize, rng: &mut Rng) -> Vec<u128> {
    // 2025-10-09T..., in ms. About 4 values per millisecond.
    let mut ts: u128 = 1_760_000_000_000;
    (0..n)
        .map(|_| {
            if rng.below(4) == 0 {
                ts += 1;
            }
            let rand_a = rng.below(1 << 12) as u128;
            let rand_b = (rng.next_u64() >> 2) as u128;
            (ts << 80) | (7 << VERSION_SHIFT) | (rand_a << 64) | (0b10 << VARIANT_SHIFT) | rand_b
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Dense {
    Runs,
    Gaps50,
    Small,
}

/// `TENANTS` tenants starting at `tenant_base`; each tenant's sequence starts at a fixed offset
/// (independent of the seed, so two seeds give overlapping sets).
fn dense(n: usize, rng: &mut Rng, tenant_base: u64, shift: u32, mode: Dense) -> Vec<u128> {
    let mut out = Vec::with_capacity(n);
    for t in 0..TENANTS {
        let tenant = (tenant_base + t) as u128;
        let quota = n / TENANTS as usize + usize::from((t as usize) < n % TENANTS as usize);
        let mut seq: u64 = 40_000 * t;
        let mut left = quota;
        while left > 0 {
            match mode {
                Dense::Runs => {
                    let len = (1_000 + rng.below(60_000) as usize).min(left);
                    out.extend((seq..seq + len as u64).map(|s| (tenant << shift) | s as u128));
                    seq += len as u64 + 1 + rng.below(4_000);
                    left -= len;
                }
                Dense::Gaps50 => {
                    if rng.below(2) == 0 {
                        out.push((tenant << shift) | seq as u128);
                        left -= 1;
                    }
                    seq += 1;
                }
                Dense::Small => {
                    out.push((tenant << shift) | seq as u128);
                    left -= 1;
                    seq += 1 + rng.below(64);
                }
            }
        }
        debug_assert!(shift == 64 || seq < 1 << shift);
    }
    out
}

/// Formats `n` as `10k`, `1M`, ...
pub fn short_n(n: usize) -> String {
    match n {
        n if n >= 1_000_000 && n % 1_000_000 == 0 => format!("{}M", n / 1_000_000),
        n if n >= 1_000 && n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

/// One generated data set.
pub struct Workload {
    pub name: String,
    pub kind: Kind,
    /// Values in random (insertion) order.
    pub shuffled: Vec<u128>,
    /// Values sorted and deduplicated.
    pub sorted: Vec<u128>,
}

impl fmt::Debug for Workload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} values)", self.name, self.sorted.len())
    }
}

impl Workload {
    pub fn new(kind: Kind, n: usize, seed: u64) -> Workload {
        Workload::from_values(
            format!("{}_{}", kind.name(), short_n(n)),
            kind,
            kind.generate(n, seed),
            seed,
        )
    }

    pub fn from_values(name: String, kind: Kind, mut values: Vec<u128>, seed: u64) -> Workload {
        values.sort_unstable();
        values.dedup();
        let sorted = values.clone();
        Rng::new(seed ^ 0xA5A5).shuffle(&mut values);
        Workload {
            name,
            kind,
            shuffled: values,
            sorted,
        }
    }

    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    pub fn contains(&self, v: u128) -> bool {
        self.sorted.binary_search(&v).is_ok()
    }

    /// `k` values of the set, random order (with replacement).
    pub fn hit_probes(&self, k: usize, seed: u64) -> Vec<u128> {
        let mut rng = Rng::new(seed ^ 0x4849_5453);
        (0..k)
            .map(|_| self.sorted[rng.below(self.len() as u64) as usize])
            .collect()
    }

    /// `k` values not in the set: half "near" misses (a present value plus 1..=256, which mostly
    /// land in an existing container / outer key) and half "far" misses (uniform between the
    /// minimum and the maximum).
    pub fn miss_probes(&self, k: usize, seed: u64) -> Vec<u128> {
        let mut rng = Rng::new(seed ^ 0x4D49_5353);
        let (min, max) = (self.sorted[0], *self.sorted.last().unwrap());
        let mut out = Vec::with_capacity(k);
        let mut attempts = 0usize;
        while out.len() < k {
            attempts += 1;
            let near = out.len() % 2 == 0 && attempts < 64 * k;
            let candidate = if near {
                let base = self.sorted[rng.below(self.len() as u64) as usize];
                base.wrapping_add(1 + rng.below(256) as u128)
            } else {
                min + rng.below_u128(max - min)
            };
            if !self.contains(candidate) {
                out.push(candidate);
            }
        }
        out
    }

    /// `count` inclusive windows over consecutive stored values, each holding `span` values.
    pub fn windows(&self, count: usize, span: usize, seed: u64) -> Vec<(u128, u128)> {
        let mut rng = Rng::new(seed ^ 0x5749_4E44);
        let span = span.clamp(1, self.len());
        (0..count)
            .map(|_| {
                let i = rng.below((self.len() - span + 1) as u64) as usize;
                (self.sorted[i], self.sorted[i + span - 1])
            })
            .collect()
    }

    /// A random subset holding `fraction` of the values.
    pub fn subset(&self, fraction: f64, seed: u64) -> Workload {
        let mut values = self.shuffled.clone();
        Rng::new(seed ^ 0x5355_4253).shuffle(&mut values);
        values.truncate(((self.len() as f64) * fraction).round().max(1.0) as usize);
        let name = format!("{}_sub{}", self.name, (fraction * 100.0).round() as u64);
        Workload::from_values(name, self.kind, values, seed)
    }
}

/// Two sets for the binary operations.
pub struct Pair {
    pub name: String,
    pub a: Workload,
    pub b: Workload,
}

impl Pair {
    /// `|A| = n_a`, `|B| = n_b`, `|A ∩ B| = shared`: both drawn from one pool of distinct values.
    pub fn split(kind: Kind, n_a: usize, n_b: usize, shared: usize, seed: u64) -> Pair {
        assert!(shared <= n_a.min(n_b));
        let mut pool = kind.generate(n_a + n_b - shared, seed);
        pool.sort_unstable();
        pool.dedup();
        Rng::new(seed ^ 0x504F_4F4C).shuffle(&mut pool);
        let a = pool[..n_a].to_vec();
        let b = pool[n_a - shared..n_a - shared + n_b].to_vec();
        let name = if n_a == n_b {
            let pct = shared as f64 * 100.0 / n_a as f64;
            format!("{}_{}_ov{}", kind.name(), short_n(n_a), pct.round() as u64)
        } else {
            format!("{}_{}_vs_{}", kind.name(), short_n(n_a), short_n(n_b))
        };
        Pair {
            a: Workload::from_values(format!("{name}_a"), kind, a, seed),
            b: Workload::from_values(format!("{name}_b"), kind, b, seed + 1),
            name,
        }
    }

    /// Two independent samples of the same distribution (same tenants and offsets), used for the
    /// dense workloads whose structure a random split would destroy.
    pub fn independent(kind: Kind, n: usize, seed: u64) -> Pair {
        let name = format!("{}_{}_indep", kind.name(), short_n(n));
        Pair {
            a: Workload::from_values(format!("{name}_a"), kind, kind.generate(n, seed), seed),
            b: Workload::from_values(
                format!("{name}_b"),
                kind,
                kind.generate(n, seed + 1),
                seed + 1,
            ),
            name,
        }
    }

    pub fn shared(&self) -> usize {
        let (mut i, mut j, mut c) = (0, 0, 0);
        let (a, b) = (&self.a.sorted, &self.b.sorted);
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    c += 1;
                    i += 1;
                    j += 1;
                }
            }
        }
        c
    }
}

/// `count` sets of `n` values each: a common core of `core` values present in every set plus
/// `n - core` values unique to each set (for the 16-way union / intersection).
pub fn many_with_core(kind: Kind, count: usize, n: usize, core: usize, seed: u64) -> Vec<Workload> {
    let mut pool = kind.generate(core + count * (n - core), seed);
    pool.sort_unstable();
    pool.dedup();
    Rng::new(seed ^ 0x434F_5245).shuffle(&mut pool);
    let (common, rest) = pool.split_at(core);
    (0..count)
        .map(|i| {
            let mut v = common.to_vec();
            v.extend_from_slice(&rest[i * (n - core)..(i + 1) * (n - core)]);
            Workload::from_values(
                format!("{}_{}x{}", kind.name(), count, short_n(n)),
                kind,
                v,
                seed + i as u64,
            )
        })
        .collect()
}

/// `count` independent samples of a (dense) distribution.
pub fn many_independent(kind: Kind, count: usize, n: usize, seed: u64) -> Vec<Workload> {
    (0..count)
        .map(|i| {
            Workload::from_values(
                format!("{}_{}x{}", kind.name(), count, short_n(n)),
                kind,
                kind.generate(n, seed + i as u64),
                seed + i as u64,
            )
        })
        .collect()
}
