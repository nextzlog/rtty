use rtty::sim::{
    ft_trial, render, stretch, Agc, Band, Channel, Preset, Qrn, Source, Station, Trial, NOISE,
};
use rtty::{Ft4, Ft8, Modem, Options};

fn tone_band(seconds: f32) -> Band {
    Band::audio(12000, seconds)
}

fn phase_frequency(x: &[f64], hz: f64, rate: f64, at: usize, len: usize) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    let (mut re2, mut im2) = (0.0, 0.0);
    for k in 0..len {
        let w = std::f64::consts::TAU * hz * (at + k) as f64 / rate;
        re += x[at + k] * w.cos();
        im -= x[at + k] * w.sin();
        let j = at + len + k;
        let w = std::f64::consts::TAU * hz * j as f64 / rate;
        re2 += x[j] * w.cos();
        im2 -= x[j] * w.sin();
    }
    let d = (im2.atan2(re2) - im.atan2(re)).rem_euclid(std::f64::consts::TAU);
    let d = if d > std::f64::consts::PI {
        d - std::f64::consts::TAU
    } else {
        d
    };
    hz + d / std::f64::consts::TAU * rate / len as f64
}

fn tone(channel: Channel, band: Band, seed: u64) -> Vec<f64> {
    let st = Station::new("", 1000.0, 0.0, 70.0).with(channel);
    let band = Band {
        noise: NOISE,
        ..band
    };
    render(&band, Source::Tone, &[st], seed)
        .unwrap()
        .into_iter()
        .map(|v| v as f64)
        .collect()
}

#[test]
fn the_simulator_is_deterministic() {
    let st =
        [Station::new("CQ JA1ZLO PM95", 1000.0, 0.0, -10.0).with(Preset::MidDisturbed.channel())];
    let band = Band::slot(Source::Ft8);
    let a = render(&band, Source::Ft8, &st, 3).unwrap();
    assert_eq!(a, render(&band, Source::Ft8, &st, 3).unwrap());
    assert_ne!(a, render(&band, Source::Ft8, &st, 4).unwrap());
    assert_eq!(a.len(), 180_000);
    assert_eq!(
        render(&Band::slot(Source::Ft4), Source::Ft4, &[], 1)
            .unwrap()
            .len(),
        90_000
    );
}

#[test]
fn noise_has_the_configured_level() {
    let x = render(&Band::audio(12000, 10.0), Source::Tone, &[], 1).unwrap();
    let rms = (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
    assert!((rms / NOISE as f64 - 1.0).abs() < 0.01, "{rms}");
}

#[test]
fn drift_and_doppler_shift_move_the_frequency_as_set() {
    let band = tone_band(10.0);
    let x = tone(
        Channel {
            drift: 0.5,
            ..Channel::default()
        },
        band,
        1,
    );
    let f0 = phase_frequency(&x, 1000.0, 12000.0, 12_000, 600);
    let f1 = phase_frequency(&x, 1000.0, 12000.0, 108_000, 600);
    assert!((f1 - f0 - 0.5 * 8.0).abs() < 0.05, "{f0} {f1}");
    assert!((f0 - 1000.0 - 0.5 * 1.05).abs() < 0.05, "{f0}");
    let x = tone(
        Channel {
            shift: 2.5,
            ..Channel::default()
        },
        band,
        1,
    );
    let f = phase_frequency(&x, 1000.0, 12000.0, 30_000, 1200);
    assert!((f - 1002.5).abs() < 0.02, "{f}");
}

#[test]
fn sample_rate_error_scales_the_frequency() {
    for ppm in [150.0, -200.0] {
        let band = Band {
            rate_error_ppm: ppm,
            ..tone_band(10.0)
        };
        let x = tone(Channel::default(), band, 1);
        let f = phase_frequency(&x, 1000.0, 12000.0, 12_000, 24_000);
        let want = 1000.0 / (1.0 + ppm * 1e-6);
        assert!((f - want).abs() < 0.005, "{ppm} {f} {want}");
    }
    let x: Vec<f64> = (0..1000).map(|k| k as f64).collect();
    let y = stretch(&x, 1.0 + 1e-4);
    assert!((y[500] - 500.0 / 1.0001).abs() < 1e-6);
    assert!(stretch(&[], 2.0).is_empty());
}

#[test]
fn clock_offset_moves_dt() {
    let mut m = Ft8::new(12000, Options::default()).unwrap();
    for offset in [-0.8, 0.6] {
        let band = Band {
            clock_offset: offset,
            ..Band::slot(Source::Ft8)
        };
        let x = render(
            &band,
            Source::Ft8,
            &[Station::new("CQ JA1ZLO PM95", 1000.0, 0.0, -5.0)],
            2,
        )
        .unwrap();
        let got = m.push(&x, std::time::UNIX_EPOCH);
        assert!(
            (got[0].dt.unwrap() - offset).abs() < 0.02,
            "{offset} {got:?}"
        );
    }
}

fn envelope(x: &[f64], win: usize) -> Vec<f64> {
    x.chunks(win)
        .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

#[test]
fn qsb_and_fading_change_the_level_but_keep_the_mean_power() {
    let band = tone_band(20.0);
    let x = tone(
        Channel {
            qsb_hz: 0.2,
            qsb_db: 12.0,
            ..Channel::default()
        },
        band,
        1,
    );
    let e = envelope(&x, 1200);
    let (lo, hi) = e
        .iter()
        .fold((f64::MAX, 0f64), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!((20.0 * (hi / lo).log10() - 12.0).abs() < 0.5, "{lo} {hi}");
    for p in [Preset::MidModerate, Preset::HighDisturbed] {
        let mut power = 0.0;
        let mut depth = 0f64;
        for seed in 0..8 {
            let x = tone(
                p.channel(),
                Band {
                    seconds: 60.0,
                    ..band
                },
                seed,
            );
            let e = envelope(&x, 600);
            power += e.iter().map(|v| v * v).sum::<f64>() / e.len() as f64;
            let (lo, hi) = e
                .iter()
                .fold((f64::MAX, 0f64), |(a, b), v| (a.min(*v), b.max(*v)));
            depth = depth.max(20.0 * (hi / lo.max(1e-9)).log10());
        }
        let flat = envelope(
            &tone(
                Channel::default(),
                Band {
                    seconds: 60.0,
                    ..band
                },
                0,
            ),
            600,
        );
        let flat = flat.iter().map(|v| v * v).sum::<f64>() / flat.len() as f64;
        let ratio = 10.0 * (power / 8.0 / flat).log10();
        assert!(ratio.abs() < 1.5 && depth > 15.0, "{p:?} {ratio} {depth}");
    }
}

#[test]
fn faster_spread_fades_faster() {
    let band = tone_band(30.0);
    let crossings = |p: Preset| {
        let e = envelope(
            &tone(
                Channel {
                    delay: 0.0,
                    ..p.channel()
                },
                band,
                5,
            ),
            120,
        );
        let mean = e.iter().sum::<f64>() / e.len() as f64;
        e.windows(2)
            .filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0)
            .count()
    };
    let (slow, mid, fast) = (
        crossings(Preset::MidQuiet),
        crossings(Preset::MidDisturbed),
        crossings(Preset::HighDisturbed),
    );
    assert!(
        slow < mid && mid < fast && fast > 20 * slow.max(1),
        "{slow} {mid} {fast}"
    );
}

#[test]
fn qrn_adds_impulses_and_agc_pumps_the_level() {
    let quiet = render(&Band::audio(12000, 10.0), Source::Tone, &[], 1).unwrap();
    let band = Band {
        qrn: Qrn {
            rate: 5.0,
            level: 50.0,
        },
        ..Band::audio(12000, 10.0)
    };
    let crashes = render(&band, Source::Tone, &[], 1).unwrap();
    let peak = |x: &[f32]| x.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak(&crashes) > 10.0 * peak(&quiet));
    let loud = crashes.iter().filter(|v| v.abs() > 6.0 * NOISE).count();
    assert!(loud > 100 && loud < 12000, "{loud}");
    let band = Band {
        agc: Agc {
            depth_db: 10.0,
            hz: 0.5,
        },
        ..Band::audio(12000, 10.0)
    };
    let x: Vec<f64> = render(&band, Source::Tone, &[], 1)
        .unwrap()
        .into_iter()
        .map(|v| v as f64)
        .collect();
    let e = envelope(&x, 1200);
    let (lo, hi) = e
        .iter()
        .fold((f64::MAX, 0f64), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!(20.0 * (hi / lo).log10() > 8.0);
}

fn rate(
    m: &mut dyn Modem,
    source: Source,
    snr: f32,
    channel: Channel,
    band: Band,
    trials: u64,
) -> Trial {
    let mut sum = Trial::default();
    for seed in 0..trials {
        let st =
            Station::new("JA1ZLO JA1YWX PM95", 800.0 + 50.0 * seed as f32, 0.1, snr).with(channel);
        sum += ft_trial(m, source, &band, &[st], 900 + seed).unwrap();
    }
    sum
}

#[test]
fn ft8_awgn_decodes_at_minus_19_db() {
    let mut m = Ft8::new(12000, Options::default()).unwrap();
    let t = rate(
        &mut m,
        Source::Ft8,
        -19.0,
        Channel::default(),
        Band::slot(Source::Ft8),
        10,
    );
    assert!(t.decoded >= 7 && t.false_decodes == 0, "{t:?}");
}

#[test]
fn ft4_awgn_decodes_at_minus_14_db() {
    let mut m = Ft4::new(12000, Options::default()).unwrap();
    let t = rate(
        &mut m,
        Source::Ft4,
        -14.0,
        Channel::default(),
        Band::slot(Source::Ft4),
        10,
    );
    assert!(t.decoded >= 9 && t.false_decodes == 0, "{t:?}");
}

#[test]
fn ft8_survives_drift_clock_and_sample_rate_errors() {
    let band = Band {
        rate_error_ppm: 100.0,
        clock_offset: 0.8,
        ..Band::slot(Source::Ft8)
    };
    let mut m = Ft8::new(12000, Options::default()).unwrap();
    let t = rate(
        &mut m,
        Source::Ft8,
        -15.0,
        Channel {
            drift: 0.1,
            ..Channel::default()
        },
        band,
        10,
    );
    assert!(t.decoded >= 9, "{t:?}");
}

fn plain() -> Options {
    let mut o = Options::default();
    o.ft.blanker = false;
    o.ft.fading = false;
    o
}

#[test]
fn the_blanker_rescues_ft8_from_static_crashes() {
    let band = Band {
        qrn: Qrn {
            rate: 4.0,
            level: 60.0,
        },
        ..Band::slot(Source::Ft8)
    };
    let mut on = Ft8::new(12000, Options::default()).unwrap();
    let mut off = Ft8::new(12000, plain()).unwrap();
    let t = rate(&mut on, Source::Ft8, -14.0, Channel::default(), band, 5);
    let p = rate(&mut off, Source::Ft8, -14.0, Channel::default(), band, 5);
    assert!(
        t.decoded == 5 && p.decoded <= 1 && t.false_decodes == 0,
        "{t:?} {p:?}"
    );
}

const FLOORS: [(Preset, f32, usize); 5] = [
    (Preset::MidQuiet, -8.0, 2),
    (Preset::MidModerate, -12.0, 3),
    (Preset::MidDisturbed, -12.0, 3),
    (Preset::LowModerate, -12.0, 3),
    (Preset::LowDisturbed, -6.0, 1),
];

#[test]
fn ft8_decodes_through_each_moderate_preset() {
    let mut m = Ft8::new(12000, Options::default()).unwrap();
    for (preset, snr, least) in FLOORS {
        let t = rate(
            &mut m,
            Source::Ft8,
            snr,
            preset.channel(),
            Band::slot(Source::Ft8),
            3,
        );
        assert!(
            t.decoded >= least && t.false_decodes == 0,
            "{preset:?} {t:?}"
        );
    }
}

#[test]
fn ft4_decodes_through_each_moderate_preset() {
    let mut m = Ft4::new(12000, Options::default()).unwrap();
    for (preset, snr, least) in FLOORS.into_iter().skip(1).take(3) {
        let t = rate(
            &mut m,
            Source::Ft4,
            snr + 4.0,
            preset.channel(),
            Band::slot(Source::Ft4),
            5,
        );
        assert!(
            t.decoded >= least && t.false_decodes == 0,
            "{preset:?} {t:?}"
        );
    }
}

#[test]
#[ignore]
fn preset_sweep() {
    for (source, mut m) in [
        (
            Source::Ft8,
            Box::new(Ft8::new(12000, Options::default()).unwrap()) as Box<dyn Modem>,
        ),
        (
            Source::Ft4,
            Box::new(Ft4::new(12000, Options::default()).unwrap()),
        ),
    ] {
        for preset in Preset::ALL {
            let row: Vec<usize> = (-24..=-4)
                .step_by(2)
                .map(|snr| {
                    rate(
                        &mut *m,
                        source,
                        snr as f32,
                        preset.channel(),
                        Band::slot(source),
                        20,
                    )
                    .decoded
                })
                .collect();
            eprintln!("{source:?} {} {row:?}", preset.name());
        }
    }
}
