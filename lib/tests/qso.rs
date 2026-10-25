mod common;

use common::{check_slots, texts, Air, FtOp, Mode, Rig, RttyOp, Script, Style};
use rtty::sim::{Channel, Preset};
use rtty::{Options, Parity};

const HOUR: f64 = 1_800_000_000.0;

fn caller(mode: Mode, style: Style) -> Rig {
    Rig::new(
        "JA1ZLO",
        mode,
        FtOp::caller("JA1ZLO", "PM95", style, Parity::Even),
    )
    .hz(1200.0)
    .snr(-8.0)
}

fn answerer(mode: Mode, style: Style) -> Rig {
    Rig::new("JA1YWX", mode, FtOp::answerer("JA1YWX", "PM95", style))
        .hz(1650.0)
        .snr(-5.0)
}

fn close(got: &str, want: &str) -> bool {
    let (g, w): (Vec<&str>, Vec<&str>) = (got.split(' ').collect(), want.split(' ').collect());
    g.len() == w.len()
        && g.iter().zip(&w).all(|(a, b)| {
            let num = |s: &str| s.trim_start_matches('R').parse::<i32>().ok();
            match (num(a), num(b)) {
                (Some(x), Some(y)) if a.len() == b.len() && a.len() >= 3 => (x - y).abs() <= 2,
                _ => a == b,
            }
        })
}

fn assert_qso(air: &Air, a: &[&str], b: &[&str]) {
    let (sa, sb) = (texts(air, "JA1ZLO"), texts(air, "JA1YWX"));
    let ok = |got: &[String], want: &[&str]| {
        let mut got: Vec<&String> = got.iter().collect();
        got.dedup();
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| close(g, w))
    };
    assert!(ok(&sa, a) && ok(&sb, b), "{:#?}", air.log);
    assert_complete(air);
}

fn assert_complete(air: &Air) {
    let (a, b) = (air.rig("JA1ZLO"), air.rig("JA1YWX"));
    assert_eq!(a.op.partner().as_deref(), Some("JA1YWX"), "{:#?}", air.log);
    assert_eq!(b.op.partner().as_deref(), Some("JA1ZLO"), "{:#?}", air.log);
    if a.mode != Mode::Rtty {
        check_slots(air, "JA1ZLO", "JA1YWX");
        check_slots(air, "JA1YWX", "JA1ZLO");
    }
}

const FT_A: &[&str] = &["CQ JA1ZLO PM95", "JA1YWX JA1ZLO -05", "JA1YWX JA1ZLO RR73"];
const FT_B: &[&str] = &[
    "JA1ZLO JA1YWX PM95",
    "JA1ZLO JA1YWX R-08",
    "JA1ZLO JA1YWX 73",
];

fn qso(a: Rig, b: Rig, base: f64, seed: u64, limit: f64) -> Air {
    let mut air = Air::new(base, seed, vec![a, b]);
    air.run(limit);
    air
}

#[test]
fn ft8_full_qso() {
    let air = qso(
        caller(Mode::Ft8, Style::Standard),
        answerer(Mode::Ft8, Style::Standard),
        HOUR - 2.0,
        1,
        120.0,
    );
    assert_qso(&air, FT_A, FT_B);
    let (a, b) = (air.rig("JA1ZLO"), air.rig("JA1YWX"));
    assert!(a.sent.iter().all(|s| s.parity == Some(Parity::Even)));
    assert!(b.sent.iter().all(|s| s.parity == Some(Parity::Odd)));
    assert!(close(&format!("X {}", b.op.received().unwrap()), "X -05"));
    assert!(close(&format!("X R{}", a.op.received().unwrap()), "X R-08"));
}

#[test]
fn ft4_full_qso() {
    let air = qso(
        caller(Mode::Ft4, Style::Standard),
        answerer(Mode::Ft4, Style::Standard),
        HOUR - 2.0,
        2,
        60.0,
    );
    assert_qso(&air, FT_A, FT_B);
}

#[test]
fn ft4_contest_qso() {
    let air = qso(
        caller(Mode::Ft4, Style::Contest),
        answerer(Mode::Ft4, Style::Contest),
        HOUR - 2.0,
        3,
        60.0,
    );
    assert_qso(
        &air,
        &[
            "CQ TEST JA1ZLO PM95",
            "JA1YWX JA1ZLO R PM95",
            "JA1YWX JA1ZLO 73",
        ],
        &["JA1ZLO JA1YWX PM95", "JA1ZLO JA1YWX RRR"],
    );
}

fn crowd(n: usize, seed: u64) -> Vec<Rig> {
    (0..n / 2)
        .flat_map(|i| {
            let a = format!("JH{}{}", i % 10, ["AAA", "BBB", "CCC"][i / 10 % 3]);
            let b = format!("JR{}{}", i % 10, ["XXX", "YYY", "ZZZ"][i / 10 % 3]);
            let lines_a = [
                format!("CQ {a} PM95"),
                format!("{b} {a} -10"),
                format!("{b} {a} RR73"),
                format!("CQ {a} PM95"),
            ];
            let lines_b = [
                format!("{a} {b} QM05"),
                format!("{a} {b} R-12"),
                format!("{a} {b} 73"),
                format!("{a} {b} 73"),
            ];
            let hz = |k: usize| 250.0 + ((i * 2 + k) * 2500 / n) as f32 + (seed % 7) as f32 * 3.0;
            let snr = -18.0 + (i % 6) as f32 * 4.0;
            let pa = if i % 3 == 0 {
                Parity::Odd
            } else {
                Parity::Even
            };
            let pb = if pa == Parity::Even {
                Parity::Odd
            } else {
                Parity::Even
            };
            [
                Rig::new(&a, Mode::Ft8, Script::new(&lines_a, Some(pa)))
                    .hz(hz(0))
                    .snr(snr)
                    .deaf(),
                Rig::new(&b, Mode::Ft8, Script::new(&lines_b, Some(pb)))
                    .hz(hz(1))
                    .snr(snr + 2.0)
                    .deaf(),
            ]
        })
        .collect()
}

#[test]
fn ft8_qso_in_a_crowded_band() {
    let mut rigs = vec![
        caller(Mode::Ft8, Style::Standard).hz(1210.0),
        Rig::new(
            "JA1YWX",
            Mode::Ft8,
            FtOp::answerer("JA1YWX", "PM95", Style::Standard).to("JA1ZLO"),
        )
        .hz(1290.0)
        .snr(-5.0),
    ];
    rigs.extend(crowd(20, 4));
    let mut air = Air::new(HOUR - 2.0, 4, rigs);
    air.run(150.0);
    assert_qso(&air, FT_A, FT_B);
    let others = air
        .rig("JA1ZLO")
        .heard
        .iter()
        .filter(|d| d.text.contains("JH") || d.text.contains("JR"))
        .count();
    assert!(others >= 30, "{others}");
}

#[test]
fn ft8_pileup_of_five_callers() {
    let mut rigs = vec![caller(Mode::Ft8, Style::Standard)];
    for (i, call) in ["JA1YWX", "JH1AAA", "JR2BBB", "JE3CCC", "JF4DDD"]
        .iter()
        .enumerate()
    {
        let snr = if i == 0 { -2.0 } else { -6.0 - 2.0 * i as f32 };
        rigs.push(
            Rig::new(
                call,
                Mode::Ft8,
                FtOp::answerer(call, "PM85", Style::Standard).patience(2),
            )
            .hz(600.0 + 300.0 * i as f32)
            .snr(snr),
        );
    }
    let mut air = Air::new(HOUR - 2.0, 5, rigs);
    air.run(240.0);
    assert_complete(&air);
    for call in ["JH1AAA", "JR2BBB", "JE3CCC", "JF4DDD"] {
        assert_eq!(air.rig(call).op.partner(), None, "{call}");
    }
    let calls = air
        .log
        .iter()
        .filter(|l| l.text.starts_with("JA1ZLO ") && l.text.ends_with("PM85"))
        .count();
    assert!(calls >= 5, "{calls}");
}

#[test]
fn ft8_qso_with_weak_signals_near_the_threshold() {
    let air = qso(
        caller(Mode::Ft8, Style::Standard).snr(-18.0),
        answerer(Mode::Ft8, Style::Standard).snr(-18.0),
        HOUR - 2.0,
        6,
        240.0,
    );
    assert_complete(&air);
}

#[test]
fn ft8_qso_through_moderate_fading_and_qsb() {
    let qsb = Channel {
        qsb_hz: 0.05,
        qsb_db: 8.0,
        ..Preset::MidModerate.channel()
    };
    let air = qso(
        caller(Mode::Ft8, Style::Standard).channel(qsb).snr(-12.0),
        answerer(Mode::Ft8, Style::Standard)
            .channel(Preset::MidModerate.channel())
            .snr(-12.0),
        HOUR - 2.0,
        7,
        240.0,
    );
    assert_complete(&air);
}

#[test]
fn ft8_qso_with_drift() {
    let drift = |d| Channel {
        drift: d,
        ..Channel::default()
    };
    let air = qso(
        caller(Mode::Ft8, Style::Standard).channel(drift(0.1)),
        answerer(Mode::Ft8, Style::Standard).channel(drift(-0.1)),
        HOUR - 2.0,
        8,
        150.0,
    );
    assert_qso(&air, FT_A, FT_B);
}

#[test]
fn ft8_qso_with_clock_offsets() {
    for (seed, (ca, cb)) in [(1.0, 0.0), (-0.5, 0.5)].into_iter().enumerate() {
        let air = qso(
            caller(Mode::Ft8, Style::Standard).clock(ca),
            answerer(Mode::Ft8, Style::Standard).clock(cb),
            HOUR - 3.0,
            9 + seed as u64,
            150.0,
        );
        assert_qso(&air, FT_A, FT_B);
    }
}

#[test]
fn ft8_qso_with_sound_card_rate_errors_and_mixed_rates() {
    let air = qso(
        caller(Mode::Ft8, Style::Standard)
            .rate(48000, 21.3)
            .ppm(150.0),
        answerer(Mode::Ft8, Style::Standard)
            .rate(44100, 93.0)
            .ppm(-200.0),
        HOUR - 2.0,
        11,
        150.0,
    );
    assert_qso(&air, FT_A, FT_B);
}

#[test]
fn ft4_qso_at_12_and_48_khz() {
    let air = qso(
        caller(Mode::Ft4, Style::Standard)
            .rate(48000, 50.0)
            .ppm(100.0),
        answerer(Mode::Ft4, Style::Standard).rate(12000, 20.0),
        HOUR - 1.0,
        12,
        90.0,
    );
    assert_qso(&air, FT_A, FT_B);
}

#[test]
fn a_lost_report_is_repeated() {
    let air = qso(
        caller(Mode::Ft8, Style::Standard).lose(&[1]),
        answerer(Mode::Ft8, Style::Standard).lose(&[3]),
        HOUR - 2.0,
        13,
        240.0,
    );
    assert_complete(&air);
    let a = texts(&air, "JA1ZLO");
    let b = texts(&air, "JA1YWX");
    assert!(
        a.iter()
            .filter(|t| t.ends_with("-05") || t.contains("JA1ZLO -0"))
            .count()
            >= 2,
        "{a:?}"
    );
    assert!(
        a.iter().filter(|t| t.ends_with("RR73")).count() >= 2,
        "{a:?}"
    );
    assert!(
        b.iter().filter(|t| t.ends_with("PM95")).count() >= 2,
        "{b:?}"
    );
    assert!(
        b.iter().filter(|t| t.ends_with(" 73")).count() >= 2,
        "{b:?}"
    );
}

#[test]
fn a_qso_without_its_roger_never_completes() {
    let lost: Vec<usize> = (1..20).collect();
    let air = qso(
        caller(Mode::Ft8, Style::Standard),
        answerer(Mode::Ft8, Style::Standard).lose(&lost),
        HOUR - 2.0,
        14,
        200.0,
    );
    assert_eq!(air.rig("JA1ZLO").op.partner(), None);
    assert_eq!(air.rig("JA1YWX").op.partner(), None);
    assert!(texts(&air, "JA1ZLO").iter().all(|t| !t.ends_with("RR73")));
}

#[test]
fn slots_across_the_hour_with_odd_block_sizes() {
    let air = qso(
        caller(Mode::Ft8, Style::Standard).rate(12000, 37.0),
        answerer(Mode::Ft8, Style::Standard).rate(48000, 61.7),
        HOUR - 31.0,
        15,
        150.0,
    );
    assert_qso(&air, FT_A, FT_B);
    let first = air.rig("JA1ZLO").sent[0].start;
    assert!(
        first < common::at(HOUR) && air.rig("JA1ZLO").sent.last().unwrap().start > common::at(HOUR)
    );
}

const RTTY_A: &[&str] = &[
    "CQ TEST JA1ZLO JA1ZLO",
    "JA1YWX 599 001 001",
    "TU JA1ZLO TEST",
];
const RTTY_B: &[&str] = &["JA1YWX", "TU 599 012 012"];

fn rtty(a: Rig, b: Rig, extra: Vec<Rig>, seed: u64) -> Air {
    let mut rigs = vec![a, b];
    rigs.extend(extra);
    let mut air = Air::new(HOUR, seed, rigs);
    air.run(80.0);
    air
}

fn rtty_pair() -> (Rig, Rig) {
    (
        Rig::new(
            "JA1ZLO",
            Mode::Rtty,
            RttyOp::caller("JA1ZLO", "599 001 001"),
        )
        .snr(5.0),
        Rig::new(
            "JA1YWX",
            Mode::Rtty,
            RttyOp::answerer("JA1YWX", "599 012 012"),
        )
        .snr(5.0),
    )
}

fn assert_rtty(air: &Air) {
    assert_qso(air, RTTY_A, RTTY_B);
    assert_eq!(air.rig("JA1ZLO").op.received().as_deref(), Some("012"));
    assert_eq!(air.rig("JA1YWX").op.received().as_deref(), Some("001"));
}

#[test]
fn rtty_contest_exchange() {
    let (a, b) = rtty_pair();
    assert_rtty(&rtty(a, b, vec![], 16));
}

#[test]
fn rtty_exchange_through_fading_at_48_khz() {
    let (a, b) = rtty_pair();
    let fade = Preset::MidModerate.channel();
    let air = rtty(
        a.snr(15.0).channel(fade).rate(48000, 40.0),
        b.snr(15.0).channel(fade),
        vec![],
        17,
    );
    assert_complete(&air);
    assert_eq!(air.rig("JA1ZLO").op.received().as_deref(), Some("012"));
}

#[test]
fn rtty_exchange_next_to_another_signal() {
    let (a, b) = rtty_pair();
    let qrm = Rig::new(
        "JE3CCC",
        Mode::Rtty,
        Script::new(
            &vec!["CQ CQ TEST JE3CCC JE3CCC JE3CCC TEST".repeat(3); 9],
            None,
        ),
    )
    .hz(2125.0 + 350.0)
    .snr(10.0)
    .deaf();
    let air = rtty(a, b, vec![qrm], 18);
    assert_rtty(&air);
}

#[test]
fn rtty_reverse_mismatch_makes_no_contact() {
    let (a, b) = rtty_pair();
    let mut reversed = Options::default();
    reversed.rtty.reverse = true;
    let air = rtty(a, b.options(reversed), vec![], 19);
    assert_eq!(air.rig("JA1ZLO").op.partner(), None);
    assert_eq!(air.rig("JA1YWX").op.partner(), None);
    let (a, b) = rtty_pair();
    let air = rtty(a.options(reversed), b.options(reversed), vec![], 19);
    assert_rtty(&air);
}

#[test]
#[ignore]
fn rtty_exchange_under_flutter() {
    let (a, b) = rtty_pair();
    let flutter = Preset::Flutter.channel();
    let air = rtty(
        a.snr(20.0).channel(flutter),
        b.snr(20.0).channel(flutter),
        vec![],
        20,
    );
    assert_complete(&air);
}

#[test]
fn ft4_qso_under_disturbed_mid_latitude_fading() {
    let fade = Preset::MidDisturbed.channel();
    let air = qso(
        caller(Mode::Ft4, Style::Standard).channel(fade).snr(-6.0),
        answerer(Mode::Ft4, Style::Standard).channel(fade).snr(-6.0),
        HOUR - 2.0,
        21,
        120.0,
    );
    assert_complete(&air);
}

#[test]
#[ignore]
fn rtty_exchange_under_disturbed_mid_latitude_fading() {
    let (a, b) = rtty_pair();
    let fade = Preset::MidDisturbed.channel();
    let air = rtty(
        a.snr(20.0).channel(fade),
        b.snr(20.0).channel(fade),
        vec![],
        22,
    );
    assert_complete(&air);
}
