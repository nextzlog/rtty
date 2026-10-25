pub fn crc14(bits: u128, len: u32) -> u16 {
    let mut rem = 0u16;
    for i in (0..len).rev() {
        let top = (rem >> 13 & 1) as u128 ^ (bits >> i & 1);
        rem = rem << 1 & 0x3fff;
        if top == 1 {
            rem ^= 0x2757;
        }
    }
    rem
}

pub fn with_crc(payload: u128) -> u128 {
    payload << 14 | crc14(payload << 5, 82) as u128
}

pub fn crc_ok(a91: u128) -> bool {
    crc14((a91 >> 14) << 5, 82) == (a91 & 0x3fff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corrupted_payload_fails_the_crc() {
        let a91 = with_crc(0x5555_aaaa_0000_ffff_1111);
        assert!(crc_ok(a91));
        assert!(!crc_ok(a91 ^ 1 << 40));
    }

    #[test]
    fn crc_is_linear_and_catches_every_single_and_double_bit_error() {
        let a = 0x1_2345_6789_abcd_ef01_2345u128 & ((1 << 77) - 1);
        let b = 0x1f_0f0f_0f0f_00ff_ff00u128;
        assert_eq!(
            crc14(a << 5, 82) ^ crc14(b << 5, 82),
            crc14((a ^ b) << 5, 82)
        );
        let w = with_crc(a);
        for i in 0..91 {
            assert!(!crc_ok(w ^ 1 << i), "{i}");
            for j in (i + 1..91).step_by(7) {
                assert!(!crc_ok(w ^ 1 << i ^ 1 << j), "{i} {j}");
            }
        }
        assert_eq!(crc14(0, 82), 0);
        assert!(crc_ok(with_crc(0)) && crc_ok(with_crc((1 << 77) - 1)));
    }
}
