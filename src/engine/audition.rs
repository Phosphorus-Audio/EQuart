use crate::band::{BAND_COUNT, BandControl, ChannelMode};
use crate::dsp::{self, State};
use crate::filter::{Coefficients, FilterKind};
use crate::precision::Sample;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Band(usize),
    Region(usize),
}

impl Target {
    pub(crate) fn encode(self) -> u32 {
        match self {
            Self::Band(index) => index as u32 + 1,
            Self::Region(index) => (BAND_COUNT + index) as u32 + 1,
        }
    }

    pub(crate) fn decode(value: u32) -> Option<Self> {
        let index = value.checked_sub(1)? as usize;
        Some(if index < BAND_COUNT {
            Self::Band(index)
        } else {
            Self::Region(index - BAND_COUNT)
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Focus {
    sections: [(FilterKind, f32, f32); 2],
    count: usize,
    route: ChannelMode,
}

impl Focus {
    pub(super) fn band(band: BandControl, frequency: f32) -> Self {
        let (kind, q) = match band.kind {
            FilterKind::Highpass | FilterKind::LowShelf => (FilterKind::Lowpass, 0.707),
            FilterKind::Lowpass | FilterKind::HighShelf => (FilterKind::Highpass, 0.707),
            _ => (FilterKind::BandPass, band.q),
        };

        Self {
            sections: [(kind, frequency, q); 2],
            count: 1,
            route: band.channel,
        }
    }

    pub(super) fn region([low, high]: [f32; 2]) -> Self {
        Self {
            sections: [
                (FilterKind::Highpass, low, 0.707),
                (FilterKind::Lowpass, high, 0.707),
            ],
            count: 2,
            route: ChannelMode::Stereo,
        }
    }
}

pub(super) struct Audition {
    target: Option<Target>,
    focus: Option<(Focus, f32)>,
    coefficients: [Coefficients<f64>; 2],
    state: [[State<f64>; 2]; 2],
    mix: f64,
}

impl Default for Audition {
    fn default() -> Self {
        Self {
            target: None,
            focus: None,
            coefficients: [Coefficients::identity(); 2],
            state: Default::default(),
            mix: 0.0,
        }
    }
}

impl Audition {
    pub(super) fn process<T: Sample>(
        &mut self,
        left: &mut [T],
        right: &mut [T],
        rate: f32,
        requested: Option<(Target, Focus)>,
    ) {
        let requested_target = requested.map(|(target, _)| target);
        if self.mix == 0.0 && self.target != requested_target {
            self.target = requested_target;
            self.state = Default::default();
        }

        let Some(target) = self.target else { return };
        if let Some((_, focus)) = requested.filter(|(requested, _)| *requested == target)
            && self.focus != Some((focus, rate))
        {
            self.focus = Some((focus, rate));
            self.coefficients = focus.sections.map(|(kind, hz, q)| {
                Coefficients::for_kind(
                    f64::from(rate),
                    f64::from(hz.clamp(10.0, rate * 0.49)),
                    f64::from(q),
                    0.0,
                    kind,
                )
            });
        }

        let (focus, _) = self.focus.expect("active audition has a filter");
        let step = 1.0 / (0.005 * f64::from(rate));
        let step = if requested_target == self.target {
            step
        } else {
            -step
        };

        for (left, right) in left.iter_mut().zip(right) {
            self.mix = (self.mix + step).clamp(0.0, 1.0);
            if self.mix == 0.0 {
                self.target = None;
                break;
            }

            let [l, r] = [left.double(), right.double()];
            let mut audible = match focus.route {
                ChannelMode::Stereo => [l, r],
                ChannelMode::Left => [l, 0.0],
                ChannelMode::Right => [0.0, r],
                ChannelMode::Mid => [(l + r) * 0.5; 2],
                ChannelMode::Side => [(l - r) * 0.5, (r - l) * 0.5],
            };

            for (states, coefficients) in self
                .state
                .iter_mut()
                .zip(&self.coefficients)
                .take(focus.count)
            {
                for (sample, state) in audible.iter_mut().zip(states) {
                    *sample = dsp::step(state, coefficients, *sample);
                }
            }

            *left = T::value(l + self.mix * (audible[0] - l));
            *right = T::value(r + self.mix * (audible[1] - r));
        }
    }
}
