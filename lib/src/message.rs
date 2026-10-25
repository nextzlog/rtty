const NTOKENS: u32 = 2_063_592;
const MAX22: u32 = 4_194_304;
const MAXGRID4: u32 = 32_400;
const ALNUM_SPACE: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const ALNUM: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const LETTERS_SPACE: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const FREE: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?";

fn index(table: &[u8], c: u8) -> Option<u32> {
    table.iter().position(|&t| t == c).map(|i| i as u32)
}

fn base_call(call: &str) -> Option<u32> {
    let b = call.as_bytes();
    let mut c6 = [b' '; 6];
    match b.len() {
        3..=6 if b[2].is_ascii_digit() => c6[..b.len()].copy_from_slice(b),
        2..=5 if b[1].is_ascii_digit() => c6[1..=b.len()].copy_from_slice(b),
        _ => return None,
    }
    let tables = [
        ALNUM_SPACE,
        ALNUM,
        DIGITS,
        LETTERS_SPACE,
        LETTERS_SPACE,
        LETTERS_SPACE,
    ];
    let mut n = 0u32;
    for (t, &c) in tables.iter().zip(&c6) {
        n = n * t.len() as u32 + index(t, c)?;
    }
    Some(n)
}

fn cq_modifier(word: &str) -> Option<u32> {
    let b = word.as_bytes();
    if b.len() == 3 && b.iter().all(u8::is_ascii_digit) {
        return word.parse().ok();
    }
    if (1..=4).contains(&b.len()) && b.iter().all(u8::is_ascii_uppercase) {
        return Some(1000 + b.iter().fold(0, |m, &c| 27 * m + (c - b'A' + 1) as u32));
    }
    None
}

fn pack28(word: &str) -> Option<(u32, bool)> {
    match word {
        "DE" => return Some((0, false)),
        "QRZ" => return Some((1, false)),
        "CQ" => return Some((2, false)),
        _ => {}
    }
    if let Some(m) = word.strip_prefix("CQ ") {
        return cq_modifier(m).map(|v| (3 + v, false));
    }
    let (base, suffix) = match word.strip_suffix("/R") {
        Some(b) => (b, true),
        None => (word, false),
    };
    base_call(base).map(|n| (NTOKENS + MAX22 + n, suffix))
}

fn unpack28(n: u32, suffix: bool) -> Option<String> {
    match n {
        0 => return Some("DE".into()),
        1 => return Some("QRZ".into()),
        2 => return Some("CQ".into()),
        3..=1002 => return Some(format!("CQ {:03}", n - 3)),
        1003..=532_443 => {
            let mut v = n - 1003;
            let mut s = [b' '; 4];
            for c in s.iter_mut().rev() {
                *c = LETTERS_SPACE[(v % 27) as usize];
                v /= 27;
            }
            let m = std::str::from_utf8(&s).ok()?.trim();
            return (!m.is_empty() && !m.contains(' ')).then(|| format!("CQ {m}"));
        }
        _ if n < NTOKENS => return None,
        _ if n < NTOKENS + MAX22 => return Some("<...>".into()),
        _ => {}
    }
    let mut v = n - NTOKENS - MAX22;
    let tables = [
        ALNUM_SPACE,
        ALNUM,
        DIGITS,
        LETTERS_SPACE,
        LETTERS_SPACE,
        LETTERS_SPACE,
    ];
    let mut c6 = [b' '; 6];
    for (c, t) in c6.iter_mut().zip(tables).rev() {
        *c = t[(v % t.len() as u32) as usize];
        v /= t.len() as u32;
    }
    if v != 0 {
        return None;
    }
    let call = std::str::from_utf8(&c6).ok()?.trim().to_string();
    if call.len() < 3 || call.contains(' ') {
        return None;
    }
    Some(if suffix { call + "/R" } else { call })
}

fn is_grid(w: &str) -> bool {
    let b = w.as_bytes();
    b.len() == 4
        && (b'A'..=b'R').contains(&b[0])
        && (b'A'..=b'R').contains(&b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
}

fn report(w: &str) -> Option<u32> {
    let b = w.as_bytes();
    if b.len() != 3 || !(b[0] == b'+' || b[0] == b'-') || !b[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let dd: i32 = w.parse().ok()?;
    (-30..=32).contains(&dd).then(|| (35 + dd) as u32)
}

fn pack_extra(words: &[&str]) -> Option<u32> {
    let g = |w: &str| {
        let b = w.as_bytes();
        (b[0] - b'A') as u32 * 1800
            + (b[1] - b'A') as u32 * 100
            + (b[2] - b'0') as u32 * 10
            + (b[3] - b'0') as u32
    };
    match words {
        [] => Some(MAXGRID4 + 1),
        ["RRR"] => Some(MAXGRID4 + 2),
        ["RR73"] => Some(MAXGRID4 + 3),
        ["73"] => Some(MAXGRID4 + 4),
        ["R", w] if is_grid(w) => Some(0x8000 | g(w)),
        [w] if is_grid(w) => Some(g(w)),
        [w] => match w.strip_prefix('R') {
            Some(r) => report(r).map(|v| 0x8000 | (MAXGRID4 + v)),
            None => report(w).map(|v| MAXGRID4 + v),
        },
        _ => None,
    }
}

fn unpack_extra(v: u32) -> Option<String> {
    let r = v & 0x8000 != 0;
    let g = v & 0x7fff;
    if g < MAXGRID4 {
        let c = [
            b'A' + (g / 1800) as u8,
            b'A' + (g / 100 % 18) as u8,
            b'0' + (g / 10 % 10) as u8,
            b'0' + (g % 10) as u8,
        ];
        let grid = String::from_utf8_lossy(&c).into_owned();
        return Some(if r { format!("R {grid}") } else { grid });
    }
    Some(match g - MAXGRID4 {
        1 => String::new(),
        2 => "RRR".into(),
        3 => "RR73".into(),
        4 => "73".into(),
        n @ 5..=67 => format!("{}{:+03}", if r { "R" } else { "" }, n as i32 - 35),
        _ => return None,
    })
}

fn pack_standard(text: &str) -> Option<u128> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let (first, rest) = match words.as_slice() {
        ["CQ", m, c, rest @ ..]
            if cq_modifier(m).is_some() && base_call(c.trim_end_matches("/R")).is_some() =>
        {
            (format!("CQ {m}"), &words[2..])
        }
        [w, rest @ ..] => (w.to_string(), rest),
        [] => return None,
    };
    let (second, extra) = rest.split_first()?;
    let (a, ra) = pack28(&first)?;
    let (b, rb) = pack28(second)?;
    if b < NTOKENS {
        return None;
    }
    let e = pack_extra(extra)?;
    let mut v = (a as u128) << 1 | ra as u128;
    v = v << 29 | ((b as u128) << 1 | rb as u128);
    v = v << 16 | e as u128;
    Some(v << 3 | 1)
}

fn pack_free(text: &str) -> Option<u128> {
    let b = text.trim().as_bytes();
    if b.len() > 13 {
        return None;
    }
    let mut v = 0u128;
    for i in 0..13 {
        v = v * 42 + index(FREE, *b.get(i).unwrap_or(&b' '))? as u128;
    }
    Some(v << 6)
}

pub fn pack(text: &str) -> Result<u128, String> {
    let t = text.trim().to_ascii_uppercase();
    if t.is_empty() {
        return Err("empty message".into());
    }
    pack_standard(&t)
        .or_else(|| pack_free(&t))
        .ok_or_else(|| format!("cannot encode '{text}'"))
}

pub fn unpack(bits: u128) -> Option<String> {
    let i3 = (bits & 7) as u32;
    let n3 = (bits >> 3 & 7) as u32;
    match (i3, n3) {
        (0, 0) => {
            let mut v = bits >> 6;
            let mut c = [b' '; 13];
            for x in c.iter_mut().rev() {
                *x = FREE[(v % 42) as usize];
                v /= 42;
            }
            (v == 0).then(|| String::from_utf8_lossy(&c).trim().to_string())
        }
        (1, _) => {
            let e = (bits >> 3 & 0xffff) as u32;
            let b = (bits >> 19 & 0x1fff_ffff) as u32;
            let a = (bits >> 48 & 0x1fff_ffff) as u32;
            if b >> 1 < NTOKENS {
                return None;
            }
            let first = unpack28(a >> 1, a & 1 == 1)?;
            let second = unpack28(b >> 1, b & 1 == 1)?;
            let extra = unpack_extra(e)?;
            Some(
                [first, second, extra]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fields {
    pub to: String,
    pub de: String,
    pub extra: String,
}

pub fn fields(text: &str) -> Option<Fields> {
    let bits = pack_standard(&text.to_ascii_uppercase())?;
    let full = unpack(bits)?;
    let words: Vec<&str> = full.split(' ').collect();
    let n = if (3..NTOKENS as u128).contains(&(bits >> 49 & 0x0fff_ffff)) {
        2
    } else {
        1
    };
    Some(Fields {
        to: words[..n].join(" "),
        de: words.get(n)?.to_string(),
        extra: words[n + 1..].join(" "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Rng;

    const MESSAGES: &[&str] = &[
        "CQ JA1ZLO PM95",
        "CQ DX JA1ZLO PM95",
        "CQ TEST JA1YWX PM95",
        "CQ 123 W1AW FN31",
        "CQ JA1ZLO",
        "QRZ K1JT FN20",
        "DE JA1ZLO PM95",
        "JA1ZLO JA1YWX PM95",
        "JA1ZLO JA1YWX -12",
        "JA1ZLO JA1YWX R-07",
        "JA1ZLO JA1YWX +05",
        "JA1ZLO JA1YWX -30",
        "JA1ZLO JA1YWX +32",
        "JA1YWX JA1ZLO R+10",
        "JA1ZLO JA1YWX RRR",
        "JA1ZLO JA1YWX RR73",
        "JA1YWX JA1ZLO 73",
        "JA1ZLO JA1YWX R PM95",
        "CQ JA1ZLO/R PM95",
        "K1JT/R JA1ZLO -15",
        "JA1ZLO JA1YWX",
        "W1AW K9A AA00",
        "TNX BOB 73 GL",
        "HELLO WORLD",
        "A",
        "+-./?",
        "1234567890ABC",
    ];

    #[test]
    fn every_message_type_packs_and_unpacks() {
        for text in MESSAGES {
            let bits = pack(text).unwrap();
            assert!(bits < 1 << 77, "{text}");
            assert_eq!(unpack(bits).as_deref(), Some(*text), "{text}");
        }
        assert_eq!(
            unpack(pack(" cq ja1zlo pm95 ").unwrap()).as_deref(),
            Some("CQ JA1ZLO PM95")
        );
    }

    #[test]
    fn invalid_messages_are_rejected() {
        for text in [
            "",
            "   ",
            "THIS IS FAR TOO LONG FOR FT8",
            "JA1ZLO@HOME",
            "CQ JA1ZLO PM95 EXTRA",
            "JA1ZLO JA1YWX -31",
            "JA1ZLO JA1YWX +33",
            "ÄÖÜ",
        ] {
            assert!(pack(text).is_err(), "{text}");
        }
        assert!(pack("JA1ZLO JA1YWX ZZ99").is_err());
    }

    #[test]
    fn random_payloads_never_panic() {
        let mut rng = Rng::new(5);
        for _ in 0..20_000 {
            let bits = (rng.next_u64() as u128) << 13 ^ rng.next_u64() as u128;
            if let Some(t) = unpack(bits & ((1 << 77) - 1)) {
                assert!(t.len() <= 40);
            }
        }
    }

    #[test]
    fn fields_split_calls_and_exchange() {
        let f = fields("CQ DX JA1ZLO PM95").unwrap();
        assert_eq!(
            (f.to.as_str(), f.de.as_str(), f.extra.as_str()),
            ("CQ DX", "JA1ZLO", "PM95")
        );
        let f = fields("JA1ZLO JA1YWX R-07").unwrap();
        assert_eq!(
            (f.to.as_str(), f.de.as_str(), f.extra.as_str()),
            ("JA1ZLO", "JA1YWX", "R-07")
        );
        let f = fields("JA1ZLO JA1YWX").unwrap();
        assert_eq!(f.extra, "");
        assert!(fields("TNX BOB 73 GL").is_none());
        assert!(fields("").is_none());
    }
}
