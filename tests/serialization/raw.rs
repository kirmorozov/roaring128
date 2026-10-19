//! An independent writer for the format of `FORMAT.md`, able to produce invalid input.

#![allow(dead_code)]

use crate::common::{KEY, Model};

pub const COOKIE: u32 = 0x3831_5252;
pub const HEADER_LEN: usize = 16;

/// The payload of one container, as written.
#[derive(Debug, Clone)]
pub enum Payload {
    /// `low = len - 1`, `2 * len` bytes.
    Array(Vec<u16>),
    /// `low = card - 1` (the declared cardinality, normally the popcount), 8192 bytes.
    Bitmap { words: Vec<u64>, card: u32 },
    /// `low = runs - 1`; every run is `(start, length - 1)`.
    Run(Vec<(u16, u16)>),
}

impl Payload {
    pub fn low(&self) -> u16 {
        match self {
            Payload::Array(v) => (v.len() - 1) as u16,
            Payload::Bitmap { card, .. } => (card - 1) as u16,
            Payload::Run(r) => (r.len() - 1) as u16,
        }
    }

    pub fn is_run(&self) -> bool {
        matches!(self, Payload::Run(_))
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Payload::Array(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            Payload::Bitmap { words, .. } => {
                assert_eq!(words.len(), 1024);
                words.iter().for_each(|w| out.extend(w.to_le_bytes()));
            }
            Payload::Run(r) => r.iter().for_each(|(s, l)| {
                out.extend(s.to_le_bytes());
                out.extend(l.to_le_bytes());
            }),
        }
    }

    /// A bitmap payload with the given indices and the matching cardinality.
    pub fn bitmap_of(indices: impl IntoIterator<Item = u16>) -> Payload {
        let mut words = vec![0u64; 1024];
        let mut card = 0;
        for i in indices {
            let w = &mut words[i as usize / 64];
            if *w & (1 << (i % 64)) == 0 {
                card += 1;
            }
            *w |= 1 << (i % 64);
        }
        Payload::Bitmap { words, card }
    }
}

/// A serialized bitmap description.
#[derive(Debug, Clone)]
pub struct Raw {
    pub cookie: u32,
    /// `None`: bit 0 set iff a container is a run.
    pub flags: Option<u32>,
    /// `None`: the number of containers.
    pub count: Option<u64>,
    /// Write the run bitset even when `flags` bit 0 is clear / omit it when set (for
    /// corruption tests): `None` follows the flags.
    pub write_bitset: Option<bool>,
    pub containers: Vec<(u128, Payload)>,
}

impl Raw {
    pub fn new(containers: Vec<(u128, Payload)>) -> Raw {
        Raw { cookie: COOKIE, flags: None, count: None, write_bitset: None, containers }
    }

    pub fn encode(&self) -> Vec<u8> {
        let any_run = self.containers.iter().any(|(_, p)| p.is_run());
        let flags = self.flags.unwrap_or(any_run as u32);
        let n = self.containers.len();
        let mut out = Vec::new();
        out.extend(self.cookie.to_le_bytes());
        out.extend(flags.to_le_bytes());
        out.extend(self.count.unwrap_or(n as u64).to_le_bytes());
        if self.write_bitset.unwrap_or(flags & 1 != 0) {
            let mut bitset = vec![0u8; n.div_ceil(8)];
            for (i, (_, p)) in self.containers.iter().enumerate() {
                if p.is_run() {
                    bitset[i / 8] |= 1 << (i % 8);
                }
            }
            out.extend(bitset);
        }
        for (key, p) in &self.containers {
            let descriptor = (key << 16) | p.low() as u128;
            out.extend(descriptor.to_le_bytes());
        }
        for (_, p) in &self.containers {
            p.write(&mut out);
        }
        out
    }

    /// The canonical encoding of a set: containers are arrays (cardinality <= 4096) or bitmaps,
    /// or, with `runs`, runs when `2 + 4 * runs` is strictly smaller (the state after
    /// `remove_run_compression()` [then `optimize()`]).
    pub fn from_model(model: &Model, runs: bool) -> Raw {
        let mut groups: Vec<(u128, Vec<u16>)> = Vec::new();
        for &v in model {
            let (k, i) = (v >> 16, v as u16);
            match groups.last_mut() {
                Some((key, vals)) if *key == k => vals.push(i),
                _ => groups.push((k, vec![i])),
            }
        }
        let containers = groups
            .into_iter()
            .map(|(k, vals)| {
                let card = vals.len();
                let intervals = to_runs(&vals);
                let run_size = 2 + 4 * intervals.len();
                let other_size = if card <= 4096 { 2 * card } else { 8192 };
                let p = if runs && run_size < other_size {
                    Payload::Run(intervals)
                } else if card <= 4096 {
                    Payload::Array(vals)
                } else {
                    Payload::bitmap_of(vals)
                };
                (k, p)
            })
            .collect();
        Raw::new(containers)
    }

    /// The set described (assuming valid content).
    pub fn model(&self) -> Model {
        let mut m = Model::new();
        for (k, p) in &self.containers {
            let base = k * KEY;
            match p {
                Payload::Array(v) => m.extend(v.iter().map(|&i| base + i as u128)),
                Payload::Bitmap { words, .. } => {
                    for (j, w) in words.iter().enumerate() {
                        for b in 0..64 {
                            if w & (1 << b) != 0 {
                                m.insert(base + 64 * j as u128 + b);
                            }
                        }
                    }
                }
                Payload::Run(r) => {
                    for &(s, l) in r {
                        m.extend((s as u128..=s as u128 + l as u128).map(|i| base + i));
                    }
                }
            }
        }
        m
    }
}

/// Maximal intervals of sorted indices as `(start, length - 1)`.
pub fn to_runs(sorted: &[u16]) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = Vec::new();
    for &i in sorted {
        match out.last_mut() {
            Some((s, l)) if (*s as u32 + *l as u32 + 1) == i as u32 => *l += 1,
            _ => out.push((i, 0)),
        }
    }
    out
}
