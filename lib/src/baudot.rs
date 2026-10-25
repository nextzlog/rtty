pub const LTRS: u8 = 0x1F;
pub const FIGS: u8 = 0x1B;

const LETTERS: [Option<char>; 32] = [
    None,
    Some('E'),
    Some('\n'),
    Some('A'),
    Some(' '),
    Some('S'),
    Some('I'),
    Some('U'),
    Some('\r'),
    Some('D'),
    Some('R'),
    Some('J'),
    Some('N'),
    Some('F'),
    Some('C'),
    Some('K'),
    Some('T'),
    Some('Z'),
    Some('L'),
    Some('W'),
    Some('H'),
    Some('Y'),
    Some('P'),
    Some('Q'),
    Some('O'),
    Some('B'),
    Some('G'),
    None,
    Some('M'),
    Some('X'),
    Some('V'),
    None,
];

const FIGURES: [Option<char>; 32] = [
    None,
    Some('3'),
    Some('\n'),
    Some('-'),
    Some(' '),
    None,
    Some('8'),
    Some('7'),
    Some('\r'),
    Some('$'),
    Some('4'),
    Some('\''),
    Some(','),
    Some('!'),
    Some(':'),
    Some('('),
    Some('5'),
    Some('+'),
    Some(')'),
    Some('2'),
    Some('#'),
    Some('6'),
    Some('0'),
    Some('1'),
    Some('9'),
    Some('?'),
    Some('&'),
    None,
    Some('.'),
    Some('/'),
    Some('='),
    None,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shift {
    Letters,
    Figures,
    Unknown,
}

#[cfg(test)]
fn sendable(c: char) -> bool {
    LETTERS.contains(&Some(c)) || FIGURES.contains(&Some(c))
}

#[derive(Debug, Clone)]
pub struct Encoder {
    shift: Shift,
    uos: bool,
}

impl Encoder {
    pub fn new(uos: bool) -> Self {
        Encoder {
            shift: Shift::Unknown,
            uos,
        }
    }

    pub fn encode(&mut self, text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for c in text.chars().map(|c| c.to_ascii_uppercase()) {
            let letter = LETTERS.iter().position(|&l| l == Some(c));
            let figure = FIGURES.iter().position(|&f| f == Some(c));
            let code = match (letter, figure) {
                (Some(l), Some(_)) => l,
                (Some(l), None) => {
                    if self.shift != Shift::Letters {
                        out.push(LTRS);
                        self.shift = Shift::Letters;
                    }
                    l
                }
                (None, Some(f)) => {
                    if self.shift != Shift::Figures {
                        out.push(FIGS);
                        self.shift = Shift::Figures;
                    }
                    f
                }
                (None, None) => continue,
            };
            out.push(code as u8);
            if c == ' ' && self.uos {
                self.shift = Shift::Letters;
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct Decoder {
    shift: Shift,
    uos: bool,
}

impl Decoder {
    pub fn new(uos: bool) -> Self {
        Decoder {
            shift: Shift::Letters,
            uos,
        }
    }

    pub fn decode(&mut self, code: u8) -> Option<char> {
        let code = code & 0x1F;
        match code {
            LTRS => {
                self.shift = Shift::Letters;
                return None;
            }
            FIGS => {
                self.shift = Shift::Figures;
                return None;
            }
            _ => {}
        }
        let c = match self.shift {
            Shift::Letters | Shift::Unknown => LETTERS[code as usize],
            Shift::Figures => FIGURES[code as usize],
        };
        if c == Some(' ') && self.uos {
            self.shift = Shift::Letters;
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str, uos: bool) -> String {
        let codes = Encoder::new(uos).encode(text);
        let mut d = Decoder::new(uos);
        codes.into_iter().filter_map(|c| d.decode(c)).collect()
    }

    #[test]
    fn codes_follow_ita2() {
        let mut e = Encoder::new(true);
        assert_eq!(e.encode("A"), vec![LTRS, 0x03]);
        assert_eq!(e.encode("5NN"), vec![FIGS, 0x10, LTRS, 0x0C, 0x0C]);
        assert_eq!(e.encode("=+#"), vec![FIGS, 0x1E, 0x11, 0x14]);
    }

    #[test]
    fn text_survives_encoding_and_decoding() {
        for uos in [false, true] {
            let t = "CQ TEST JA1ZLO 599 10H 599-13M $1.5 (A/B)? 'X'! & = +\r\n";
            assert_eq!(round_trip(t, uos), t);
        }
        assert_eq!(round_trip("ja1zlo é", true), "JA1ZLO ");
    }

    #[test]
    fn unshift_on_space_shifts_figures_again() {
        let mut e = Encoder::new(true);
        assert_eq!(e.encode("1 2"), vec![FIGS, 0x17, 0x04, FIGS, 0x13]);
        let mut e = Encoder::new(false);
        assert_eq!(e.encode("1 2"), vec![FIGS, 0x17, 0x04, 0x13]);
        let mut d = Decoder::new(true);
        let s: String = [FIGS, 0x17, 0x04, 0x13]
            .into_iter()
            .filter_map(|c| d.decode(c))
            .collect();
        assert_eq!(s, "1 W");
    }

    #[test]
    fn sendable_characters() {
        for c in "AZ09-?:$!&#'().,/=+ \r\n".chars() {
            assert!(sendable(c), "{c}");
        }
        for c in "a@*%;".chars() {
            assert!(!sendable(c), "{c}");
        }
    }

    #[test]
    fn every_character_round_trips_alone_and_after_each_shift() {
        let all: Vec<char> = LETTERS
            .iter()
            .chain(FIGURES.iter())
            .flatten()
            .copied()
            .collect();
        for uos in [false, true] {
            for &c in &all {
                for prefix in ["", "A", "1", "1 ", "A "] {
                    let t = format!("{prefix}{c}");
                    assert_eq!(round_trip(&t, uos), t, "{t:?} {uos}");
                }
            }
        }
        let mut d = Decoder::new(false);
        assert_eq!(d.decode(0), None);
        assert_eq!(d.decode(0xE3), Some('A'));
        assert_eq!(d.decode(FIGS), None);
        assert_eq!(d.decode(0x03), Some('-'));
        assert_eq!(d.decode(LTRS), None);
        assert_eq!(d.decode(0x03), Some('A'));
    }
}
