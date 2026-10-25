const PROCEDURES: &[&str] = &[
    "CQ", "DE", "TU", "R", "K", "KN", "BK", "AR", "SK", "QSL", "QRZ", "TEST", "NR", "UR", "GL",
    "EE", "73", "TNX", "TKS", "FB", "AGN", "PSE", "CFM", "RR", "QSO",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Heard {
    pub callsign: Option<String>,
    pub report: Option<String>,
    pub exchange: Vec<String>,
}

fn is_report(word: &str) -> bool {
    let b = word.as_bytes();
    word.bytes().all(|c| c.is_ascii_digit())
        && match b.len() {
            2 => word == "59",
            3 => b[0] == b'5' && b[1] != b'0' && b[2] != b'0',
            _ => false,
        }
}

fn base_call(part: &str) -> bool {
    let chars: Vec<char> = part.chars().collect();
    let Some(i) = chars.iter().rposition(|c| c.is_ascii_digit()) else {
        return false;
    };
    let (prefix, suffix) = (&chars[..i], &chars[i + 1..]);
    (1..=3).contains(&prefix.len())
        && prefix.iter().any(|c| c.is_ascii_uppercase())
        && prefix.iter().all(|c| c.is_ascii_alphanumeric())
        && (1..=4).contains(&suffix.len())
        && suffix.iter().all(|c| c.is_ascii_uppercase())
}

fn base_of(call: &str) -> Option<&str> {
    let parts: Vec<&str> = call.split('/').collect();
    let bases: Vec<&str> = parts.iter().copied().filter(|p| base_call(p)).collect();
    let ok = bases.len() == 1
        && parts.len() <= 3
        && parts
            .iter()
            .all(|p| (1..=4).contains(&p.len()) || base_call(p));
    ok.then(|| bases[0])
}

fn same_call(a: &str, b: &str) -> bool {
    !b.is_empty() && (a == b || matches!((base_of(a), base_of(b)), (Some(x), Some(y)) if x == y))
}

pub fn parse(words: &[String], my_call: &str) -> Heard {
    let words: Vec<String> = words
        .iter()
        .flat_map(|w| w.split('-'))
        .map(|w| w.trim().to_ascii_uppercase())
        .filter(|w| !w.is_empty())
        .collect();
    let my_call = my_call.trim().to_ascii_uppercase();
    let is_call = |w: &str| base_of(w).is_some();
    let mut best: Option<(usize, usize)> = None;
    for (i, w) in words.iter().enumerate() {
        if !is_call(w) || same_call(w, &my_call) {
            continue;
        }
        let after_de = i > 0 && words[i - 1] == "DE";
        let score = 2 * usize::from(after_de) + words.iter().filter(|x| *x == w).count();
        if best.is_none_or(|(s, _)| score >= s) {
            best = Some((score, i));
        }
    }
    let callsign = best.map(|(_, i)| words[i].clone());
    let last_call = callsign
        .as_ref()
        .and_then(|c| words.iter().rposition(|w| w == c));
    let last_report = words.iter().rposition(|w| is_report(w));
    let start = match (last_report, last_call) {
        (Some(r), _) => r + 1,
        (None, Some(c)) => c + 1,
        (None, None) => 0,
    };
    let mut exchange: Vec<String> = Vec::new();
    for w in &words[start.min(words.len())..] {
        let skip = PROCEDURES.contains(&w.as_str()) || is_call(w) || is_report(w);
        if !skip && !exchange.contains(w) {
            exchange.push(w.clone());
        }
    }
    Heard {
        callsign,
        report: last_report.map(|i| words[i].clone()),
        exchange,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(text: &str) -> Heard {
        let words: Vec<String> = text.split_whitespace().map(String::from).collect();
        parse(&words, "JA1ZLO")
    }

    fn h(call: Option<&str>, report: Option<&str>, exchange: &[&str]) -> Heard {
        Heard {
            callsign: call.map(String::from),
            report: report.map(String::from),
            exchange: exchange.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn calls_and_exchanges_are_read_from_a_line() {
        let words: Vec<String> = "CQ TEST DE JA1ZLO JA1ZLO"
            .split_whitespace()
            .map(String::from)
            .collect();
        assert_eq!(parse(&words, "JA1YWX"), h(Some("JA1ZLO"), None, &[]));
        assert_eq!(
            heard("JA1YWX 599 001 001"),
            h(Some("JA1YWX"), Some("599"), &["001"])
        );
        assert_eq!(
            heard("JA1ZLO DE JA1YWX 599-1317M 1317M K"),
            h(Some("JA1YWX"), Some("599"), &["1317M"])
        );
        assert_eq!(heard("599 13 13"), h(None, Some("599"), &["13"]));
        assert_eq!(heard("JA1ZLO 599 13"), h(None, Some("599"), &["13"]));
        assert_eq!(heard("JA1ZLO/1 TU"), Heard::default());
        assert_eq!(heard(""), Heard::default());
    }

    #[test]
    fn de_and_repetition_win() {
        assert_eq!(
            heard("JA1YWX DE JH1ABC").callsign.as_deref(),
            Some("JH1ABC")
        );
        assert_eq!(
            heard("JH1ABC JA1YWX JA1YWX").callsign.as_deref(),
            Some("JA1YWX")
        );
    }

    #[test]
    fn no_cut_numbers_in_rtty() {
        assert_eq!(
            heard("JA1YWX 5NN 1T"),
            h(Some("JA1YWX"), None, &["5NN", "1T"])
        );
    }
}
