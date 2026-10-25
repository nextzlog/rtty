use crate::tables::{GENERATOR, MN, NM, NUM_ROWS};

pub const N: usize = 174;
pub const K: usize = 91;
pub const M: usize = 83;

fn rows() -> &'static [u128; M] {
    static ROWS: std::sync::OnceLock<[u128; M]> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        let mut r = [0u128; M];
        for (row, g) in r.iter_mut().zip(GENERATOR.iter()) {
            *row = g.iter().fold(0u128, |a, &b| a << 8 | b as u128) >> 5;
        }
        r
    })
}

pub fn encode(a91: u128) -> [u8; N] {
    let mut cw = [0u8; N];
    for (i, c) in cw.iter_mut().take(K).enumerate() {
        *c = (a91 >> (K - 1 - i) & 1) as u8;
    }
    for (i, row) in rows().iter().enumerate() {
        cw[K + i] = ((row & a91).count_ones() & 1) as u8;
    }
    cw
}

pub fn bits_to_a91(cw: &[u8]) -> u128 {
    cw[..K].iter().fold(0u128, |a, &b| a << 1 | b as u128)
}

fn errors(cw: &[u8; N]) -> usize {
    (0..M)
        .filter(|&m| {
            NM[m][..NUM_ROWS[m] as usize]
                .iter()
                .fold(0, |x, &n| x ^ cw[n as usize - 1])
                != 0
        })
        .count()
}

const STALL: usize = 10;

pub fn decode(llr: &[f32; N], iterations: usize) -> Option<[u8; N]> {
    let mut tov = [[0f32; 3]; N];
    let mut toc = [[0f32; 7]; M];
    let mut cw = [0u8; N];
    let mut best = M;
    let mut since = 0;
    for _ in 0..iterations {
        for n in 0..N {
            cw[n] = (llr[n] + tov[n][0] + tov[n][1] + tov[n][2] > 0.0) as u8;
        }
        let e = errors(&cw);
        if e == 0 {
            return cw.contains(&1).then_some(cw);
        }
        if e < best {
            best = e;
            since = 0;
        } else {
            since += 1;
            if since > STALL {
                return None;
            }
        }
        for m in 0..M {
            for (k, &n1) in NM[m][..NUM_ROWS[m] as usize].iter().enumerate() {
                let n = n1 as usize - 1;
                let mut t = llr[n];
                for (j, &m1) in MN[n].iter().enumerate() {
                    if m1 as usize - 1 != m {
                        t += tov[n][j];
                    }
                }
                toc[m][k] = (-t / 2.0).tanh();
            }
        }
        for n in 0..N {
            for (j, &m1) in MN[n].iter().enumerate() {
                let m = m1 as usize - 1;
                let mut p = 1.0f32;
                for (k, &n1) in NM[m][..NUM_ROWS[m] as usize].iter().enumerate() {
                    if n1 as usize - 1 != n {
                        p *= toc[m][k];
                    }
                }
                tov[n][j] = -2.0 * p.clamp(-0.999_999, 0.999_999).atanh();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::{crc_ok, with_crc};
    use crate::sim::Rng;

    #[test]
    fn every_encoded_word_meets_all_parity_checks() {
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let payload = (rng.next_u64() as u128) << 13 ^ rng.next_u64() as u128 & ((1 << 77) - 1);
            let cw = encode(with_crc(payload & ((1 << 77) - 1)));
            assert_eq!(errors(&cw), 0);
            assert!(crc_ok(bits_to_a91(&cw)));
        }
    }

    #[test]
    fn belief_propagation_repairs_flipped_bits() {
        let cw = encode(with_crc(0x1234_5678_9abc_def0_1234));
        let mut llr = [0f32; N];
        for (i, l) in llr.iter_mut().enumerate() {
            let sign = if cw[i] == 1 { 1.0 } else { -1.0 };
            *l = if i % 9 == 0 { -sign * 0.5 } else { sign * 2.0 };
        }
        assert_eq!(decode(&llr, 30), Some(cw));
    }

    fn flipped(cw: &[u8; N], flips: usize, rng: &mut Rng) -> [f32; N] {
        let mut llr = [0f32; N];
        for (i, l) in llr.iter_mut().enumerate() {
            *l = if cw[i] == 1 { 3.0 } else { -3.0 };
        }
        let mut done = 0;
        while done < flips {
            let i = (rng.next_u64() % N as u64) as usize;
            if llr[i].signum() == if cw[i] == 1 { 1.0 } else { -1.0 } {
                llr[i] = -llr[i];
                done += 1;
            }
        }
        llr
    }

    #[test]
    fn repair_limits_for_random_codewords() {
        let mut rng = Rng::new(11);
        let mut rate = Vec::new();
        for flips in [4, 8, 12, 40] {
            let ok = (0..40)
                .filter(|_| {
                    let cw = encode(with_crc(rng.next_u64() as u128 & ((1 << 77) - 1)));
                    decode(&flipped(&cw, flips, &mut rng), 50) == Some(cw)
                })
                .count();
            rate.push((flips, ok));
        }
        assert!(
            rate[0].1 == 40 && rate[1].1 >= 38 && rate[2].1 >= 20,
            "{rate:?}"
        );
        assert!(rate[3].1 <= 2, "{rate:?}");
    }

    #[test]
    fn noise_never_yields_an_invalid_codeword() {
        let mut rng = Rng::new(12);
        for _ in 0..100 {
            let mut llr = [0f32; N];
            llr.iter_mut().for_each(|l| *l = 2.0 * rng.gauss() as f32);
            if let Some(cw) = decode(&llr, 30) {
                assert_eq!(errors(&cw), 0);
            }
        }
        assert_eq!(decode(&[0.0; N], 30), None);
        assert_eq!(decode(&[-5.0; N], 30), None);
    }
}
