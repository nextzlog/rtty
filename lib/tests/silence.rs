mod common;

use common::at;
use rtty::sim::{render, Agc, Band, Qrn, Source, Station};
use rtty::{Modem, Options, Rtty, RttyOptions};

fn bands(seconds: f32) -> Vec<(&'static str, Band, Source, Vec<Station>)> {
    let base = Band::audio(12000, seconds);
    let crashes = Band {
        qrn: Qrn {
            rate: 3.0,
            level: 80.0,
        },
        ..base
    };
    let breathing = Band {
        agc: Agc {
            depth_db: 15.0,
            hz: 0.3,
        },
        ..base
    };
    let carriers = vec![
        Station::new("", 1000.0, 0.0, 10.0),
        Station::new("", 1503.0, 0.0, 0.0),
        Station::new("", 2125.0, 0.0, 5.0),
        Station::new("", 2295.0, 0.0, 5.0),
    ];
    vec![
        ("white", base, Source::Tone, vec![]),
        ("crashes", crashes, Source::Tone, vec![]),
        ("breathing", breathing, Source::Tone, vec![]),
        ("carriers", crashes, Source::Tone, carriers),
    ]
}

fn decodes(m: &mut dyn Modem, x: &[f32]) -> Vec<String> {
    let mut got: Vec<String> = x
        .chunks(1200)
        .enumerate()
        .flat_map(|(i, c)| m.push(c, at(i as f64 * 0.1)))
        .map(|d| d.text)
        .collect();
    got.extend(
        m.push(&[0.0; 12000], at(x.len() as f64 / 12000.0))
            .into_iter()
            .map(|d| d.text),
    );
    got
}

#[test]
fn rtty_prints_nothing_from_empty_bands() {
    for (name, band, source, stations) in bands(60.0) {
        for seed in 0..2 {
            let x = render(&band, source, &stations, seed).unwrap();
            let mut m = Rtty::new(12000, Options::default()).unwrap();
            let got = decodes(&mut m, &x);
            assert!(got.is_empty(), "{name} {got:?}");
        }
    }
}

#[test]
fn long_silence_around_a_transmission_adds_no_garbage() {
    let text = "CQ TEST JA1ZLO JA1ZLO";
    let band = Band {
        qrn: Qrn {
            rate: 1.0,
            level: 40.0,
        },
        ..Band::audio(12000, 50.0)
    };
    let st = [Station::new(text, 2125.0, 20.0, 6.0)];
    let x = render(&band, Source::Rtty(RttyOptions::default()), &st, 1).unwrap();
    let got = decodes(&mut Rtty::new(12000, Options::default()).unwrap(), &x);
    assert_eq!(got.join(" "), text);
}
