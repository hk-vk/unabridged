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

mod keys;

pub use keys::{ActivationBytes, DecryptionKey};
