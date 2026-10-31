use std::time::{Duration, SystemTime};

const SLIP: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, Default)]
pub struct Clock {
    base: Option<(SystemTime, u64)>,
}

fn after(t: SystemTime, n: u64, rate: u32) -> SystemTime {
    t + Duration::from_secs_f64(n as f64 / rate.max(1) as f64)
}

impl Clock {
    pub fn stamp(&mut self, len: usize, rate: u32, now: SystemTime) -> SystemTime {
        let span = Duration::from_secs_f64(len as f64 / rate.max(1) as f64);
        let fresh = now.checked_sub(span).unwrap_or(now);
        let (base, count) = match self.base {
            Some((b, c)) => {
                let t = after(b, c, rate);
                let gap = t.duration_since(fresh).unwrap_or_else(|e| e.duration());
                if gap < SLIP {
                    (b, c)
                } else {
                    (fresh, 0)
                }
            }
            None => (fresh, 0),
        };
        self.base = Some((base, count + len as u64));
        after(base, count, rate)
    }

    pub fn reset(&mut self) {
        self.base = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_follow_on_and_a_slip_restarts_the_count() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut c = Clock::default();
        assert_eq!(c.stamp(1200, 12000, t0), t0 - Duration::from_millis(100));
        let late = t0 + Duration::from_millis(130);
        assert_eq!(c.stamp(1200, 12000, late), t0);
        let gone = t0 + Duration::from_secs(5);
        assert_eq!(
            c.stamp(1200, 12000, gone),
            gone - Duration::from_millis(100)
        );
        c.reset();
        assert_eq!(c.stamp(0, 12000, t0), t0);
    }
}
