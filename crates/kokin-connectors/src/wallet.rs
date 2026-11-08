//! Address syntax and checksum validation only. No balance, account or ownership inference.
use crate::{Error, Result};
use sha3::{Digest, Keccak256};
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
pub fn normalize(value: &str) -> Result<String> {
    if let Some(hex) = value.strip_prefix("0x") {
        if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid(
                "Ethereum requires 0x followed by 40 hexadecimal characters",
            ));
        }
        let lower = hex.to_ascii_lowercase();
        let digest = Keccak256::digest(lower.as_bytes());
        let checksum: String = lower
            .bytes()
            .enumerate()
            .map(|(i, b)| {
                let nibble = if i % 2 == 0 {
                    digest[i / 2] >> 4
                } else {
                    digest[i / 2] & 15
                };
                if nibble >= 8 {
                    b.to_ascii_uppercase() as char
                } else {
                    b as char
                }
            })
            .collect();
        let mixed = hex.bytes().any(|b| b.is_ascii_lowercase())
            && hex.bytes().any(|b| b.is_ascii_uppercase());
        if mixed && checksum != hex {
            return Err(invalid(
                "Ethereum mixed-case address has an invalid EIP-55 checksum",
            ));
        }
        // All-lower/all-upper legacy inputs have no input checksum protection.
        // Canonical output uses EIP-55; the original remains on the Selector.
        return Ok(format!("0x{checksum}"));
    }
    if value.to_ascii_lowercase().starts_with("bc1") {
        if value.len() > 90 {
            return Err(invalid("Bitcoin SegWit address exceeds 90 characters"));
        }
        let (hrp, _, _) = bech32::segwit::decode(value).map_err(|_| {
            invalid("invalid Bitcoin Bech32/Bech32m checksum, witness program or casing")
        })?;
        if hrp != bech32::hrp::BC {
            return Err(invalid("only Bitcoin mainnet addresses are supported"));
        }
        return Ok(value.to_ascii_lowercase());
    }
    if !(26..=35).contains(&value.len())
        || !matches!(value.as_bytes().first(), Some(b'1' | b'3' | b'T'))
    {
        return Err(invalid("supported families: Ethereum 0x, Bitcoin mainnet Base58Check/SegWit and TRON Base58Check; private keys and other networks are not accepted"));
    }
    let bytes = bs58::decode(value)
        .with_check(None)
        .into_vec()
        .map_err(|_| invalid("invalid Base58Check alphabet or checksum"))?;
    if bytes.len() != 21 || !matches!(bytes[0], 0x00 | 0x05 | 0x41) {
        return Err(invalid(
            "unsupported wallet address version or payload length",
        ));
    }
    if (value.starts_with('T') && bytes[0] != 0x41)
        || (!value.starts_with('T') && !matches!(bytes[0], 0x00 | 0x05))
    {
        return Err(invalid("address prefix and network version disagree"));
    }
    Ok(value.into())
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn eip55_official_vectors_and_mutations() {
        // Official ERC-55 vectors: https://eips.ethereum.org/EIPS/eip-55#test-cases
        for address in [
            "0x52908400098527886E0F7030069857D2E4169EE7",
            "0x8617E340B3D01FA5F11F306F4090FD50E238070D",
            "0xde709f2102306220921060314715629080e2fb77",
            "0x27b1fdb04752bbc536007a920d24acb045561c26",
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
        ] {
            assert_eq!(normalize(address).unwrap(), address);
            assert_eq!(normalize(&address.to_ascii_lowercase()).unwrap(), address);
        }
        assert!(normalize("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAEd").is_err());
        assert!(normalize("0xnot-an-address").is_err());
    }
    #[test]
    fn base58check_rejects_corruption_private_keys_and_wrong_payloads() {
        for address in [
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
            "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
            "TNPeeaaFB7K9cmo4uQpcU32zGK8G1NYqeL",
        ] {
            assert_eq!(normalize(address).unwrap(), address);
            let mut altered = address.to_owned();
            altered.pop();
            altered.push('1');
            assert!(normalize(&altered).is_err());
        }
        let encoded = bs58::encode([0x80; 33]).with_check().into_string();
        assert!(normalize(&encoded).is_err());
        let wrong = bs58::encode([0x41; 20]).with_check().into_string();
        assert!(normalize(&wrong).is_err());
    }
    #[test]
    fn segwit_checksums_witness_lengths_and_mixed_case_are_checked() {
        // Encoder and decoder are independently maintained in the reviewed library;
        // a changed character must be rejected even when address shape is unchanged.
        for version in [bech32::segwit::VERSION_0, bech32::segwit::VERSION_1] {
            let address = bech32::segwit::encode(bech32::hrp::BC, version, &[1u8; 32]).unwrap();
            assert_eq!(normalize(&address).unwrap(), address);
            assert_eq!(normalize(&address.to_ascii_uppercase()).unwrap(), address);
            let mut wrong = address.clone();
            wrong.replace_range(4..5, "Q");
            assert!(normalize(&wrong).is_err());
            let mut wrong = address.clone();
            wrong.pop();
            wrong.push('l');
            assert!(normalize(&wrong).is_err());
        }
        assert!(normalize("bc1invalid").is_err());
    }
}
