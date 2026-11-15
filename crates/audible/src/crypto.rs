//! Small crypto helpers shared by voucher and audio decryption.

use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, generic_array::GenericArray};

/// AES block size in bytes.
pub(crate) const BLOCK: usize = 16;

/// AES-128-CBC decrypt the leading whole blocks of `data` in place with no
/// padding. Trailing bytes (`len % 16`) are left untouched, which is exactly
/// how Audible encrypts audio samples.
pub(crate) fn aes128_cbc_decrypt_blocks(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) {
    let whole = data.len() - data.len() % BLOCK;
    if whole == 0 {
        return;
    }
    let mut dec = cbc::Decryptor::<Aes128>::new(key.into(), iv.into());
    for chunk in data[..whole].chunks_exact_mut(BLOCK) {
        dec.decrypt_block_mut(GenericArray::from_mut_slice(chunk));
    }
}

/// Parse a 32-character hex string into 16 bytes.
pub(crate) fn hex16(value: &str) -> Option<[u8; 16]> {
    let bytes = hex::decode(value.trim()).ok()?;
    bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NIST SP 800-38A, F.2.2 CBC-AES128.Decrypt (first two blocks).
    #[test]
    fn nist_sp800_38a_cbc_aes128() {
        let key = hex16("2b7e151628aed2a6abf7158809cf4f3c").unwrap();
        let iv = hex16("000102030405060708090a0b0c0d0e0f").unwrap();
        let mut data =
            hex::decode("7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b2")
                .unwrap();
        aes128_cbc_decrypt_blocks(&key, &iv, &mut data);
        assert_eq!(
            hex::encode(&data),
            "6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51"
        );
    }

    #[test]
    fn leaves_trailing_partial_block_in_clear() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        let mut short = vec![1, 2, 3];
        aes128_cbc_decrypt_blocks(&key, &iv, &mut short);
        assert_eq!(short, [1, 2, 3]);

        let mut data = vec![0u8; 16 + 5];
        data[16..].copy_from_slice(&[5, 4, 3, 2, 1]);
        aes128_cbc_decrypt_blocks(&key, &iv, &mut data);
        assert_eq!(&data[16..], &[5, 4, 3, 2, 1]);
    }

    #[test]
    fn hex16_validates_length() {
        assert!(hex16("00").is_none());
        assert!(hex16("zz".repeat(16).as_str()).is_none());
        assert_eq!(hex16(&"ab".repeat(16)), Some([0xab; 16]));
    }
}
