mod common;

use common::{at, secs};
use rtty::sim::{render, rtty_trial, Band, Channel, Preset, Qrn, Source, Station};
use rtty::{Decode, Modem, Options, Rtty, RttyOptions, Unit};

const TEXT: &str = "CQ TEST JA1ZLO JA1ZLO TEST 599 001 001 TU JA1YWX 599-13M $1.5 (A/B)?";

fn rtty(o: Options) -> Rtty {
    Rtty::new(12000, o).unwrap()
}

fn errors(snr: f32, channel: Channel, o: Options, band: Band, seeds: u64) -> usize {
    (0..seeds)
        .map(|seed| {
            let st = Station::new(TEXT, 1500.0, 0.2, snr).with(channel);
            rtty_trial(&mut rtty(o), &band, &st, seed).unwrap().errors
        })
        .sum()
}

fn band() -> Band {
    Band::audio(12000, 18.0)
}

fn push(m: &mut Rtty, x: &[f32], block: usize) -> Vec<Decode> {
    let mut got: Vec<Decode> = x
        .chunks(block)
        .enumerate()
        .flat_map(|(i, c)| m.push(c, at(100.0 + (i * block) as f64 / 12000.0)))
        .collect();
    got.extend(m.push(&[0.0; 12000], at(100.0 + x.len() as f64 / 12000.0)));
    got
}

#[test]
fn words_arrive_with_time_stamps_as_they_are_typed() {
    let mut m = rtty(Options::default());
    let tx = m
        .transmit("CQ TEST JA1ZLO\r\nK", 2125.0, at(100.0))
        .unwrap();
    assert_eq!((tx.start, tx.parity), (at(100.0), None));
    let got = push(&mut m, &tx.audio, 240);
    let texts: Vec<&str> = got.iter().map(|d| d.text.as_str()).collect();
    assert_eq!(texts, ["CQ", "TEST", "JA1ZLO", "\n", "K"]);
    let char_time = 7.5 / 45.45;
    for (d, n) in got.iter().zip([4.0, 9.0, 18.0]) {
        let t = secs(d.at) - 100.0;
        assert!((t - 0.15 - n * char_time).abs() < 0.2, "{} {t}", d.text);
        assert!(d.slot.is_none() && d.dt.is_none() && d.snr > 5.0);
    }
}

#[test]
fn line_breaks_follow_text_only() {
    let mut m = rtty(Options::default());
    let tx = m
        .transmit("\r\n\r\nJA1ZLO\r\n\r\nJA1YWX\r\n", 2125.0, at(100.0))
        .unwrap();
    let got = push(&mut m, &tx.audio, 240);
    let texts: Vec<&str> = got.iter().map(|d| d.text.as_str()).collect();
    assert_eq!(texts, ["JA1ZLO", "\n", "JA1YWX", "\n"]);
    let units: Vec<Unit> = got.iter().map(|d| d.unit).collect();
    assert_eq!(
        units,
        [Unit::Word, Unit::LineBreak, Unit::Word, Unit::LineBreak]
    );
}

#[test]
fn small_chunks_give_the_same_decodes_as_one_buffer() {
    let st = Station::new(TEXT, 2125.0, 0.2, 3.0);
    let x = render(&band(), Source::Rtty(RttyOptions::default()), &[st], 3).unwrap();
    let whole = push(&mut rtty(Options::default()), &x, x.len());
    for block in [1, 7, 240, 1200] {
        assert_eq!(
            push(&mut rtty(Options::default()), &x, block),
            whole,
            "{block}"
        );
    }
}

#[test]
fn moderate_snr_fading_and_qrn() {
    let o = Options::default();
    assert_eq!(errors(3.0, Channel::default(), o, band(), 3), 0);
    assert!(errors(-3.0, Channel::default(), o, band(), 3) <= 15);
    let plain = Options {
        rtty: RttyOptions {
            atc: false,
            blanker: false,
            ..RttyOptions::default()
        },
        ..o
    };
    let fading = errors(10.0, Preset::MidModerate.channel(), o, band(), 3);
    assert!(fading <= 20, "{fading}");
    assert!(fading < errors(10.0, Preset::MidModerate.channel(), plain, band(), 3));
    let crashes = Band {
        qrn: Qrn {
            rate: 4.0,
            level: 60.0,
        },
        ..band()
    };
    assert!(errors(6.0, Channel::default(), o, crashes, 3) <= 6);
}

#[test]
fn baud_errors_of_two_percent_are_tolerated() {
    for baud in [44.54, 46.36] {
        let sent = RttyOptions {
            baud,
            ..RttyOptions::default()
        };
        let st = Station::new(TEXT, 2125.0, 0.2, 10.0);
        let x = render(&band(), Source::Rtty(sent), &[st], 4).unwrap();
        let got = push(&mut rtty(Options::default()), &x, 480);
        let text: Vec<&str> = got.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(text.join(" "), TEXT, "{baud}");
    }
}

#[test]
fn a_neighbour_300_hz_away_does_not_corrupt_the_text() {
    for (gap, level) in [(300.0, 0.0), (450.0, 6.0)] {
        let wanted = Station::new(TEXT, 2125.0, 0.2, 6.0);
        let qrm = Station::new(
            &"RYRYRY QRM QRM DE JE3CCC ".repeat(3),
            2125.0 + gap,
            0.0,
            6.0 + level,
        );
        let x = render(
            &band(),
            Source::Rtty(RttyOptions::default()),
            &[wanted, qrm],
            5,
        )
        .unwrap();
        let got = push(&mut rtty(Options::default()), &x, 480);
        let text: Vec<&str> = got.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(text.join(" "), TEXT, "{gap}");
    }
}

#[test]
fn reversed_shift_is_garbage_until_the_option_is_set() {
    let st = Station::new(TEXT, 2125.0, 0.2, 10.0);
    let x = render(&band(), Source::Rtty(RttyOptions::default()), &[st], 6).unwrap();
    let mut reversed = Options::default();
    reversed.rtty.reverse = true;
    let wrong: Vec<Decode> = push(&mut rtty(reversed), &x, 480);
    assert!(
        wrong
            .iter()
            .all(|d| !TEXT.contains(&d.text) || d.text.len() < 3),
        "{wrong:?}"
    );
    let sent = RttyOptions {
        reverse: true,
        ..RttyOptions::default()
    };
    let st = Station::new(TEXT, 2125.0, 0.2, 10.0);
    let x = render(&band(), Source::Rtty(sent), &[st], 6).unwrap();
    let got = push(&mut rtty(reversed), &x, 480);
    assert_eq!(
        got.iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        TEXT
    );
}

#[test]
fn noise_and_ft8_audio_print_nothing() {
    for seed in 0..3 {
        let x = render(&band(), Source::Rtty(RttyOptions::default()), &[], seed).unwrap();
        assert!(
            push(&mut rtty(Options::default()), &x, 480).is_empty(),
            "{seed}"
        );
        let ft8 = [
            Station::new("CQ JA1ZLO PM95", 2125.0, 0.0, 10.0),
            Station::new("CQ JA1YWX PM95", 2260.0, 0.3, 10.0),
        ];
        let x = render(&Band::audio(12000, 15.0), Source::Ft8, &ft8, seed).unwrap();
        let got = push(&mut rtty(Options::default()), &x, 480);
        assert!(got.iter().all(|d| d.text.len() <= 1), "{got:?}");
    }
}

#[test]
fn a_reversed_decode_reports_the_frequency_to_tune_to() {
    let mut reversed = Options::default();
    reversed.rtty.reverse = true;
    let mut m = rtty(reversed);
    let tx = m.transmit("CQ TEST JA1ZLO", 2125.0, at(100.0)).unwrap();
    let got = push(&mut m, &tx.audio, 480);
    assert_eq!(got.len(), 3);
    assert!(got.iter().all(|d| d.hz == 2125.0), "{got:?}");
    let tx = m.transmit("CQ TEST JA1ZLO", 1500.0, at(100.0)).unwrap();
    reversed.rx_hz = Some(1500.0);
    let again = push(&mut rtty(reversed), &tx.audio, 480);
    assert_eq!(again.len(), 3, "{again:?}");
    assert!(again.iter().all(|d| d.hz == 1500.0), "{again:?}");
}

#[test]
fn an_endless_word_is_split_and_does_not_grow_without_bound() {
    let mut m = rtty(Options::default());
    let tx = m.transmit(&"RY".repeat(200), 2125.0, at(100.0)).unwrap();
    let half = tx.audio.len() / 2;
    let early = push(&mut m, &tx.audio[..half], 480);
    assert!(!early.is_empty());
    assert!(early.iter().all(|d| d.text.len() <= 64), "{early:?}");
}
