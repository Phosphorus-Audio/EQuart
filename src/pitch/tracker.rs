use super::{LOOKAHEAD, PERIODIC_BINS, candidates::Observations};

const HISTORY: usize = LOOKAHEAD + 1;
const VOICE_SWITCH: f32 = 0.01;
const NEW_NOTE: f32 = 0.01;
// The largest supported transition window uses the 128-sample hop at 8 kHz.
const MAX_RADIUS: usize = 69;

struct Frame<const BINS: usize, const STATES: usize> {
    threshold: f32,
    previous: [u16; STATES],
    frequency: [f32; BINS],
    clarity: [f32; BINS],
}

pub(super) struct Tracker<
    const PITCH_BINS: usize = PERIODIC_BINS,
    const BINS: usize = { crate::pitch::BINS },
    const STATES: usize = { crate::pitch::STATES },
> {
    score: Vec<f32>,
    next: Vec<f32>,
    history: Vec<Frame<BINS, STATES>>,
    frames: usize,
    radius: usize,
    transitions: Vec<f32>,
}

impl<const PITCH_BINS: usize, const BINS: usize, const STATES: usize>
    Tracker<PITCH_BINS, BINS, STATES>
{
    pub fn new(hop_seconds: f32) -> Self {
        let mut tracker = Self {
            score: vec![f32::NEG_INFINITY; STATES],
            next: vec![f32::NEG_INFINITY; STATES],
            history: (0..HISTORY)
                .map(|_| Frame {
                    threshold: 0.0,
                    previous: [0; STATES],
                    frequency: [0.0; BINS],
                    clarity: [0.0; BINS],
                })
                .collect(),
            frames: 0,
            radius: 0,
            transitions: Vec::with_capacity((MAX_RADIUS + 1).pow(2)),
        };
        tracker.set_hop(hop_seconds);
        tracker.reset();
        tracker
    }

    pub fn set_hop(&mut self, hop_seconds: f32) {
        // scale 2.5-semitone window from 256/44100s hop to our hop
        let radius = (25.0 * hop_seconds / (256.0 / 44100.0)).ceil() as usize;
        if self.radius == radius {
            return;
        }
        assert!(radius <= MAX_RADIUS && 2 * radius < PITCH_BINS);
        self.radius = radius;
        self.transitions.clear();
        // Interior bins share a normalization, and the two edges are symmetric.
        for edge in 0..=radius {
            let normalization =
                ((radius + 1).pow(2) - (radius - edge) * (radius - edge + 1) / 2) as f32;
            for distance in 0..=radius {
                let local = (radius + 1 - distance) as f32 / normalization;
                self.transitions
                    .push(((1.0 - NEW_NOTE) * local + NEW_NOTE / PITCH_BINS as f32).ln());
            }
        }
    }

    pub fn reset(&mut self) {
        self.score.fill(f32::NEG_INFINITY);
        self.score[BINS..BINS + PITCH_BINS].fill(-(PITCH_BINS as f32).ln());
        self.frames = 0;
    }

    pub fn process(
        &mut self,
        observation: &Observations<BINS>,
        threshold: f32,
        lag: usize,
    ) -> (f32, f32) {
        assert!(lag < HISTORY, "Pitch decoder lookahead exceeds its history");

        let best_voiced = (0..PITCH_BINS)
            .max_by(|&a, &b| self.score[a].total_cmp(&self.score[b]))
            .expect("the tracker has voiced pitch bins");
        let best_unvoiced = (BINS..BINS + PITCH_BINS)
            .max_by(|&a, &b| self.score[a].total_cmp(&self.score[b]))
            .expect("the tracker has unvoiced pitch bins");
        let frame = &mut self.history[self.frames % HISTORY];
        frame.frequency = observation.frequency;
        frame.clarity = observation.clarity;
        frame.threshold = threshold;
        self.next.fill(f32::NEG_INFINITY);

        let stay = (1.0 - VOICE_SWITCH).ln();
        let switch = VOICE_SWITCH.ln();
        let jump = (NEW_NOTE / PITCH_BINS as f32).ln();
        let unvoiced = (1.0 - observation.probability.iter().sum::<f32>()).clamp(0.0, 1.0);

        for i in 0..PITCH_BINS {
            let mut voiced = (best_voiced, self.score[best_voiced] + jump);
            let mut silent = (best_unvoiced, self.score[best_unvoiced] + jump);

            for j in i.saturating_sub(self.radius)..=(i + self.radius).min(PITCH_BINS - 1) {
                let edge = j.min(PITCH_BINS - 1 - j).min(self.radius);
                let transition = self.transitions[edge * (self.radius + 1) + i.abs_diff(j)];
                let voiced_candidate = self.score[j] + transition;
                let silent_candidate = self.score[j + BINS] + transition;

                if voiced_candidate > voiced.1 {
                    voiced = (j, voiced_candidate);
                }

                if silent_candidate > silent.1 {
                    silent = (j + BINS, silent_candidate);
                }
            }

            // same transition kernel for voiced/unvoiced so unvoiced doesn't dominate
            let (parent, score) = if voiced.1 + stay > silent.1 + switch {
                (voiced.0, voiced.1 + stay)
            } else {
                (silent.0, silent.1 + switch)
            };
            self.next[i] = score + observation.probability[i].ln();
            frame.previous[i] = parent as u16;

            let (parent, score) = if silent.1 + stay > voiced.1 + switch {
                (silent.0, silent.1 + stay)
            } else {
                (voiced.0, voiced.1 + switch)
            };
            self.next[i + BINS] = score + unvoiced.ln();
            frame.previous[i + BINS] = parent as u16;
        }

        let mut state = (0..STATES)
            .max_by(|&a, &b| self.next[a].total_cmp(&self.next[b]))
            .expect("the tracker has at least one state");
        let maximum = self.next[state];
        assert!(maximum.is_finite(), "Pitch HMM has no reachable state");

        for (score, next) in self.score.iter_mut().zip(&self.next) {
            *score = next - maximum;
        }

        self.frames += 1;

        if self.frames <= lag {
            return (0.0, 0.0);
        }

        for offset in 0..lag {
            state = self.history[(self.frames - 1 - offset) % HISTORY].previous[state] as usize;
        }

        if state >= BINS {
            return (0.0, 0.0);
        }

        let frame = &self.history[(self.frames - 1 - lag) % HISTORY];
        let clarity = frame.clarity[state];
        let frequency = if clarity >= frame.threshold {
            frame.frequency[state]
        } else {
            0.0
        };
        (frequency, clarity)
    }
}
