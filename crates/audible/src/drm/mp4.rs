//! Just enough ISO-BMFF (MP4) parsing to find encrypted audio samples and the
//! boxes that must be patched to make the file playable.
//!
//! All offsets are absolute file offsets. Only non-fragmented files are
//! supported, which is what Audible serves for AAX and AAXC.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};

/// Largest `moov` we are willing to load into memory.
const MAX_MOOV: u64 = 256 * 1024 * 1024;

/// A box header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoxHeader {
    /// Absolute offset of the box.
    pub start: u64,
    /// Header length (8, or 16 for 64-bit sizes).
    pub header_len: u64,
    /// Total box size including the header.
    pub size: u64,
    /// Four-character type.
    pub kind: [u8; 4],
}

impl BoxHeader {
    pub fn payload_start(&self) -> u64 {
        self.start + self.header_len
    }

    pub fn end(&self) -> u64 {
        self.start + self.size
    }
}

/// Parse a box header from `buf` at `pos`; `base` is the absolute offset of
/// `buf[0]` and `limit` the absolute end of the enclosing box/file.
fn header_at(buf: &[u8], pos: usize, base: u64, limit: u64) -> Result<BoxHeader> {
    let bad = || {
        Error::InvalidMedia(format!(
            "truncated box header at offset {}",
            base + pos as u64
        ))
    };
    let head = buf.get(pos..pos + 8).ok_or_else(bad)?;
    let size32 = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let kind = [head[4], head[5], head[6], head[7]];
    let start = base + pos as u64;
    let (size, header_len) = match size32 {
        0 => (limit - start, 8),
        1 => {
            let ext = buf.get(pos + 8..pos + 16).ok_or_else(bad)?;
            let mut b = [0u8; 8];
            b.copy_from_slice(ext);
            (u64::from_be_bytes(b), 16)
        }
        n => (u64::from(n), 8),
    };
    if size < header_len || start + size > limit {
        return Err(Error::InvalidMedia(format!(
            "box `{}` at offset {start} overruns its parent",
            String::from_utf8_lossy(&kind)
        )));
    }
    Ok(BoxHeader {
        start,
        header_len,
        size,
        kind,
    })
}

/// Children of a box held in memory. `buf` holds bytes from absolute offset
/// `base`; children are read from `from..to` (absolute).
fn children(buf: &[u8], base: u64, from: u64, to: u64) -> Result<Vec<BoxHeader>> {
    let mut out = Vec::new();
    let mut pos = from;
    while pos + 8 <= to {
        let h = header_at(buf, (pos - base) as usize, base, to)?;
        pos = h.end();
        out.push(h);
    }
    Ok(out)
}

fn find(boxes: &[BoxHeader], kind: &[u8; 4]) -> Option<BoxHeader> {
    boxes.iter().copied().find(|b| &b.kind == kind)
}

fn require(boxes: &[BoxHeader], kind: &[u8; 4], parent: &str) -> Result<BoxHeader> {
    find(boxes, kind).ok_or_else(|| {
        Error::InvalidMedia(format!(
            "`{parent}` has no `{}` box",
            String::from_utf8_lossy(kind)
        ))
    })
}

/// Big-endian reader over a byte slice.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self
            .buf
            .get(self.pos..self.pos + n)
            .ok_or_else(|| Error::InvalidMedia("sample table is truncated".into()))?;
        self.pos += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }
}

/// A contiguous run of samples in `mdat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Absolute offset of the first sample.
    pub offset: u64,
    /// Sizes of the samples, in order.
    pub sample_sizes: Vec<u32>,
}

impl Chunk {
    pub fn len(&self) -> u64 {
        self.sample_sizes.iter().map(|s| u64::from(*s)).sum()
    }
}

/// An encrypted (`aavd`) audio track.
#[derive(Debug, Clone)]
pub(crate) struct EncryptedTrack {
    /// The `aavd` sample entry.
    pub sample_entry: BoxHeader,
    /// Boxes nested in the sample entry (`esds`, `adrm`, `aabd`, ...).
    pub entry_children: Vec<BoxHeader>,
    /// Payload of the `adrm` box (AAX only).
    pub adrm: Option<Vec<u8>>,
    /// Ancestors of the sample entry, outermost first
    /// (`moov`, `trak`, `mdia`, `minf`, `stbl`, `stsd`). Only read by the
    /// test fixture builder, which grows these boxes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub ancestors: Vec<BoxHeader>,
    /// Sample layout.
    pub chunks: Vec<Chunk>,
}

/// What we learned about a file.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    /// The `ftyp` box.
    pub ftyp: Option<BoxHeader>,
    /// `ftyp` payload bytes.
    pub ftyp_payload: Vec<u8>,
    /// Encrypted audio tracks.
    pub tracks: Vec<EncryptedTrack>,
}

/// Parse a file's layout.
pub(crate) fn parse(file: &mut File) -> Result<Layout> {
    let file_len = file.metadata()?.len();
    let mut top = Vec::new();
    let mut pos = 0u64;
    let mut head = [0u8; 16];
    while pos + 8 <= file_len {
        file.seek(SeekFrom::Start(pos))?;
        let n = (file_len - pos).min(16) as usize;
        file.read_exact(&mut head[..n])?;
        let h = header_at(&head[..n], 0, pos, file_len)?;
        pos = h.end();
        top.push(h);
    }
    if find(&top, b"moof").is_some() {
        return Err(Error::InvalidMedia(
            "fragmented MP4 is not supported".into(),
        ));
    }
    let moov = require(&top, b"moov", "file")?;
    if moov.size > MAX_MOOV {
        return Err(Error::InvalidMedia("moov box is implausibly large".into()));
    }
    let moov_buf = read_range(file, moov.start, moov.size)?;
    let base = moov.start;

    let ftyp = find(&top, b"ftyp");
    let ftyp_payload = match ftyp {
        Some(f) => read_range(file, f.payload_start(), f.size - f.header_len)?,
        None => Vec::new(),
    };

    let slice = |h: &BoxHeader| -> &[u8] {
        let s = (h.payload_start() - base) as usize;
        let e = (h.end() - base) as usize;
        &moov_buf[s..e]
    };

    let mut tracks = Vec::new();
    for trak in children(&moov_buf, base, moov.payload_start(), moov.end())?
        .into_iter()
        .filter(|b| &b.kind == b"trak")
    {
        let mdia = require(
            &children(&moov_buf, base, trak.payload_start(), trak.end())?,
            b"mdia",
            "trak",
        )?;
        let mdia_kids = children(&moov_buf, base, mdia.payload_start(), mdia.end())?;
        let Some(minf) = find(&mdia_kids, b"minf") else {
            continue;
        };
        let minf_kids = children(&moov_buf, base, minf.payload_start(), minf.end())?;
        let Some(stbl) = find(&minf_kids, b"stbl") else {
            continue;
        };
        let stbl_kids = children(&moov_buf, base, stbl.payload_start(), stbl.end())?;
        let stsd = require(&stbl_kids, b"stsd", "stbl")?;
        // stsd: version/flags (4) + entry count (4), then entries.
        let entries = children(&moov_buf, base, stsd.payload_start() + 8, stsd.end())?;
        let Some(entry) = entries.first().copied().filter(|e| &e.kind == b"aavd") else {
            continue;
        };
        let entry_children = sample_entry_children(&moov_buf, base, &entry)?;
        let adrm = find(&entry_children, b"adrm").map(|a| slice(&a).to_vec());
        let chunks = sample_chunks(&stbl_kids, slice)?;
        tracks.push(EncryptedTrack {
            sample_entry: entry,
            entry_children,
            adrm,
            ancestors: vec![moov, trak, mdia, minf, stbl, stsd],
            chunks,
        });
    }
    Ok(Layout {
        ftyp,
        ftyp_payload,
        tracks,
    })
}

/// Children of an audio sample entry, after the fixed fields whose length
/// depends on the QuickTime sound description version.
fn sample_entry_children(buf: &[u8], base: u64, entry: &BoxHeader) -> Result<Vec<BoxHeader>> {
    let p = (entry.payload_start() - base) as usize;
    // 6 reserved + 2 data reference index, then a 2-byte version.
    let version = buf
        .get(p + 8..p + 10)
        .map(|v| u16::from_be_bytes([v[0], v[1]]))
        .ok_or_else(|| Error::InvalidMedia("audio sample entry is truncated".into()))?;
    let fixed = match version {
        0 => 28,
        1 => 44,
        2 => 64,
        v => {
            return Err(Error::InvalidMedia(format!(
                "unknown sound description version {v}"
            )));
        }
    };
    children(buf, base, entry.payload_start() + fixed, entry.end())
}

/// Resolve stsz/stsc/stco into chunks of samples.
fn sample_chunks<'a>(
    stbl: &[BoxHeader],
    slice: impl Fn(&BoxHeader) -> &'a [u8],
) -> Result<Vec<Chunk>> {
    // Sample sizes.
    let stsz = require(stbl, b"stsz", "stbl")?;
    let mut c = Cursor::new(slice(&stsz));
    c.take(4)?;
    let uniform = c.u32()?;
    let count = c.u32()? as usize;
    let sizes: Vec<u32> = if uniform == 0 {
        (0..count).map(|_| c.u32()).collect::<Result<_>>()?
    } else {
        vec![uniform; count]
    };

    // Chunk offsets.
    let offsets: Vec<u64> = if let Some(stco) = find(stbl, b"stco") {
        let mut c = Cursor::new(slice(&stco));
        c.take(4)?;
        let n = c.u32()? as usize;
        (0..n)
            .map(|_| c.u32().map(u64::from))
            .collect::<Result<_>>()?
    } else {
        let co64 = require(stbl, b"co64", "stbl")?;
        let mut c = Cursor::new(slice(&co64));
        c.take(4)?;
        let n = c.u32()? as usize;
        (0..n).map(|_| c.u64()).collect::<Result<_>>()?
    };

    // Sample-to-chunk runs: (first_chunk 1-based, samples_per_chunk).
    let stsc = require(stbl, b"stsc", "stbl")?;
    let mut c = Cursor::new(slice(&stsc));
    c.take(4)?;
    let n = c.u32()? as usize;
    let mut runs = Vec::with_capacity(n);
    for _ in 0..n {
        let first = c.u32()?;
        let per = c.u32()?;
        c.take(4)?; // sample description index
        runs.push((first, per));
    }

    let mut chunks = Vec::with_capacity(offsets.len());
    let mut next_sample = 0usize;
    for (i, offset) in offsets.iter().enumerate() {
        let chunk_no = i as u32 + 1;
        let per = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk_no)
            .map(|(_, per)| *per as usize)
            .ok_or_else(|| Error::InvalidMedia("chunk not covered by stsc".into()))?;
        let end = next_sample + per;
        let sample_sizes = sizes
            .get(next_sample..end)
            .ok_or_else(|| {
                Error::InvalidMedia("stsc references more samples than stsz has".into())
            })?
            .to_vec();
        next_sample = end;
        chunks.push(Chunk {
            offset: *offset,
            sample_sizes,
        });
    }
    if next_sample != sizes.len() {
        return Err(Error::InvalidMedia(format!(
            "sample table covers {next_sample} of {} samples",
            sizes.len()
        )));
    }
    Ok(chunks)
}

fn read_range(file: &mut File, start: u64, len: u64) -> Result<Vec<u8>> {
    let mut buf =
        vec![0u8; usize::try_from(len).map_err(|_| Error::InvalidMedia("box too large".into()))?];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut buf)?;
    Ok(buf)
}
