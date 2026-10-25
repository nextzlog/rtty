use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rtty::sim::Rng;
use rtty::{fields, pack, unpack, Ft4, Ft8, Modem, Options, Rtty};

fn at(secs: f64) -> SystemTime {
    if secs >= 0.0 {
        UNIX_EPOCH + Duration::from_secs_f64(secs)
    } else {
        UNIX_EPOCH - Duration::from_secs_f64(-secs)
    }
}

fn random_options(rng: &mut Rng) -> Options {
    let mut pick = |normal: f32| -> f32 {
        match rng.next_u64() % 6 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => -normal,
            3 => normal * 1e6,
            4 => 0.0,
            _ => normal,
        }
    };
    let mut o = Options {
        min_hz: pick(100.0),
        max_hz: pick(3000.0),
        rx_hz: Some(pick(1500.0)),
        ..Options::default()
    };
    o.ft.threshold = pick(1.3);
    o.ft.rx_width = pick(100.0);
    o.rtty.mark = pick(2125.0);
    o.rtty.shift = pick(170.0);
    o.rtty.baud = pick(45.45);
    o.rtty.squelch = pick(0.5);
    o
}

#[test]
fn seeded_streams_with_bad_input_never_panic() {
    let mut rng = Rng::new(2024);
    for round in 0..24 {
        let rate = [8000, 11025, 12000, 44100, 48000, 96000][round % 6];
        let o = random_options(&mut rng);
        let mut modems: Vec<Box<dyn Modem>> = vec![
            Box::new(Ft8::new(rate, o).unwrap()),
            Box::new(Ft4::new(rate, o).unwrap()),
            Box::new(Rtty::new(rate, o).unwrap()),
        ];
        let mut t = [-20.0, 1_799_999_990.0, 3599.9][round % 3];
        for _ in 0..40 {
            let len = (rng.next_u64() % (rate as u64 / 4)) as usize;
            let x: Vec<f32> = (0..len)
                .map(|_| match rng.next_u64() % 500 {
                    0 => f32::NAN,
                    1 => f32::INFINITY,
                    2 => 1e30,
                    _ => (0.1 * rng.gauss()) as f32,
                })
                .collect();
            let jitter = match rng.next_u64() % 10 {
                0 => 5.0,
                1 => -0.5,
                _ => 0.015 * (rng.uniform() - 0.5),
            };
            for m in modems.iter_mut() {
                m.push(&x, at(t + jitter));
                m.set_options(random_options(&mut rng));
                let _ = m.transmit("CQ JA1ZLO PM95", 1500.0, at(t));
            }
            t += len as f64 / rate as f64;
        }
    }
}

#[test]
fn every_unpacked_payload_packs_back_to_the_same_text() {
    let mut rng = Rng::new(77);
    let mut bad = Vec::new();
    for _ in 0..200_000 {
        let bits = ((rng.next_u64() as u128) << 13 ^ rng.next_u64() as u128) & ((1 << 77) - 1);
        let bits = bits & !7 | (rng.next_u64() % 2) as u128;
        if let Some(t) = unpack(bits) {
            if t.contains('<') {
                continue;
            }
            if pack(&t).ok().and_then(unpack).as_deref() != Some(t.as_str()) {
                bad.push(t);
            }
        }
    }
    bad.truncate(20);
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn fields_follow_the_packed_layout() {
    let f = fields("CQ 123 FN31").unwrap();
    assert_eq!((f.to.as_str(), f.de.as_str()), ("CQ", "123"));
}
