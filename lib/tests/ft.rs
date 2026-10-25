mod common;

use common::at;
use rtty::sim::{ft_trial, render, Band, Source, Station};
use rtty::{Decode, Ft4, Ft8, Modem, Options, Parity, RttyOptions};

fn ft8() -> Ft8 {
    Ft8::new(12000, Options::default()).unwrap()
}

fn decode(m: &mut dyn Modem, source: Source, stations: &[Station], seed: u64) -> Vec<Decode> {
    let x = render(&Band::slot(source), source, stations, seed).unwrap();
    m.push(&x, at(0.0))
}

#[test]
fn dt_df_and_snr_are_measured() {
    for (i, (hz, dt, snr)) in [
        (512.3, -0.8, -5.0),
        (1234.5, 0.0, -12.0),
        (2345.6, 1.7, -16.0),
    ]
    .into_iter()
    .enumerate()
    {
        let st = Station::new("JA1ZLO JA1YWX -12", hz, dt, snr);
        let got = decode(&mut ft8(), Source::Ft8, &[st], 40 + i as u64);
        assert_eq!(got.len(), 1);
        let s = &got[0];
        assert!((s.hz - hz).abs() < 0.5, "df {} vs {hz}", s.hz);
        assert!((s.dt.unwrap() - dt).abs() < 0.02, "dt {:?} vs {dt}", s.dt);
        assert!((s.snr - snr).abs() < 2.0, "snr {} vs {snr}", s.snr);
    }
}

fn busy(m: &mut dyn Modem, source: Source, n: usize, weakest: f32, seed: u64) -> (usize, usize) {
    let stations: Vec<Station> = (0..n)
        .map(|i| {
            let call = format!(
                "JA{}{}{}",
                i % 10,
                char::from(b'A' + (i / 10) as u8),
                char::from(b'A' + i as u8 % 26)
            );
            let hz = 200.0 + i as f32 * 2600.0 / n as f32;
            let snr = weakest + (i % 7) as f32 * 3.0;
            let dt = (i % 5) as f32 * 0.1 - 0.2;
            Station::new(&format!("CQ {call} PM95"), hz, dt, snr)
        })
        .collect();
    let t = ft_trial(m, source, &Band::slot(source), &stations, seed).unwrap();
    (t.decoded, t.false_decodes)
}

#[test]
fn a_crowded_ft8_slot_decodes_every_station() {
    assert_eq!(busy(&mut ft8(), Source::Ft8, 30, -16.0, 9), (30, 0));
}

#[test]
fn a_crowded_ft4_slot_decodes_every_station() {
    let mut m = Ft4::new(12000, Options::default()).unwrap();
    assert_eq!(busy(&mut m, Source::Ft4, 12, -12.0, 9), (12, 0));
}

#[test]
fn overlapping_signals_are_separated_by_subtraction() {
    let a = Station::new("CQ JA1ZLO PM95", 1000.0, 0.0, -2.0);
    let b = Station::new("JA1ZLO JA1YWX PM95", 1020.0, 0.3, -12.0);
    let got = decode(&mut ft8(), Source::Ft8, &[a, b], 77);
    let texts: Vec<&str> = got.iter().map(|s| s.text.as_str()).collect();
    assert!(
        texts.contains(&"CQ JA1ZLO PM95") && texts.contains(&"JA1ZLO JA1YWX PM95"),
        "{texts:?}"
    );
}

#[test]
fn noise_and_wrong_modes_decode_nothing() {
    let mut m8 = ft8();
    let mut m4 = Ft4::new(12000, Options::default()).unwrap();
    for seed in 0..3 {
        assert!(decode(&mut m8, Source::Ft8, &[], seed).is_empty());
        assert!(decode(&mut m4, Source::Ft4, &[], seed).is_empty());
        let rtty = [Station::new(
            "CQ TEST JA1ZLO JA1ZLO TEST ".repeat(3).as_str(),
            1500.0,
            0.0,
            10.0,
        )];
        let band = Band::slot(Source::Ft8);
        let x = render(&band, Source::Rtty(RttyOptions::default()), &rtty, seed).unwrap();
        assert!(m8.push(&x, at(0.0)).is_empty());
        assert!(m4.push(&x[..90_000], at(0.0)).is_empty());
        let ft4 = [Station::new("CQ JA1ZLO PM95", 1500.0, 0.0, 0.0)];
        assert!(decode(&mut m8, Source::Ft4, &ft4, seed).is_empty());
        let x = render(
            &band,
            Source::Ft8,
            &[Station::new("CQ JA1ZLO PM95", 1500.0, 0.0, 0.0)],
            seed,
        )
        .unwrap();
        assert!(m4.push(&x[..90_000], at(0.0)).is_empty());
    }
}

#[test]
fn the_receiver_aligns_samples_to_utc_slots() {
    let x = render(
        &Band::slot(Source::Ft8),
        Source::Ft8,
        &[Station::new("CQ JA1ZLO PM95", 1500.0, 0.0, -8.0)],
        5,
    )
    .unwrap();
    let t0 = 1_800_000_000.0 + 27.0;
    let mut rx = ft8();
    assert!(rx.push(&vec![0f32; 36000], at(t0)).is_empty());
    let (a, b) = x.split_at(100_000);
    assert!(rx.push(a, at(t0 + 3.0)).is_empty());
    let got = rx.push(b, at(t0 + 3.0 + 100_000.0 / 12000.0));
    assert_eq!(got.len(), 1);
    let d = &got[0];
    assert_eq!(
        (d.slot, d.unit),
        (Some(at(1_800_000_030.0)), rtty::Unit::Message)
    );
    assert_eq!(d.at, at(1_800_000_045.0));
    assert_eq!(
        Parity::of(d.slot.unwrap(), std::time::Duration::from_secs(15)),
        Parity::Even
    );
}

#[test]
fn a_gap_in_the_samples_restarts_the_slot_but_jitter_does_not() {
    let x = render(
        &Band::slot(Source::Ft8),
        Source::Ft8,
        &[Station::new("CQ JA1ZLO PM95", 1500.0, 0.0, -8.0)],
        5,
    )
    .unwrap();
    let mut rx = ft8();
    let t0 = 1_800_000_000.0;
    assert!(rx.push(&x[..90_000], at(t0)).is_empty());
    assert!(rx.push(&x[90_000..], at(t0 + 8.0)).is_empty());
    let got = rx.push(&x, at(t0 + 15.0));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].slot, Some(at(t0 + 15.0)));
    let mut rx = ft8();
    let mut got = Vec::new();
    for (i, c) in x.chunks(960).enumerate() {
        let jitter = [0.0, 0.004, -0.006, 0.009][i % 4];
        got.extend(rx.push(c, at(t0 + i as f64 * 0.08 + jitter)));
    }
    assert_eq!(got.len(), 1);
}

#[test]
fn ft4_slots_are_seven_and_a_half_seconds() {
    let x = render(
        &Band::slot(Source::Ft4),
        Source::Ft4,
        &[Station::new("CQ JA1ZLO PM95", 1200.0, 0.1, -8.0)],
        6,
    )
    .unwrap();
    let mut rx = Ft4::new(12000, Options::default()).unwrap();
    let got = rx.push(&x, at(1_800_000_007.5));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].slot, Some(at(1_800_000_007.5)));
}

#[test]
fn rx_frequency_priority_and_range() {
    let st = [
        Station::new("CQ JA1ZLO PM95", 500.0, 0.0, -5.0),
        Station::new("CQ JA1YWX PM95", 1500.0, 0.0, -15.0),
        Station::new("CQ JR2XYZ PM74", 2500.0, 0.0, -10.0),
    ];
    let x = render(&Band::slot(Source::Ft8), Source::Ft8, &st, 8).unwrap();
    let run = |o: Options| Ft8::new(12000, o).unwrap().push(&x, at(0.0));
    let got = run(Options {
        rx_hz: Some(1500.0),
        ..Options::default()
    });
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].text, "CQ JA1YWX PM95");
    assert_eq!(
        run(Options {
            min_hz: 1000.0,
            max_hz: 2000.0,
            ..Options::default()
        })
        .len(),
        1
    );
    let mut o = Options::default();
    o.ft.passes = 1;
    assert_eq!(run(o).len(), 3);
}

#[test]
fn other_sample_rates_are_converted_with_anti_aliasing() {
    let band = Band::audio(48000, 15.0);
    let st = [Station::new("CQ JA1ZLO PM95", 1500.0, 0.0, -10.0)];
    let mut x = render(&band, Source::Ft8, &st, 11).unwrap();
    for (k, v) in x.iter_mut().enumerate() {
        *v += 2.0 * (std::f64::consts::TAU * 10500.0 * k as f64 / 48000.0).sin() as f32;
    }
    let mut rx = Ft8::new(48000, Options::default()).unwrap();
    let t0 = 1_800_000_000.0;
    let mut got = Vec::new();
    for (i, chunk) in x.chunks(4801).enumerate() {
        got.extend(rx.push(chunk, at(t0 + (i * 4801) as f64 / 48000.0)));
    }
    got.extend(rx.push(&[0.0; 48000], at(t0 + 15.0)));
    assert_eq!(got.len(), 1, "{got:?}");
    assert!((got[0].hz - 1500.0).abs() < 0.5 && got[0].dt.unwrap().abs() < 0.02);
}
