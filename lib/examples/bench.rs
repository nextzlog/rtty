use rtty::sim::{render, rtty_trial, Agc, Band, Preset, Qrn, Source, Station};
use rtty::{Modem, Options, Rtty};

const USAGE: &str = "usage: bench [preset|qrn|silence|all] [trials] [plain]";

fn presets(name: &str) -> Vec<(String, Preset, Qrn)> {
    let qrn = Qrn {
        rate: 4.0,
        level: 60.0,
    };
    match name {
        "all" => Preset::ALL
            .iter()
            .map(|p| (p.name().to_string(), *p, Qrn::default()))
            .chain([("qrn".into(), Preset::Awgn, qrn)])
            .collect(),
        "qrn" => vec![("qrn".into(), Preset::Awgn, qrn)],
        _ => Preset::ALL
            .iter()
            .filter(|p| p.name() == name)
            .map(|p| (p.name().to_string(), *p, Qrn::default()))
            .collect(),
    }
}

fn options(plain: bool) -> Options {
    let mut o = Options::default();
    if plain {
        o.rtty.atc = false;
        o.rtty.blanker = false;
    }
    o
}

fn rtty(name: &str, preset: Preset, qrn: Qrn, trials: u64, plain: bool) {
    let text = "CQ TEST JA1ZLO JA1ZLO TEST 599 001 001 TU JA1YWX 599 012 012";
    let band = Band {
        qrn,
        ..Band::audio(12000, 15.0)
    };
    let mut modem = Rtty::new(12000, options(plain)).unwrap();
    println!("RTTY {name}: snr_db char_errors/chars cer_pct");
    for snr in (-10..=16).rev().step_by(2) {
        let (mut sent, mut bad) = (0, 0);
        for seed in 0..trials {
            let st = Station::new(text, 1500.0, 0.2, snr as f32).with(preset.channel());
            let t = rtty_trial(&mut modem, &band, &st, 1000 + seed).unwrap();
            sent += t.sent;
            bad += t.errors;
        }
        println!(
            "{snr:7} {bad:5}/{sent:<5} {:6.1}",
            100.0 * bad as f64 / sent as f64
        );
    }
}

fn empty(seconds: f32) -> Vec<(&'static str, Band, Vec<Station>)> {
    let base = Band::audio(12000, seconds);
    let crashes = Band {
        qrn: Qrn {
            rate: 3.0,
            level: 80.0,
        },
        ..base
    };
    let carriers = [1000.0, 1503.0, 2125.0, 2295.0]
        .map(|hz| Station::new("", hz, 0.0, 5.0))
        .to_vec();
    vec![
        ("white", base, vec![]),
        ("crashes", crashes, vec![]),
        (
            "breathing",
            Band {
                agc: Agc {
                    depth_db: 15.0,
                    hz: 0.3,
                },
                ..base
            },
            vec![],
        ),
        ("carriers", crashes, carriers),
    ]
}

fn silence(minutes: u64) {
    let seconds = 60.0;
    for (name, band, stations) in empty(seconds) {
        {
            println!("RTTY silence {name}: squelch false_chars_per_min cer_pct_at_0db");
            for squelch in [0.45, 0.5, 0.55, 0.6, 0.65] {
                let mut o = Options::default();
                o.rtty.squelch = squelch;
                let mut chars = 0;
                for seed in 0..minutes {
                    let x = render(&band, Source::Tone, &stations, seed).unwrap();
                    let mut m = Rtty::new(12000, o).unwrap();
                    chars += m
                        .push(&x, std::time::UNIX_EPOCH)
                        .iter()
                        .map(|d| d.text.len())
                        .sum::<usize>();
                }
                let (mut sent, mut bad) = (0, 0);
                for seed in 0..minutes {
                    let st =
                        Station::new("CQ TEST JA1ZLO JA1ZLO TEST 599 001 001", 1500.0, 0.2, 0.0);
                    let t = rtty_trial(
                        &mut Rtty::new(12000, o).unwrap(),
                        &Band::audio(12000, 10.0),
                        &st,
                        seed,
                    )
                    .unwrap();
                    sent += t.sent;
                    bad += t.errors;
                }
                println!(
                    "{squelch:7} {:8.2} {:8.1}",
                    chars as f64 / minutes as f64,
                    100.0 * bad as f64 / sent as f64
                );
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = args.first().map(String::as_str).unwrap_or("awgn");
    let trials = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let plain = args.iter().any(|a| a == "plain");
    if name == "silence" {
        return silence(trials);
    }
    let list = presets(name);
    if list.is_empty() {
        eprintln!("{USAGE}");
    }
    for (name, preset, qrn) in list {
        rtty(&name, preset, qrn, trials, plain);
    }
}
