//! Minimal fragmented-MP4 reader for ffmpeg's `frag_keyframe+empty_moov+default_base_moof` output.
//!
//! The stream is `ftyp moov (moof mdat)*`. Every fragment starts on a video keyframe and is
//! self-contained, so any run of fragments appended to the init segment is a playable file.

use std::io::{self, Read};

pub struct Fragment {
    /// Seconds since the first fragment.
    pub start: f64,
    pub bytes: Vec<u8>,
}

pub struct Fmp4Reader<R: Read> {
    r: R,
    pub init: Vec<u8>,
    video_track: u32,
    timescale: u32,
    base: Option<u64>,
}

struct BoxData {
    kind: [u8; 4],
    bytes: Vec<u8>,
    header: usize,
}

fn read_box<R: Read>(r: &mut R) -> io::Result<Option<BoxData>> {
    let mut hdr = [0u8; 8];
    match r.read_exact(&mut hdr) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let mut size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
    let kind = [hdr[4], hdr[5], hdr[6], hdr[7]];
    let mut bytes = hdr.to_vec();
    let mut header = 8;
    if size == 1 {
        let mut ext = [0u8; 8];
        r.read_exact(&mut ext)?;
        size = u64::from_be_bytes(ext);
        bytes.extend_from_slice(&ext);
        header = 16;
    }
    if size < header as u64 || size > (1 << 32) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad mp4 box size"));
    }
    let body = size as usize - header;
    let at = bytes.len();
    bytes.resize(at + body, 0);
    r.read_exact(&mut bytes[at..])?;
    Ok(Some(BoxData { kind, bytes, header }))
}

/// Iterate the child boxes inside `body` as (type, payload) slices.
fn children(body: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        if pos + 8 > body.len() {
            return None;
        }
        let mut size = u32::from_be_bytes(body[pos..pos + 4].try_into().ok()?) as usize;
        let kind = &body[pos + 4..pos + 8];
        let mut hdr = 8;
        if size == 1 {
            size = u64::from_be_bytes(body.get(pos + 8..pos + 16)?.try_into().ok()?) as usize;
            hdr = 16;
        } else if size == 0 {
            size = body.len() - pos;
        }
        if size < hdr || pos + size > body.len() {
            return None;
        }
        let payload = &body[pos + hdr..pos + size];
        pos += size;
        Some((kind, payload))
    })
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be_u64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// (track_id, timescale) of the video track inside a moov payload.
fn parse_moov(moov: &[u8]) -> Option<(u32, u32)> {
    for (kind, trak) in children(moov) {
        if kind != b"trak" {
            continue;
        }
        let mut track_id = None;
        let mut timescale = None;
        let mut is_video = false;
        for (k, p) in children(trak) {
            match k {
                b"tkhd" => track_id = be_u32(p, if p[0] == 1 { 20 } else { 12 }),
                b"mdia" => {
                    for (mk, mp) in children(p) {
                        match mk {
                            b"mdhd" => timescale = be_u32(mp, if mp[0] == 1 { 20 } else { 12 }),
                            b"hdlr" => is_video = mp.get(8..12) == Some(b"vide"),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if is_video {
            return Some((track_id?, timescale?));
        }
    }
    None
}

/// baseMediaDecodeTime of `track` inside a moof payload.
fn parse_moof(moof: &[u8], track: u32) -> Option<u64> {
    for (kind, traf) in children(moof) {
        if kind != b"traf" {
            continue;
        }
        let mut id = None;
        let mut tfdt = None;
        for (k, p) in children(traf) {
            match k {
                b"tfhd" => id = be_u32(p, 4),
                b"tfdt" => tfdt = if p.first() == Some(&1) { be_u64(p, 4) } else { be_u32(p, 4).map(u64::from) },
                _ => {}
            }
        }
        if id == Some(track) {
            return tfdt;
        }
    }
    None
}

impl<R: Read> Fmp4Reader<R> {
    pub fn new(r: R) -> Self {
        Self { r, init: Vec::new(), video_track: 1, timescale: 0, base: None }
    }

    /// Read until the next complete moof+mdat pair. `Ok(None)` at end of stream.
    pub fn next_fragment(&mut self) -> io::Result<Option<Fragment>> {
        let mut moof: Option<(Vec<u8>, f64)> = None;
        while let Some(b) = read_box(&mut self.r)? {
            match &b.kind {
                b"moof" => {
                    let dts = parse_moof(&b.bytes[b.header..], self.video_track).unwrap_or(0);
                    let base = *self.base.get_or_insert(dts);
                    let start = dts.saturating_sub(base) as f64 / self.timescale.max(1) as f64;
                    moof = Some((b.bytes, start));
                }
                b"mdat" => {
                    if let Some((mut bytes, start)) = moof.take() {
                        bytes.extend_from_slice(&b.bytes);
                        return Ok(Some(Fragment { start, bytes }));
                    }
                }
                b"moov" => {
                    if let Some((track, ts)) = parse_moov(&b.bytes[b.header..]) {
                        self.video_track = track;
                        self.timescale = ts;
                    }
                    self.init.extend_from_slice(&b.bytes);
                }
                _ if moof.is_none() && self.base.is_none() => self.init.extend_from_slice(&b.bytes),
                _ => {}
            }
        }
        Ok(None)
    }
}
