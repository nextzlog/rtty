use std::time::Duration;

use rtty_ui::{Event, RttyPanel};

#[test]
fn a_host_keyer_confirms_before_the_burst_and_releases_after_it() {
    let mut p = RttyPanel::new("en");
    p.set_output_rate(8000);
    p.set_ptt_lead(Some(Duration::from_millis(300)));
    p.set_dial(14_085_000);
    p.send("cq test");
    let ev = p.take_events();
    assert_eq!(ev, [Event::Ptt(true)]);
    p.keyed(true);
    let ev = p.take_events();
    let [Event::Burst(b)] = &ev[..] else {
        panic!("{ev:?}")
    };
    p.tick(b.end() + Duration::from_secs(1));
    assert_eq!(p.take_events(), [Event::Ptt(false)]);
    assert!(p.sending());
    p.keyed(false);
    assert!(!p.sending());
    p.send("qrz");
    p.halt();
    assert_eq!(
        p.take_events(),
        [Event::Ptt(true), Event::Halt, Event::Ptt(false)]
    );
}
