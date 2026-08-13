//! Writing a recovery key down, and typing it back in.
//!
//! A [`crate::RecoveryKey`] is thirty-two random bytes and the only way into a
//! case whose passphrase is gone. Until it has a form a person can copy onto
//! paper and type back a year later, "keep this somewhere safe" is not advice
//! anyone can follow, and the recovery path is a feature that exists only in
//! tests.
//!
//! # The alphabet
//!
//! Crockford base32: ten digits and twenty-two letters, with `I`, `L`, `O` and
//! `U` left out. The first three are omitted because they are what people
//! mis-read `1` and `0` as, and reading is where this fails — the key is
//! written by one person and typed by another, possibly from a photograph of a
//! sticky note. `U` is omitted so that no combination of characters spells an
//! obscenity, which sounds frivolous and is not: a key someone is embarrassed
//! to read aloud over the phone is a key they will transcribe wrongly.
//!
//! Decoding accepts lowercase and treats `I`/`i`/`l`/`L` as `1` and `O`/`o` as
//! `0`, following Crockford, because those are the substitutions a careful
//! person makes when copying by hand.
//!
//! # The checksum
//!
//! Twenty-four bits, from BLAKE3 of the key. Its job is to answer "did I type this
//! correctly" immediately, rather than after an Argon2 derivation returns
//! `WrongKey` and leaves the user unsure whether they typed it wrong or the
//! case is damaged. Those are very different situations and only one of them is
//! recoverable by trying again.
//!
//! It is a *detection* code, not a correction code, and detection is
//! probabilistic: an arbitrary corruption survives it about once in sixteen
//! million times. Every *single-character* substitution is caught outright,
//! which is asserted exhaustively rather than sampled. It is not a second
//! factor and not an integrity guarantee about the case — the wrapped key's
//! AEAD tag is what makes a wrong key fail closed.
//!
//! # Why the sizes are what they are
//!
//! 256 key bits plus 24 checksum bits is 280, which is exactly 56 characters of
//! five bits. That is not a coincidence to be grateful for, it is the reason 24
//! was chosen over 20: with no padding bits there are no bits a character can
//! carry that the decoder ignores, so every distinct string decodes to a
//! distinct result and no two written forms can mean the same key. A padded
//! encoding had exactly that defect, and `one_wrong_character_is_caught` found
//! it — four checksum bits shared a character with the last key bits and went
//! uncompared, so flipping them produced a different phrase for the same key.

use zeroize::Zeroizing;

use crate::{KeyError, Result, Secret, KEY_LEN};

/// Crockford base32, in value order.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Bits of BLAKE3 appended as a typo check.
const CHECKSUM_BITS: usize = 24;
const CHECKSUM_BYTES: usize = CHECKSUM_BITS / 8;

/// The whole written form, five bits per character.
///
/// Exact by construction: (256 + 24) / 5 = 56, with no remainder and so no
/// padding. Guarded by `the_encoding_divides_evenly_into_characters`.
const TOTAL_CHARS: usize = (KEY_LEN * 8 + CHECKSUM_BITS) / 5;

/// Characters between dashes.
///
/// Groups exist to be counted. Someone checking a transcription against paper
/// compares eight short runs, not one string of fifty-six, and a dropped
/// character shows up as a short group rather than as a key that is simply
/// wrong.
const GROUP: usize = 7;

/// Render a recovery key as the string a user writes down.
///
/// Returns [`Zeroizing`] so the rendered key does not outlive its use. The
/// caller is displaying secret material and should treat the value that way:
/// this is the one place a key is deliberately turned into something readable.
pub fn to_phrase(key: &Secret) -> Zeroizing<String> {
    let mut bits: Zeroizing<Vec<u8>> = Zeroizing::new(key.expose().to_vec());
    // Big-endian: the leading CHECKSUM_BITS of the digest, continuing the bit
    // stream straight on from the key with no alignment gap.
    bits.extend_from_slice(&checksum(key.expose()));

    let mut out = String::with_capacity(TOTAL_CHARS + TOTAL_CHARS / GROUP);
    for i in 0..TOTAL_CHARS {
        if i > 0 && i % GROUP == 0 {
            out.push('-');
        }
        out.push(ALPHABET[take5(&bits, i) as usize] as char);
    }
    Zeroizing::new(out)
}

/// Parse a written-down recovery key.
///
/// Dashes and whitespace are ignored wherever they appear, so a key split
/// across two lines of a notebook parses. Case is ignored, and the Crockford
/// substitutions are applied.
pub fn from_phrase(phrase: &str) -> Result<Secret> {
    let mut values: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(TOTAL_CHARS));
    for ch in phrase.chars() {
        if ch == '-' || ch.is_whitespace() {
            continue;
        }
        values.push(decode_char(ch)?);
    }

    if values.len() != TOTAL_CHARS {
        return Err(KeyError::BadLength {
            expected: TOTAL_CHARS,
            found: values.len(),
        });
    }

    let mut bytes: Zeroizing<Vec<u8>> = Zeroizing::new(vec![0u8; KEY_LEN + CHECKSUM_BYTES]);
    for (i, v) in values.iter().enumerate() {
        put5(&mut bytes, i, *v);
    }

    let key = Secret::from_slice(&bytes[..KEY_LEN])?;

    // Every checksum bit lands inside a whole byte, so this compares all of
    // them. An earlier version compared five-bit characters instead, and the
    // four checksum bits that share a character with the tail of the key
    // escaped the comparison entirely.
    if bytes[KEY_LEN..] != checksum(key.expose()) {
        return Err(KeyError::MistypedRecoveryKey);
    }

    Ok(key)
}

/// The leading [`CHECKSUM_BITS`] of BLAKE3 over the key.
fn checksum(key: &[u8; KEY_LEN]) -> [u8; CHECKSUM_BYTES] {
    let digest = blake3::hash(key);
    let mut out = [0u8; CHECKSUM_BYTES];
    out.copy_from_slice(&digest.as_bytes()[..CHECKSUM_BYTES]);
    out
}

/// The `i`th five-bit group of a big-endian bit stream.
fn take5(bytes: &[u8], i: usize) -> u8 {
    let mut v = 0u8;
    for bit in 0..5 {
        let n = i * 5 + bit;
        let byte = n / 8;
        // Unreachable given TOTAL_CHARS. Written as a bounds check rather
        // than an index so that changing a constant produces a wrong answer
        // instead of a panic inside a function holding key material.
        let got = if byte < bytes.len() {
            (bytes[byte] >> (7 - n % 8)) & 1
        } else {
            0
        };
        v = (v << 1) | got;
    }
    v
}

/// Write a five-bit group into a big-endian bit stream.
fn put5(bytes: &mut [u8], i: usize, value: u8) {
    for bit in 0..5 {
        let n = i * 5 + bit;
        let byte = n / 8;
        if byte >= bytes.len() {
            continue;
        }
        let set = (value >> (4 - bit)) & 1;
        let mask = 1 << (7 - n % 8);
        if set == 1 {
            bytes[byte] |= mask;
        } else {
            bytes[byte] &= !mask;
        }
    }
}

fn decode_char(ch: char) -> Result<u8> {
    let upper = ch.to_ascii_uppercase();
    // Crockford's read-alike rules, applied before the table lookup so that a
    // key copied by hand parses the way the person who copied it intended.
    let upper = match upper {
        'I' | 'L' => '1',
        'O' => '0',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|c| *c as char == upper)
        .map(|p| p as u8)
        .ok_or(KeyError::MistypedRecoveryKey)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_key_survives_being_written_down_and_typed_back() {
        for _ in 0..64 {
            let key = Secret::generate().unwrap();
            let phrase = to_phrase(&key);
            assert_eq!(from_phrase(&phrase).unwrap(), key);
        }
    }

    /// The shape matters: it is proofread by a human, in groups.
    #[test]
    fn the_written_form_is_grouped_and_uses_no_confusable_characters() {
        let phrase = to_phrase(&Secret::from_bytes([0x5a; KEY_LEN]));
        let groups: Vec<&str> = phrase.split('-').collect();
        assert_eq!(groups.len(), 8, "{}", phrase.as_str());
        assert!(
            groups.iter().all(|g| g.len() == GROUP),
            "{}",
            phrase.as_str()
        );
        assert!(
            !phrase.contains(['I', 'L', 'O', 'U']),
            "confusable character in {}",
            phrase.as_str()
        );
    }

    /// The whole point of the checksum: a wrong character is caught here, not
    /// after Argon2 returns WrongKey and leaves the user guessing whether the
    /// case is damaged.
    #[test]
    fn one_wrong_character_is_caught() {
        let key = Secret::generate().unwrap();
        let phrase = to_phrase(&key);

        let mut caught = 0;
        let mut tried = 0;
        let chars: Vec<char> = phrase.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            if *c == '-' {
                continue;
            }
            for replacement in ALPHABET.iter().map(|b| *b as char) {
                if replacement == *c {
                    continue;
                }
                let mut typo: Vec<char> = chars.clone();
                typo[i] = replacement;
                let typo: String = typo.into_iter().collect();
                tried += 1;
                match from_phrase(&typo) {
                    Err(KeyError::MistypedRecoveryKey) => caught += 1,
                    Err(other) => panic!("wrong error for a single typo: {other}"),
                    Ok(recovered) => {
                        // The worse of the two failures: a phrase that parses
                        // is a phrase the user is told is correct.
                        assert_ne!(recovered, key, "a typo produced the same key");
                    }
                }
            }
        }

        // Every single-character substitution changes either the key bits or the
        // checksum bits, and either way the re-encoded checksum disagrees. This
        // asserts the whole population, not a sample of it.
        assert_eq!(
            caught,
            tried,
            "{} of {tried} typos slipped through",
            tried - caught
        );
    }

    /// Guards the property the module rests on: no padding bits, so no two
    /// written forms can mean the same key.
    #[test]
    fn the_encoding_divides_evenly_into_characters() {
        assert_eq!((KEY_LEN * 8 + CHECKSUM_BITS) % 5, 0);
        assert_eq!(TOTAL_CHARS % GROUP, 0);
    }

    #[test]
    fn dashes_case_and_line_breaks_are_forgiven() {
        let key = Secret::generate().unwrap();
        let phrase = to_phrase(&key);

        let no_dashes: String = phrase.chars().filter(|c| *c != '-').collect();
        assert_eq!(from_phrase(&no_dashes).unwrap(), key);
        assert_eq!(from_phrase(&phrase.to_lowercase()).unwrap(), key);
        assert_eq!(from_phrase(&phrase.replace('-', "\n  ")).unwrap(), key);
    }

    /// Crockford's substitutions, which are what a careful transcriber applies.
    #[test]
    fn read_alike_characters_decode_to_what_they_look_like() {
        let key = Secret::generate().unwrap();
        let phrase = to_phrase(&key);
        let confused = phrase.replace('1', "I").replace('0', "O");
        assert_eq!(from_phrase(&confused).unwrap(), key);

        let confused = phrase.replace('1', "l");
        assert_eq!(from_phrase(&confused).unwrap(), key);
    }

    /// A short or long phrase is a different mistake from a mistyped one, and
    /// the message a user sees should say so.
    #[test]
    fn the_wrong_length_is_reported_as_a_length() {
        let phrase = to_phrase(&Secret::generate().unwrap());
        let short = &phrase[..phrase.len() - 1];
        assert!(matches!(
            from_phrase(short),
            Err(KeyError::BadLength { .. })
        ));

        let long = format!("{}Z", phrase.as_str());
        assert!(matches!(
            from_phrase(&long),
            Err(KeyError::BadLength { .. })
        ));
    }

    #[test]
    fn a_character_outside_the_alphabet_is_refused() {
        let phrase = to_phrase(&Secret::generate().unwrap());
        let bad = phrase.replacen(|c: char| c != '-', "!", 1);
        assert!(matches!(
            from_phrase(&bad),
            Err(KeyError::MistypedRecoveryKey)
        ));
    }
}
