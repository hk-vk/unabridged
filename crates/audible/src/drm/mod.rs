//! Audible DRM: key material and conversion of encrypted files to playable
//! M4B/M4A.
//!
//! Audible's "Adrm" scheme stores AAC audio in an MP4 container whose sample
//! entry is renamed `aavd`. Each audio sample is AES-128-CBC encrypted with a
//! per-file key and IV (IV reset per sample, trailing `len % 16` bytes left in
//! clear). The two flavours differ only in where the key comes from:
//!
//! * **AAX**: key and IV are derived from the account's 4 activation bytes and
//!   the file's `adrm` box ([`aax`]).
//! * **AAXC**: key and IV arrive in the license voucher ([`DecryptionKey::Aaxc`]).
//!
//! Widevine/DASH titles are not supported; the license step reports them as
//! [`crate::Error::UnsupportedDrm`].

mod aax;
pub(crate) mod convert;
mod decrypter;
mod keys;
mod mp4;

pub use decrypter::{Decrypter, FfmpegDecrypter, NativeDecrypter};
pub use keys::{ActivationBytes, DecryptionKey};

/// Kind of Audible audio file, detected from its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Encrypted with an `adrm` box: needs activation bytes.
    Aax,
    /// Encrypted without an `adrm` box: needs the voucher key.
    Aaxc,
    /// Not encrypted (plain M4A/M4B).
    Plain,
}

/// Inspect a file to see whether and how it is encrypted. Blocking.
pub fn detect(path: &std::path::Path) -> crate::Result<FileKind> {
    let layout = mp4::parse(&mut std::fs::File::open(path)?)?;
    Ok(match layout.tracks.first() {
        None => FileKind::Plain,
        Some(t) if t.adrm.is_some() => FileKind::Aax,
        Some(_) => FileKind::Aaxc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let plain = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sine.m4a"
        ));
        assert_eq!(detect(plain).unwrap(), FileKind::Plain);

        let aaxc = dir.path().join("a.aaxc");
        std::fs::write(
            &aaxc,
            convert::tests::make_encrypted(dir.path(), None, &[1; 16], &[2; 16]),
        )
        .unwrap();
        assert_eq!(detect(&aaxc).unwrap(), FileKind::Aaxc);

        let payload = hex::decode(aax::tests::ADRM_PAYLOAD).unwrap();
        let aax = dir.path().join("a.aax");
        std::fs::write(
            &aax,
            convert::tests::make_encrypted(dir.path(), Some(&payload), &[1; 16], &[2; 16]),
        )
        .unwrap();
        assert_eq!(detect(&aax).unwrap(), FileKind::Aax);
    }
}
