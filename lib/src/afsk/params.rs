use crate::modem::RttyOptions;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Params {
    pub mark: f64,
    pub space: f64,
    pub baud: f64,
    pub uos: bool,
    pub squelch: f64,
    pub atc: bool,
    pub blanker: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params::new(&RttyOptions::default(), None)
    }
}

impl Params {
    pub fn new(o: &RttyOptions, mark: Option<f32>) -> Self {
        let mark = mark.unwrap_or(o.mark) as f64;
        let space = mark + o.shift as f64;
        let (mark, space) = if o.reverse {
            (space, mark)
        } else {
            (mark, space)
        };
        Params {
            mark,
            space,
            baud: o.baud as f64,
            uos: o.uos,
            squelch: o.squelch as f64,
            atc: o.atc,
            blanker: o.blanker,
        }
    }

    pub fn bit_len(&self, rate: u32) -> f64 {
        rate as f64 / self.baud.max(1.0)
    }
}
