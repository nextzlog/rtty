use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rtty::sim::Rng;
use rtty::{Modem, Options, Rtty};

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
        let mut m = Rtty::new(rate, o).unwrap();
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
            m.push(&x, at(t + jitter));
            m.set_options(random_options(&mut rng));
            let _ = m.transmit("CQ TEST JA1ZLO", 1500.0, at(t));
            t += len as f64 / rate as f64;
        }
    }
}
