//! AAX file-key derivation from activation bytes and the `adrm` box.
//!
//! Port of `mov_read_adrm` in FFmpeg's `libavformat/mov.c` (also used by
//! AAXClean). The fixed key was extracted from Audible's SDK by the FFmpeg
//! developers.

use sha1::{Digest, Sha1};

use super::keys::ActivationBytes;
use crate::crypto::aes128_cbc_decrypt_blocks;
use crate::error::{Error, Result};

/// FFmpeg's `audible_fixed_key` default.
const FIXED_KEY: [u8; 16] = [
    0x77, 0x21, 0x4d, 0x4b, 0x19, 0x6a, 0x87, 0xcd, 0x52, 0x00, 0x45, 0xfd, 0x20, 0xa5, 0x1d, 0x67,
];

/// `adrm` payload layout: 8 bytes, 56-byte DRM blob, 4 bytes, 20-byte checksum.
const BLOB: std::ops::Range<usize> = 8..64;
const CHECKSUM: std::ops::Range<usize> = 68..88;

fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// Derive the per-file AES key and IV from activation bytes and the payload
/// of the file's `adrm` box.
pub(crate) fn derive_file_key(ab: &ActivationBytes, adrm: &[u8]) -> Result<([u8; 16], [u8; 16])> {
    if adrm.len() < CHECKSUM.end {
        return Err(Error::InvalidMedia("adrm box is too short".into()));
    }
    let ab = &ab.0;
    let ik = sha1(&[&FIXED_KEY, ab]);
    let iiv = sha1(&[&FIXED_KEY, &ik, ab]);
    if sha1(&[&ik[..16], &iiv[..16]])[..] != adrm[CHECKSUM] {
        return Err(Error::Crypto(
            "activation bytes do not match this file (adrm checksum mismatch)".into(),
        ));
    }
    let mut key = [0u8; 16];
    let mut iv = [0u8; 16];
    key.copy_from_slice(&ik[..16]);
    iv.copy_from_slice(&iiv[..16]);

    // Only the first 48 bytes (3 blocks) of the 56-byte blob are decrypted.
    let mut out = [0u8; 48];
    out.copy_from_slice(&adrm[BLOB][..48]);
    aes128_cbc_decrypt_blocks(&key, &iv, &mut out);
    if (0..4).any(|i| ab[i] != out[3 - i]) {
        return Err(Error::Crypto(
            "adrm blob did not decrypt with these activation bytes".into(),
        ));
    }
    let mut file_key = [0u8; 16];
    file_key.copy_from_slice(&out[8..24]);
    let digest = sha1(&[&out[26..42], &file_key, &FIXED_KEY]);
    let mut file_iv = [0u8; 16];
    file_iv.copy_from_slice(&digest[..16]);
    Ok((file_key, file_iv))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `adrm` payload generated independently with Python hashlib + `openssl
    /// enc` following FFmpeg's algorithm, for activation bytes `1ceb00da`,
    /// file key `00112233…eeff` and IV seed `a0a1…af`.
    pub(crate) const ADRM_PAYLOAD: &str = "0000000000000000cf06cbb2760f5e8f92f15bf775c6ed677712bcdcdeac1b60\
        ac017775f23e0d54edfcb9b6ddefda2eececb566a1279716eeeeeeeeeeeeeeee000000007b19e237cd6eef8770b3\
        0a93fe165070ab199e540000000000000000";

    #[test]
    fn known_answer_derivation() {
        let adrm = hex::decode(ADRM_PAYLOAD).unwrap();
        let ab: ActivationBytes = "1ceb00da".parse().unwrap();
        let (key, iv) = derive_file_key(&ab, &adrm).unwrap();
        assert_eq!(hex::encode(key), "00112233445566778899aabbccddeeff");
        assert_eq!(hex::encode(iv), "af9a572275b6b1d5830d63c9aebb1189");
    }

    #[test]
    fn wrong_activation_bytes_fail_checksum() {
        let adrm = hex::decode(ADRM_PAYLOAD).unwrap();
        let ab: ActivationBytes = "deadbeef".parse().unwrap();
        assert!(matches!(derive_file_key(&ab, &adrm), Err(Error::Crypto(_))));
        assert!(derive_file_key(&ab, &adrm[..40]).is_err());
    }
}
