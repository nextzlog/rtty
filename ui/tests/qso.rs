use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rtty_ui::{Burst, Event, FtPanel, Mode, Qso};

const RATE: u32 = 12000;

fn slot(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_793_000_010 + 15 * n)
}

fn station(call: &str, hz: f32) -> (FtPanel, Rc<RefCell<Vec<Event>>>) {
    let mut p = FtPanel::new("en");
    p.my_call = call.into();
    p.my_grid = "PM95".into();
    p.tx_hz = hz;
    p.set_output_rate(RATE);
    p.start();
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    p.on_event(move |e| seen.borrow_mut().push(e.clone()));
    (p, log)
}

fn burst(log: &Rc<RefCell<Vec<Event>>>) -> Option<Burst> {
    log.borrow().iter().rev().find_map(|e| match e {
        Event::Burst(b) => Some(b.clone()),
        _ => None,
    })
}

fn air(from: &mut FtPanel, log: &Rc<RefCell<Vec<Event>>>, to: &mut FtPanel, n: u64) -> String {
    let start = slot(n);
    from.tick(start - Duration::from_millis(50));
    let b = burst(log).expect("a burst");
    assert_eq!(b.at, start + Duration::from_millis(500));
    let mut audio = vec![0.0f32; 15 * RATE as usize];
    let offset = RATE as usize / 2;
    for (k, v) in b.samples.iter().enumerate() {
        if let Some(x) = audio.get_mut(offset + k) {
            *x += 0.3 * v;
        }
    }
    audio.extend(vec![0.0; RATE as usize / 10]);
    let block = RATE as usize / 10;
    for (k, chunk) in audio.chunks(block).enumerate() {
        to.push_samples(RATE, chunk, start + Duration::from_millis(100 * k as u64));
    }
    from.tick(b.end());
    assert!(!from.sending());
    to.decodes()
        .last()
        .map(|d| d.text.clone())
        .unwrap_or_default()
}

fn completed(log: &Rc<RefCell<Vec<Event>>>) -> Vec<Qso> {
    log.borrow()
        .iter()
        .filter_map(|e| match e {
            Event::QsoCompleted(q) => Some(q.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_simulated_ft8_qso_between_two_panels_fires_qso_completed() {
    let (mut zlo, zlo_log) = station("JA1ZLO", 1200.0);
    let (mut ywx, ywx_log) = station("JA1YWX", 1700.0);
    zlo.seq.enabled = true;
    assert_eq!(air(&mut zlo, &zlo_log, &mut ywx, 0), "CQ JA1ZLO PM95");
    ywx.select(ywx.decodes().len() - 1);
    assert!(ywx_log
        .borrow()
        .iter()
        .any(|e| matches!(e, Event::Pick(p) if p.callsign == "JA1ZLO")));
    assert_eq!(air(&mut ywx, &ywx_log, &mut zlo, 1), "JA1ZLO JA1YWX PM95");
    assert!(air(&mut zlo, &zlo_log, &mut ywx, 2).starts_with("JA1YWX JA1ZLO "));
    assert!(air(&mut ywx, &ywx_log, &mut zlo, 3).starts_with("JA1ZLO JA1YWX R"));
    assert_eq!(air(&mut zlo, &zlo_log, &mut ywx, 4), "JA1YWX JA1ZLO RR73");
    assert_eq!(air(&mut ywx, &ywx_log, &mut zlo, 5), "JA1ZLO JA1YWX 73");
    assert!(!zlo.seq.enabled && !ywx.seq.enabled);
    let (a, b) = (completed(&zlo_log), completed(&ywx_log));
    assert_eq!((a.len(), b.len()), (1, 1), "{a:?} {b:?}");
    assert_eq!(
        (a[0].callsign.as_str(), b[0].callsign.as_str()),
        ("JA1YWX", "JA1ZLO")
    );
    assert_eq!((a[0].sent, a[0].rcvd), (b[0].rcvd, b[0].sent));
    assert_eq!((a[0].mode, a[0].grid.as_str()), (Mode::Ft8, "PM95"));
    let ptt: Vec<bool> = zlo_log
        .borrow()
        .iter()
        .filter_map(|e| match e {
            Event::Ptt(on) => Some(*on),
            _ => None,
        })
        .collect();
    assert_eq!(ptt, [true, false, true, false, true, false]);
}
