//! Hex encoding + decoding helpers.
//!
//! Every pleme-io crate that round-trips raw bytes through a canonical
//! sexpr form (`substrate-forge::program::ProgramSource::Wasm`,
//! `weights-forge::storage::StorageKind::Inline`, etc.) needs the same
//! lowercase hex encode/decode pair. This module is the single copy —
//! consumers should `use iac_forge::hex;` rather than reimplementing.
//!
//! The encoding is lowercase (fixed) so content-hash comparisons across
//! emitters stay stable: `deadbeef` ≠ `DEADBEEF` as strings even though
//! they decode to the same bytes.

use crate::sexpr::SExprError;

/// Encode a byte slice as a lowercase hex string.
///
/// The output length is exactly `bytes.len() * 2`. The encoding is
/// deterministic: identical byte slices always produce the identical
/// string, regardless of platform.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(nibble(b >> 4));
        out.push(nibble(b & 0xF));
    }
    out
}

/// Decode a lowercase (or mixed-case) hex string into bytes.
///
/// Returns `SExprError::Parse` on:
/// - odd-length input (can't chunk into byte pairs),
/// - non-ASCII bytes,
/// - non-hex digits.
///
/// The empty string decodes to an empty `Vec<u8>` — valid degenerate.
pub fn decode(s: &str) -> Result<Vec<u8>, SExprError> {
    if s.len() % 2 != 0 {
        return Err(SExprError::Parse(
            "hex string must have even length".into(),
        ));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk)
            .map_err(|_| SExprError::Parse("non-ascii in hex".into()))?;
        let byte = u8::from_str_radix(pair, 16)
            .map_err(|e| SExprError::Parse(format!("bad hex: {e}")))?;
        out.push(byte);
    }
    Ok(out)
}

/// Map a 0..=15 nibble to its lowercase hex character. Also used by
/// `sexpr::ContentHash::to_hex`.
pub(crate) fn nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_empty_is_empty() {
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn encode_is_lowercase() {
        // Pinned: a future uppercase refactor would break every
        // content-hash string comparison across crates.
        assert_eq!(encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn encode_length_is_twice_input() {
        for n in 0..16 {
            let bytes: Vec<u8> = (0..n).collect();
            assert_eq!(encode(&bytes).len(), n as usize * 2);
        }
    }

    #[test]
    fn decode_empty_is_empty_vec() {
        assert!(decode("").unwrap().is_empty());
    }

    #[test]
    fn decode_rejects_odd_length() {
        let err = decode("abc").unwrap_err();
        assert!(format!("{err:?}").contains("even length"));
    }

    #[test]
    fn decode_rejects_non_ascii() {
        // Non-ASCII branch: chunk into 2-byte windows such that the
        // window breaks up a multi-byte UTF-8 char, leaving the lead
        // byte with no continuation in its chunk. "aña" = 4 bytes:
        // [0x61, 0xc3, 0xb1, 0x61]. Chunks [0x61, 0xc3] and
        // [0xb1, 0x61] — the first chunk ends mid-"ñ" and fails
        // str::from_utf8, hitting the "non-ascii" branch (rather than
        // the "bad hex" branch that a valid-UTF-8 chunk would hit).
        let err = decode("aña").unwrap_err();
        assert!(
            format!("{err:?}").contains("non-ascii"),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn decode_rejects_non_hex_digits() {
        let err = decode("ag").unwrap_err();
        assert!(format!("{err:?}").contains("bad hex"));
    }

    #[test]
    fn decode_accepts_uppercase_mixed_case() {
        // `u8::from_str_radix(_, 16)` accepts both cases even though
        // the encoder only emits lowercase — pin the asymmetry so
        // round-tripping an externally-produced uppercase string
        // continues to work.
        assert_eq!(decode("DEADBEEF").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(decode("DeadBeef").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn roundtrip_every_byte_value() {
        // Round-trip all 256 single-byte values individually.
        for b in 0u8..=255u8 {
            let enc = encode(&[b]);
            assert_eq!(enc.len(), 2);
            let dec = decode(&enc).unwrap();
            assert_eq!(dec, vec![b], "failed for byte 0x{b:02x}");
        }
    }

    #[test]
    fn roundtrip_larger_buffer() {
        let original: Vec<u8> = (0..=255).chain((0..=255).rev()).collect();
        let enc = encode(&original);
        assert_eq!(enc.len(), original.len() * 2);
        let dec = decode(&enc).unwrap();
        assert_eq!(dec, original);
    }

    #[test]
    fn nibble_boundaries() {
        assert_eq!(nibble(0), '0');
        assert_eq!(nibble(9), '9');
        assert_eq!(nibble(10), 'a');
        assert_eq!(nibble(15), 'f');
    }

    #[test]
    #[should_panic(expected = "nibble must be 0..=15")]
    fn nibble_panics_above_range() {
        // unreachable!() must panic — encode() gates inputs via bit-
        // masks, but an internal misuse should surface loudly instead
        // of producing garbage characters.
        let _ = nibble(16);
    }
}
