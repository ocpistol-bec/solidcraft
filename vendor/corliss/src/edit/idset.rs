//! The object-id allocation set, section t5 of the M stream (spec 11.2.7).
//!
//! Records follow one another, each introduced by its u32 type (the first type is the t5
//! count): type 1 is a chunk of 256 ids (u24 chunk, u32 used blocks, u32 partial blocks, one
//! free-bit byte per partial block), type 2 a run of empty chunks (u24 first, u24 0, u24 end).
//! A u24 0 ends the list. Edits change only the records they touch [W-01].

use std::collections::BTreeSet;

use crate::bytes::{ByteReader, ByteWriter};
use crate::error::{CorlissError, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "id allocation set";

/// One record of the set.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Chunk {
        chunk: u32,
        used: u32,
        partial: u32,
        free: Vec<u8>,
    },
    Run {
        first: u32,
        fill: u32,
        end: u32,
    },
}

fn u24(r: &mut ByteReader<'_>, field: &'static str) -> Result<u32> {
    let b = r.take(3, field)?;
    Ok(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
}

fn p24(w: &mut ByteWriter, v: u32) {
    w.bytes(&v.to_le_bytes()[..3]);
}

fn parse_records(first_type: u32, b: &[u8]) -> Result<Vec<Record>> {
    let mut r = ByteReader::new(b, STRUCTURE);
    let mut out = vec![parse_record(&mut r, first_type)?];
    while r.rest() != [0, 0, 0] {
        let typ = r.u32("type")?;
        out.push(parse_record(&mut r, typ)?);
    }
    Ok(out)
}

fn parse_record(r: &mut ByteReader<'_>, typ: u32) -> Result<Record> {
    match typ {
        1 => {
            let chunk = u24(r, "chunk")?;
            let (used, partial) = (r.u32("used")?, r.u32("partial")?);
            let free = r.take(partial.count_ones() as usize, "free")?.to_vec();
            Ok(Record::Chunk {
                chunk,
                used,
                partial,
                free,
            })
        }
        2 => Ok(Record::Run {
            first: u24(r, "first")?,
            fill: u24(r, "fill")?,
            end: u24(r, "end")?,
        }),
        other => Err(CorlissError::invalid(
            STRUCTURE,
            "type",
            r.position(),
            other,
            "1 or 2",
        )),
    }
}

fn serialize_records(recs: &[Record]) -> (u32, Vec<u8>) {
    let mut w = ByteWriter::default();
    for (k, rec) in recs.iter().enumerate() {
        // The first record's type is stored apart, as the t5 count word.
        if k > 0 {
            w.u32(record_type(rec));
        }
        write_record(&mut w, rec);
    }
    w.bytes(&[0, 0, 0]);
    (recs.first().map_or(1, record_type), w.into_bytes())
}

fn record_type(rec: &Record) -> u32 {
    if matches!(rec, Record::Chunk { .. }) {
        1
    } else {
        2
    }
}

fn write_record(w: &mut ByteWriter, rec: &Record) {
    match rec {
        Record::Chunk {
            chunk,
            used,
            partial,
            free,
        } => {
            p24(w, *chunk);
            w.u32(*used).u32(*partial).bytes(free);
        }
        Record::Run { first, fill, end } => [*first, *fill, *end].iter().for_each(|v| p24(w, *v)),
    }
}

/// The allocated ids of a set (id 0 included when its block is used).
pub fn decode(first_type: u32, b: &[u8]) -> Result<BTreeSet<u64>> {
    let mut ids = BTreeSet::new();
    for rec in parse_records(first_type, b)? {
        let Record::Chunk { chunk, .. } = rec else {
            continue;
        };
        for (blk, mask) in blocks(&rec) {
            let base = 256 * u64::from(chunk) + 8 * u64::from(blk);
            ids.extend((0..8u64).filter(|j| mask >> j & 1 == 0).map(|j| base + j));
        }
    }
    Ok(ids)
}

/// A chunk's blocks as (block → free mask).
fn blocks(rec: &Record) -> Vec<(u32, u8)> {
    let Record::Chunk {
        used,
        partial,
        free,
        ..
    } = rec
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut k = 0;
    for blk in 0..32 {
        if used >> blk & 1 == 1 {
            let mask = if partial >> blk & 1 == 1 {
                free.get(k).copied().unwrap_or(0)
            } else {
                0
            };
            k += usize::from(partial >> blk & 1 == 1);
            out.push((blk, mask));
        }
    }
    out
}

fn chunk_of(chunk: u32, blocks: &[(u32, u8)]) -> Record {
    let (mut used, mut partial, mut free) = (0u32, 0u32, Vec::new());
    for (blk, mask) in blocks {
        used |= 1 << blk;
        if *mask != 0 {
            partial |= 1 << blk;
            free.push(*mask);
        }
    }
    Record::Chunk {
        chunk,
        used,
        partial,
        free,
    }
}

/// Marks `add` allocated; a block that was not used becomes used with its other ids up to
/// max(new id, `id_space`) free. Returns the new (first type, bytes).
pub fn add(first_type: u32, b: &[u8], add: &[u64], id_space: u64) -> Result<(u32, Vec<u8>)> {
    let mut recs = parse_records(first_type, b)?;
    let hi = add.iter().copied().fold(id_space, u64::max);
    for &x in add.iter().collect::<BTreeSet<_>>() {
        mark(&mut recs, x, hi);
    }
    Ok(serialize_records(&recs))
}

/// Marks id `x` used in its chunk's record.
fn mark(recs: &mut Vec<Record>, x: u64, hi: u64) {
    let (c, blk, bit) = ((x / 256) as u32, ((x % 256) / 8) as u32, (x % 8) as u8);
    let pos = recs
        .iter()
        .position(|r| matches!(r, Record::Chunk { chunk, .. } if *chunk == c));
    let mut bl = pos.map(|p| blocks(&recs[p])).unwrap_or_default();
    match bl.iter_mut().find(|(b, _)| *b == blk) {
        Some((_, mask)) => *mask &= !(1 << bit),
        None => {
            let base = 256 * u64::from(c) + 8 * u64::from(blk);
            let mask = (0..8u8)
                .filter(|j| *j != bit && base + u64::from(*j) <= hi)
                .fold(0u8, |m, j| m | 1 << j);
            bl.push((blk, mask));
            bl.sort_unstable();
        }
    }
    place(recs, pos, c, chunk_of(c, &bl));
}

/// Puts a chunk record in place: replacing its old record, splitting a run, or in order.
fn place(recs: &mut Vec<Record>, pos: Option<usize>, c: u32, rec: Record) {
    if let Some(p) = pos {
        recs[p] = rec;
        return;
    }
    let run = recs
        .iter()
        .position(|r| matches!(r, Record::Run { first, end, .. } if *first <= c && c < *end));
    if let Some(k) = run {
        split_run(recs, k, c, rec);
        return;
    }
    let last_chunk = |r: &Record| match r {
        Record::Chunk { chunk, .. } => *chunk,
        Record::Run { end, .. } => end.saturating_sub(1),
    };
    let k = recs
        .iter()
        .position(|r| last_chunk(r) >= c)
        .unwrap_or(recs.len());
    recs.insert(k, rec);
}

/// Replaces chunk `c` of run `recs[k]` with `rec`, keeping the run's other chunks around it.
fn split_run(recs: &mut Vec<Record>, k: usize, c: u32, rec: Record) {
    let Record::Run { first, fill, end } = recs[k].clone() else {
        return;
    };
    let mut parts = Vec::new();
    if first < c {
        parts.push(Record::Run {
            first,
            fill,
            end: c,
        });
    }
    parts.push(rec);
    if c + 1 < end {
        parts.push(Record::Run {
            first: c + 1,
            fill,
            end,
        });
    }
    recs.splice(k..=k, parts);
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// Where the bitset ends: the offset of the id-space word that is followed by an object list
/// and a tail whose lengths fit the section exactly; the longest object list wins. Returns
/// (offset, object count, tail count).
fn split_point(b: &[u8], id_space: u32) -> Option<(usize, usize, usize)> {
    let mut best: Option<(usize, usize, usize)> = None;
    for p in (0..b.len().saturating_sub(5)).filter(|p| u32_at(b, *p) == Some(id_space)) {
        let n = usize::from(u16::from_le_bytes([b[p + 4], b[p + 5]]));
        let q = p + 6 + 20 * n;
        let Some(m) = u32_at(b, q) else { continue };
        let fits = (b.len() as i64 - q as i64 - 4) == 6 * i64::from(m) - 2;
        if fits && best.is_none_or(|(_, bn, _)| n > bn) {
            best = Some((p, n, m as usize));
        }
    }
    best
}

/// The M stream's t5 section split into its parts (spec 11.2.7 syntax block): the object-id
/// allocation set and what follows it.
///
/// ```no_run
/// # let section: Vec<u8> = Vec::new();
/// let t5 = corliss::edit::idset::T5::split(&section, 226).unwrap();
/// let ids = corliss::edit::idset::decode(t5.first_type, &t5.bitset).unwrap();
/// assert_eq!(t5.encode(), section);
/// # let _ = ids;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T5 {
    /// Type of the first record (1 chunk, 2 run), stored as the section's count word.
    pub first_type: u32,
    /// The allocation set's records after the first type word.
    pub bitset: Vec<u8>,
    /// The id space the section records (the M stream's last sec2 N).
    pub id_space: u32,
    /// Object classes and their counts.
    pub objs: Vec<(Guid, u32)>,
    /// Count of the trailing u16 words, as stored.
    pub m: u32,
    /// The trailing u16 words.
    pub rest: Vec<u16>,
}

impl T5 {
    /// Splits a t5 section (count + body) given the M stream's id space.
    pub fn split(section: &[u8], id_space: u32) -> Option<T5> {
        let first_type = u32::from_le_bytes(section.get(..4)?.try_into().ok()?);
        let b = section.get(4..)?;
        let (p, n, m) = split_point(b, id_space)?;
        let objs = (0..n)
            .map(|k| {
                let at = p + 6 + 20 * k;
                let g = Guid::from_bytes(b[at..at + 16].try_into().unwrap_or([0; 16]));
                (g, u32_at(b, at + 16).unwrap_or(0))
            })
            .collect();
        let rest = b[p + 6 + 20 * n + 4..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        Some(T5 {
            first_type,
            bitset: b[..p].to_vec(),
            id_space,
            objs,
            m: m as u32,
            rest,
        })
    }

    /// The section's bytes; `T5::split(s, n)?.encode() == s`.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        w.u32(self.first_type)
            .bytes(&self.bitset)
            .u32(self.id_space)
            .u16(self.objs.len() as u16);
        for (g, i) in &self.objs {
            w.guid(g).u32(*i);
        }
        w.u32(self.m);
        self.rest.iter().for_each(|v| {
            w.u16(*v);
        });
        w.into_bytes()
    }
}
