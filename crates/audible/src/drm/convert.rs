//! Pure-Rust conversion of AAX/AAXC to a playable M4B.
//!
//! The output is a byte-for-byte copy of the input with every audio sample
//! decrypted in place and a handful of same-size patches, so no offsets move:
//!
//! * sample entry `aavd` → `mp4a` (as AAXClean does);
//! * `adrm` / `aabd` DRM boxes inside the sample entry → `free`;
//! * `ftyp` brands `aax ` / `aaxc` → `M4B `.
//!
//! Chapters (the text track), cover art and iTunes metadata are untouched.
//! On macOS `std::fs::copy` clones the file (APFS), so the copy is cheap.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use tracing::{debug, info};

use super::keys::DecryptionKey;
use super::{aax, mp4};
use crate::crypto::aes128_cbc_decrypt_blocks;
use crate::download::{Progress, ProgressCallback};
use crate::error::{Error, Result};

const PROGRESS_STEP: u64 = 4 * 1024 * 1024;

/// Decrypt `input` into `output`. Blocking; call from `spawn_blocking`.
/// Returns the number of audio bytes decrypted.
pub(crate) fn decrypt_file(
    input: &Path,
    output: &Path,
    key: &DecryptionKey,
    progress: Option<&ProgressCallback>,
) -> Result<u64> {
    let layout = mp4::parse(&mut File::open(input)?)?;
    if layout.tracks.is_empty() {
        return Err(Error::InvalidMedia(
            "no encrypted (aavd) audio track; not an AAX/AAXC file".into(),
        ));
    }
    let keys = layout
        .tracks
        .iter()
        .map(|t| track_key(t, key))
        .collect::<Result<Vec<_>>>()?;

    let tmp = tmp_path(output);
    std::fs::copy(input, &tmp)?;
    let result = patch_and_decrypt(&tmp, &layout, &keys, progress);
    match result {
        Ok(bytes) => {
            std::fs::rename(&tmp, output)?;
            info!(bytes, path = %output.display(), "decrypted audiobook");
            Ok(bytes)
        }
        Err(e) => {
            std::fs::remove_file(&tmp).ok();
            Err(e)
        }
    }
}

fn track_key(track: &mp4::EncryptedTrack, key: &DecryptionKey) -> Result<([u8; 16], [u8; 16])> {
    match key {
        DecryptionKey::Aaxc { key, iv } => Ok((*key, *iv)),
        DecryptionKey::Aax(ab) => {
            let adrm = track.adrm.as_deref().ok_or_else(|| {
                Error::InvalidMedia("file has no adrm box; use the AAXC voucher key".into())
            })?;
            aax::derive_file_key(ab, adrm)
        }
    }
}

fn patch_and_decrypt(
    path: &Path,
    layout: &mp4::Layout,
    keys: &[([u8; 16], [u8; 16])],
    progress: Option<&ProgressCallback>,
) -> Result<u64> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;

    if let Some(ftyp) = layout.ftyp {
        let mut payload = layout.ftyp_payload.clone();
        let mut changed = false;
        for (i, brand) in payload.chunks_exact_mut(4).enumerate() {
            // Index 1 is the minor version, not a brand.
            if i != 1 && brand.starts_with(b"aax") {
                brand.copy_from_slice(b"M4B ");
                changed = true;
            }
        }
        if changed {
            write_at(&mut f, ftyp.payload_start(), &payload)?;
        }
    }

    let total: u64 = layout
        .tracks
        .iter()
        .flat_map(|t| t.chunks.iter())
        .map(mp4::Chunk::len)
        .sum();
    let mut done = 0u64;
    let mut next_report = 0u64;
    let mut buf = Vec::new();
    for (track, (key, iv)) in layout.tracks.iter().zip(keys) {
        write_at(&mut f, track.sample_entry.start + 4, b"mp4a")?;
        for child in &track.entry_children {
            if &child.kind == b"adrm" || &child.kind == b"aabd" {
                write_at(&mut f, child.start + 4, b"free")?;
            }
        }
        for chunk in &track.chunks {
            let len = usize::try_from(chunk.len())
                .map_err(|_| Error::InvalidMedia("chunk too large".into()))?;
            buf.resize(len, 0);
            f.seek(SeekFrom::Start(chunk.offset))?;
            f.read_exact(&mut buf)?;
            let mut pos = 0usize;
            for size in &chunk.sample_sizes {
                let end = pos + *size as usize;
                aes128_cbc_decrypt_blocks(key, iv, &mut buf[pos..end]);
                pos = end;
            }
            write_at(&mut f, chunk.offset, &buf)?;
            done += chunk.len();
            if let Some(cb) = progress
                && done >= next_report
            {
                cb(Progress {
                    downloaded: done,
                    total: Some(total),
                });
                next_report = done + PROGRESS_STEP;
            }
        }
    }
    f.sync_all()?;
    if let Some(cb) = progress {
        cb(Progress {
            downloaded: done,
            total: Some(total),
        });
    }
    debug!(
        tracks = layout.tracks.len(),
        bytes = done,
        "samples decrypted"
    );
    Ok(done)
}

fn write_at(f: &mut File, offset: u64, data: &[u8]) -> Result<()> {
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(data)?;
    Ok(())
}

fn tmp_path(output: &Path) -> PathBuf {
    let mut name = output
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    output.with_file_name(name)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aes::Aes128;
    use aes::cipher::{BlockEncryptMut, KeyIvInit, generic_array::GenericArray};

    use crate::drm::ActivationBytes;

    pub(crate) const AAX_KEY: &str = "00112233445566778899aabbccddeeff";
    pub(crate) const AAX_IV: &str = "af9a572275b6b1d5830d63c9aebb1189";

    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sine.m4a"
        ))
        .unwrap()
    }

    fn encrypt_blocks(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) {
        let whole = data.len() - data.len() % 16;
        let mut enc = cbc::Encryptor::<Aes128>::new(key.into(), iv.into());
        for block in data[..whole].chunks_exact_mut(16) {
            enc.encrypt_block_mut(GenericArray::from_mut_slice(block));
        }
    }

    fn parse_bytes(bytes: &[u8], dir: &Path) -> mp4::Layout {
        let p = dir.join("probe.mp4");
        std::fs::write(&p, bytes).unwrap();
        mp4::parse(&mut File::open(&p).unwrap()).unwrap()
    }

    fn put_u32(buf: &mut [u8], at: u64, v: u32) {
        buf[at as usize..at as usize + 4].copy_from_slice(&v.to_be_bytes());
    }

    fn get_u32(buf: &[u8], at: u64) -> u32 {
        u32::from_be_bytes(buf[at as usize..at as usize + 4].try_into().unwrap())
    }

    /// Turn the plain `sine.m4a` fixture into an Audible-style encrypted file:
    /// rename `mp4a` to `aavd`, optionally append an `adrm` box to the sample
    /// entry (fixing up ancestor sizes; `moov` follows `mdat`, so no chunk
    /// offsets move), set the `aax `/`aaxc` brand and encrypt every sample.
    pub(crate) fn make_encrypted(
        dir: &Path,
        adrm_payload: Option<&[u8]>,
        key: &[u8; 16],
        iv: &[u8; 16],
    ) -> Vec<u8> {
        let mut bytes = fixture();
        // Rename the sample entry so the parser sees it as encrypted.
        let entry_type = bytes.windows(4).position(|w| w == b"mp4a").unwrap();
        bytes[entry_type..entry_type + 4].copy_from_slice(b"aavd");
        let layout = parse_bytes(&bytes, dir);
        let track = &layout.tracks[0];

        if let Some(payload) = adrm_payload {
            let mut adrm_box = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
            adrm_box.extend_from_slice(b"adrm");
            adrm_box.extend_from_slice(payload);
            let insert_at = track.sample_entry.end();
            let grow = adrm_box.len() as u32;
            for b in track
                .ancestors
                .iter()
                .chain(std::iter::once(&track.sample_entry))
            {
                let size = get_u32(&bytes, b.start);
                put_u32(&mut bytes, b.start, size + grow);
            }
            let tail = bytes.split_off(insert_at as usize);
            bytes.extend_from_slice(&adrm_box);
            bytes.extend_from_slice(&tail);
        }
        // Real files carry these major brands; FFmpeg keys its AAXC
        // handling off `aaxc`.
        let brand: &[u8; 4] = if adrm_payload.is_some() {
            b"aax "
        } else {
            b"aaxc"
        };
        bytes[8..12].copy_from_slice(brand);

        let layout = parse_bytes(&bytes, dir);
        for chunk in &layout.tracks[0].chunks {
            let mut pos = chunk.offset as usize;
            for size in &chunk.sample_sizes {
                encrypt_blocks(key, iv, &mut bytes[pos..pos + *size as usize]);
                pos += *size as usize;
            }
        }
        bytes
    }

    #[test]
    fn aaxc_round_trip_restores_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let key = [0x11u8; 16];
        let iv = [0x22u8; 16];
        let encrypted = make_encrypted(dir.path(), None, &key, &iv);
        assert_ne!(encrypted, fixture());
        let input = dir.path().join("book.aaxc");
        let output = dir.path().join("book.m4b");
        std::fs::write(&input, &encrypted).unwrap();

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        let cb: ProgressCallback = std::sync::Arc::new(move |p: Progress| {
            assert!(p.downloaded <= p.total.unwrap());
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let bytes =
            decrypt_file(&input, &output, &DecryptionKey::Aaxc { key, iv }, Some(&cb)).unwrap();
        assert!(bytes > 7000);
        assert!(calls.load(std::sync::atomic::Ordering::Relaxed) >= 2);
        let mut out = std::fs::read(&output).unwrap();
        assert_eq!(&out[8..12], b"M4B ");
        out[8..12].copy_from_slice(b"M4A ");
        assert_eq!(
            out,
            fixture(),
            "decrypt(encrypt(x)) == x apart from the brand"
        );
        assert!(!tmp_path(&output).exists());
    }

    #[test]
    fn aax_round_trip_with_adrm_known_answer() {
        let dir = tempfile::tempdir().unwrap();
        let payload = hex::decode(crate::drm::aax::tests::ADRM_PAYLOAD).unwrap();
        let key = crate::crypto::hex16(AAX_KEY).unwrap();
        let iv = crate::crypto::hex16(AAX_IV).unwrap();
        let encrypted = make_encrypted(dir.path(), Some(&payload), &key, &iv);
        let input = dir.path().join("book.aax");
        let output = dir.path().join("book.m4b");
        std::fs::write(&input, &encrypted).unwrap();

        let ab: ActivationBytes = "1ceb00da".parse().unwrap();
        decrypt_file(&input, &output, &DecryptionKey::Aax(ab), None).unwrap();
        let out = std::fs::read(&output).unwrap();
        assert_eq!(out.len(), encrypted.len(), "same-size rewrite");
        assert_eq!(&out[8..12], b"M4B ");
        assert!(out.windows(4).any(|w| w == b"mp4a"));
        assert!(!out.windows(4).any(|w| w == b"adrm" || w == b"aavd"));

        // Audio samples match the original plain file.
        let plain = parse_bytes(
            &{
                let mut b = fixture();
                let i = b.windows(4).position(|w| w == b"mp4a").unwrap();
                b[i..i + 4].copy_from_slice(b"aavd");
                b
            },
            dir.path(),
        );
        let orig = fixture();
        for chunk in &plain.tracks[0].chunks {
            let s = chunk.offset as usize;
            let e = s + chunk.len() as usize;
            assert_eq!(out[s..e], orig[s..e]);
        }
    }

    #[test]
    fn wrong_key_is_rejected_for_aax_and_plain_files() {
        let dir = tempfile::tempdir().unwrap();
        let payload = hex::decode(crate::drm::aax::tests::ADRM_PAYLOAD).unwrap();
        let encrypted = make_encrypted(dir.path(), Some(&payload), &[0; 16], &[0; 16]);
        let input = dir.path().join("book.aax");
        std::fs::write(&input, &encrypted).unwrap();
        let output = dir.path().join("out.m4b");
        let err = decrypt_file(
            &input,
            &output,
            &DecryptionKey::Aax("deadbeef".parse().unwrap()),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Crypto(_)), "{err:?}");
        assert!(!output.exists());

        let plain = dir.path().join("plain.m4a");
        std::fs::write(&plain, fixture()).unwrap();
        let err = decrypt_file(
            &plain,
            &output,
            &DecryptionKey::Aaxc {
                key: [0; 16],
                iv: [0; 16],
            },
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidMedia(_)), "{err:?}");
    }
}
