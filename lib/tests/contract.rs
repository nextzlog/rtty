mod common;

use common::{at, secs};
use rtty::sim::{words, Rng, Station};
use rtty::{Decode, Modem, Options, Rtty};

const T0: f64 = 1_800_000_000.0;

const TEXT: &str = "CQ TEST JA1ZLO JA1ZLO";
const HZ: f32 = 2125.0;

fn modem(rate: u32, options: Options) -> Rtty {
    Rtty::new(rate, options).unwrap()
}

fn on_air(m: &mut dyn Modem, text: &str, hz: f32, rate: u32, seed: u64) -> (Vec<f32>, f64) {
    let tx = m.transmit(text, hz, at(T0 + 0.2)).unwrap();
    let lead = secs(tx.start) - T0;
    let len = ((lead + 16.0) * rate as f64) as usize;
    let mut rng = Rng::new(seed);
    let mut x: Vec<f32> = (0..len).map(|_| (0.02 * rng.gauss()) as f32).collect();
    let k0 = (lead * rate as f64).round() as usize;
    for (k, v) in tx.audio.iter().enumerate() {
        x[k0 + k] += 0.3 * v;
    }
    (x, secs(tx.start))
}

fn push_all(m: &mut dyn Modem, x: &[f32], rate: u32, block: usize) -> Vec<Decode> {
    x.chunks(block.max(1))
        .enumerate()
        .flat_map(|(i, c)| m.push(c, at(T0 + (i * block) as f64 / rate as f64)))
        .collect::<Vec<_>>()
}

fn joined(d: &[Decode]) -> String {
    words(d.iter().map(|d| d.text.as_str()))
}

#[test]
fn transmit_then_push_round_trips_with_timestamps() {
    for rate in [12000, 48000] {
        let mut m = modem(rate, Options::default());
        let text = TEXT;
        let (x, start) = on_air(&mut m, text, HZ, rate, 1);
        let got = push_all(&mut m, &x, rate, rate as usize / 25);
        assert_eq!(joined(&got), text, "{rate}");
        for d in &got {
            assert!((d.hz - HZ).abs() < 1.0, "{d:?}");
            assert!(secs(d.at) > start && secs(d.at) < start + 6.0, "{d:?}");
        }
        assert!(got.windows(2).all(|w| w[0].at <= w[1].at));
        assert_eq!(got.len(), 4);
    }
}

#[test]
fn chunked_and_whole_buffers_decode_the_same() {
    let mut m = modem(12000, Options::default());
    let (x, _) = on_air(&mut m, TEXT, HZ, 12000, 2);
    let whole = push_all(&mut m, &x, 12000, x.len());
    for block in [1, 240, 1001, 4096] {
        let mut m = modem(12000, Options::default());
        let parts = push_all(&mut m, &x, 12000, block);
        assert_eq!(parts, whole, "{block}");
    }
    assert!(!whole.is_empty());
}

#[test]
fn empty_short_silent_and_non_finite_input_is_harmless() {
    for rate in [8000, 11025, 22050, 44100, 48000] {
        let mut m = modem(rate, Options::default());
        assert!(m.push(&[], at(T0)).is_empty());
        assert!(m.push(&[0.5; 3], at(T0)).is_empty());
        assert!(m
            .push(&vec![0.0; rate as usize * 16], at(T0 + 1.0))
            .is_empty());
        let mut bad = vec![f32::NAN; rate as usize * 16];
        bad[100] = f32::INFINITY;
        bad[200] = 1e30;
        assert!(m.push(&bad, at(T0 + 20.0)).is_empty(), "{rate}");
        let mut m = modem(rate, Options::default());
        let (x, _) = on_air(&mut m, TEXT, HZ, rate, 3);
        let mut x = x;
        x[1000] = f32::NAN;
        assert_eq!(joined(&push_all(&mut m, &x, rate, 2000)), TEXT, "{rate}");
    }
}

#[test]
fn set_options_takes_effect() {
    let mut m = modem(12000, Options::default());
    let (x, _) = on_air(&mut m, TEXT, HZ, 12000, 4);
    let away = Options {
        min_hz: 300.0,
        max_hz: 900.0,
        ..Options::default()
    };
    m.set_options(away);
    assert_eq!(m.options().max_hz, 900.0);
    assert!(push_all(&mut m, &x, 12000, 960).is_empty());
    m.set_options(Options::default());
    assert!(!push_all(&mut m, &x, 12000, 960).is_empty());
    m.set_options(Options {
        max_hz: f32::NAN,
        min_hz: -4.0,
        ..Options::default()
    });
    assert_eq!(m.options().check(), Ok(()));
}

#[test]
fn extreme_options_never_panic() {
    let extremes = [
        Options {
            min_hz: 0.0,
            max_hz: 1e9,
            ..Options::default()
        },
        Options {
            min_hz: 2999.0,
            max_hz: 3000.0,
            rx_hz: Some(-5.0),
            ..Options::default()
        },
        {
            let mut o = Options::default();
            o.rtty.baud = 1e9;
            o.rtty.shift = 1e9;
            o.rtty.squelch = 2.0;
            o
        },
    ];
    for o in extremes {
        let mut m = modem(12000, o);
        let mut rng = Rng::new(7);
        let x: Vec<f32> = (0..12000 * 8)
            .map(|_| (0.05 * rng.gauss()) as f32)
            .collect();
        let _ = push_all(&mut m, &x, 12000, 12000 * 8);
        let _ = m.transmit(TEXT, 1000.0, at(T0));
    }
}

#[test]
fn arbitrary_lengths_and_rates_never_panic() {
    let mut rng = Rng::new(9);
    for rate in [8000, 11025, 22050, 44100, 48000] {
        let mut m = modem(rate, Options::default());
        let mut t = T0;
        for _ in 0..60 {
            let n = (rng.next_u64() % 9000) as usize;
            let x: Vec<f32> = (0..n).map(|_| (0.1 * rng.gauss()) as f32).collect();
            let _ = m.push(&x, at(t));
            t += n as f64 / rate as f64 + if rng.uniform() < 0.1 { 3.0 } else { 0.0 };
        }
        let tx = m.transmit(TEXT, 1000.0, at(t)).unwrap();
        assert!(!tx.audio.is_empty());
    }
}

fn call(rng: &mut Rng) -> String {
    let l = |rng: &mut Rng| char::from(b'A' + (rng.next_u64() % 26) as u8);
    let d = char::from(b'0' + (rng.next_u64() % 10) as u8);
    let tail: String = (0..1 + rng.next_u64() % 3).map(|_| l(rng)).collect();
    format!("J{}{d}{tail}", l(rng))
}

#[test]
fn random_messages_at_random_frequencies_round_trip() {
    let mut rng = Rng::new(21);
    let mut m = modem(12000, Options::default());
    for k in 0..6 {
        let text = (0..4).map(|_| call(&mut rng)).collect::<Vec<_>>().join(" ")
            + " 599 0"
            + &k.to_string();
        let hz = 600.0 + rng.uniform() as f32 * 2000.0;
        let band = rtty::sim::Band::audio(12000, 8.0);
        let t =
            rtty::sim::rtty_trial(&mut m, &band, &Station::new(&text, hz, 0.2, 10.0), k).unwrap();
        assert_eq!(t.errors, 0, "{text} {hz} {}", t.text);
    }
}
