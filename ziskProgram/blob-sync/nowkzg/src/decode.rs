//! EIP-4844 field-element decoding.
//!
//! Each blob is exactly 4096 BLS12-381 field elements, each serialized as 32
//! big-endian bytes. The BLS modulus is 255 bits, so the highest byte of every
//! field element is `0x00` for any valid blob produced by a standards-
//! compliant signer. Stripping that byte recovers a 4096 × 31 = 126_976 byte
//! payload — the actual rollup data the batcher embedded.
//!
//! NOTE: this is *not* OP-stack frame decoding. OP rollups wrap the resulting
//! payload in their own frame format (version byte, length prefix, channel
//! framing). Plug that on top if you want batches; for raw blob inspection
//! this is enough.

pub const BLOB_BYTES: usize = 131_072; // 4096 * 32
pub const FIELD_ELEMENTS_PER_BLOB: usize = 4096;
pub const PAYLOAD_BYTES: usize = FIELD_ELEMENTS_PER_BLOB * 31; // 126_976

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("field element {index} has non-zero high byte 0x{byte:02x}; not a canonical blob")]
    NonCanonicalFieldElement { index: usize, byte: u8 },
}

/// Strip the leading `0x00` byte from each 32-byte field element.
///
/// Returns the 126_976-byte concatenated payload. Errors if any field element
/// has a non-zero high byte (which would indicate the data isn't a real blob,
/// e.g. raw user-supplied bytes that haven't been canonicalised).
pub fn decode_field_elements(blob: &[u8; BLOB_BYTES]) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::with_capacity(PAYLOAD_BYTES);
    for (i, fe) in blob.chunks_exact(32).enumerate() {
        if fe[0] != 0 {
            return Err(DecodeError::NonCanonicalFieldElement { index: i, byte: fe[0] });
        }
        out.extend_from_slice(&fe[1..32]);
    }
    debug_assert_eq!(out.len(), PAYLOAD_BYTES);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_zero_blob() {
        let blob = Box::new([0u8; BLOB_BYTES]);
        let out = decode_field_elements(&blob).unwrap();
        assert_eq!(out.len(), PAYLOAD_BYTES);
        assert!(out.iter().all(|b| *b == 0));
    }

    #[test]
    fn rejects_non_canonical() {
        let mut blob = Box::new([0u8; BLOB_BYTES]);
        blob[0] = 0x01;
        let err = decode_field_elements(&blob).unwrap_err();
        match err {
            DecodeError::NonCanonicalFieldElement { index, byte } => {
                assert_eq!(index, 0);
                assert_eq!(byte, 0x01);
            }
        }
    }

    #[test]
    fn extracts_31_bytes_per_fe() {
        let mut blob = Box::new([0u8; BLOB_BYTES]);
        // FE 0: high byte zero, then 31 bytes of pattern.
        for i in 1..32 {
            blob[i] = i as u8;
        }
        let out = decode_field_elements(&blob).unwrap();
        for i in 0..31 {
            assert_eq!(out[i], (i + 1) as u8);
        }
    }
}
