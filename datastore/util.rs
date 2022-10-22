pub fn is_canonical_uuid(uuid: &str) -> bool {
    let dash_pattern = 0b000000001000010000100001000000000000u64;
    if uuid.len() != 36 { return false; }
    uuid.bytes().enumerate()
        .all(|(i, b)| {
            if (dash_pattern & (1u64 << (35-i))) != 0 {
                b == b'-'
            } else {
                b.wrapping_sub(b'0') < 10 || b.wrapping_sub(b'a') < 6
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_canonical_uuid() {
        assert!(is_canonical_uuid("47ab22d6-f82c-4e0a-9a8a-7300f50d0901"));
        assert!(!is_canonical_uuid("47ab22d6-f82c-4e0a-9a8a-7300f5"));
        assert!(!is_canonical_uuid("47ab22d-6f82c-4e0a-9a8a-7300f50d0901"));
        assert!(!is_canonical_uuid("47AB22D6-F82C-4E0A-9A8A-7300F50D0901"));
        assert!(!is_canonical_uuid("::::::::-f82c-4e0a-9a8a-7300f50d0901"));
        assert!(!is_canonical_uuid("////////-f82c-4e0a-9a8a-7300f50d0901"));
        assert!(!is_canonical_uuid("````````-f82c-4e0a-9a8a-7300f50d0901"));
        assert!(!is_canonical_uuid("gggggggg-f82c-4e0a-9a8a-7300f50d0901"));
    }
}
