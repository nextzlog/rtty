mod common;

use common::{texts, Air, Rig, RttyOp, Script};
use rtty::sim::Preset;
use rtty::Options;

const HOUR: f64 = 1_800_000_000.0;

fn assert_qso(air: &Air, a: &[&str], b: &[&str]) {
    let (sa, sb) = (texts(air, "JA1ZLO"), texts(air, "JA1YWX"));
    let ok = |got: &[String], want: &[&str]| {
        let mut got: Vec<&String> = got.iter().collect();
        got.dedup();
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| g == w)
    };
    assert!(ok(&sa, a) && ok(&sb, b), "{:#?}", air.log);
    assert_complete(air);
}

fn assert_complete(air: &Air) {
    let (a, b) = (air.rig("JA1ZLO"), air.rig("JA1YWX"));
    assert_eq!(a.op.partner().as_deref(), Some("JA1YWX"), "{:#?}", air.log);
    assert_eq!(b.op.partner().as_deref(), Some("JA1ZLO"), "{:#?}", air.log);
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
        Rig::new("JA1ZLO", RttyOp::caller("JA1ZLO", "599 001 001")).snr(5.0),
        Rig::new("JA1YWX", RttyOp::answerer("JA1YWX", "599 012 012")).snr(5.0),
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
        Script::new(&vec!["CQ CQ TEST JE3CCC JE3CCC JE3CCC TEST".repeat(3); 9]),
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
