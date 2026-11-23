use super::*;

#[derive(Clone, Copy)]
pub(crate) struct Clock {
    tempo: f64,
    beats: Option<f64>,
    seconds: Option<f64>,
    playing: bool,
    offset: u64,
    elapsed: u64,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            tempo: 120.0,
            beats: None,
            seconds: None,
            playing: true,
            offset: 0,
            elapsed: 0,
        }
    }
}

impl Clock {
    pub(crate) fn set(&mut self, transport: &Transport) {
        self.tempo = transport
            .tempo
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
            .unwrap_or(120.0);
        self.beats = transport.pos_beats().filter(|beats| beats.is_finite());
        self.seconds = transport
            .pos_seconds()
            .filter(|seconds| seconds.is_finite());
        self.playing = transport.playing;
        self.offset = 0;
    }

    pub(crate) fn advance(&mut self, samples: usize) {
        self.elapsed += samples as u64;
        self.offset += samples as u64;
    }

    fn position(self, lfo: Lfo, hz: f64, rate: f32) -> Option<f64> {
        let origin = if lfo.sync {
            self.beats
                .map(|beats| beats * 60.0 / self.tempo)
                .or(self.seconds)
        } else {
            self.seconds
        };
        let seconds = if self.playing {
            self.offset as f64 / f64::from(rate)
        } else {
            0.0
        };
        origin.map(|origin| (origin + seconds) * hz)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Live {
    pub frequency: f32,
    pub gain: f32,
    pub q: f32,
    pub mapping_input: f32,
    pub mapping_base: f32,
    pub phase: f32,
    pub lfo_offset: f32,
    pub random: f32,
    pub lfo_base: f32,
}

impl Live {
    pub(crate) fn apply(self, band: &mut BandControl) {
        band.frequency = self.frequency;
        band.gain = self.gain;
        band.q = self.q;
    }
}

#[derive(Default)]
struct State {
    last: Option<u64>,
    cycles: f64,
    phase: f32,
    random: f32,
    offset: f32,
    strength: f32,
    target: Option<Target>,
    live: Option<Live>,
}

fn random(cycle: i64, index: usize) -> f32 {
    let mut bits = (cycle as u64).wrapping_add((index as u64 + 1) * 0x9e37);
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((bits ^ (bits >> 31)) >> 40) as f32 / 8_388_607.5 - 1.0
}

pub(crate) struct Rack {
    states: [State; BAND_COUNT],
    pub clock: Clock,
}

impl Default for Rack {
    fn default() -> Self {
        Self {
            states: std::array::from_fn(|_| State::default()),
            clock: Clock::default(),
        }
    }
}

impl Rack {
    pub(crate) fn live(&self, index: usize) -> Option<Live> {
        self.states[index].live
    }

    pub(crate) fn update(&mut self, index: usize, band: &mut BandControl, rate: f32, linear: bool) {
        let state = &mut self.states[index];
        let lfo = band.lfo;
        let mapping = band.mapping;

        if !band.enabled || (!lfo.enabled && !mapping.enabled) {
            if state.live.is_some() {
                *state = State::default();
            }

            return;
        }

        let clock = self.clock;
        let elapsed = state.last.map_or(0, |last| clock.elapsed - last);
        let dt = elapsed as f32 / rate;
        state.last = Some(clock.elapsed);
        let mut lfo_base = lfo.target.value(*band);

        if lfo.enabled && lfo.target.supported(*band) {
            let max_hz = if linear { Lfo::LINEAR_MAX_HZ } else { 40.0 };
            let hz = lfo.rate(clock.tempo).clamp(0.0, max_hz);
            let hz = if hz.is_finite() { hz } else { 0.0 };
            let position = if lfo.transport {
                clock.position(lfo, hz, rate)
            } else {
                None
            };

            if let Some(position) = position {
                state.cycles = position;
            } else if !lfo.transport || clock.playing {
                state.cycles += hz * f64::from(dt);
            }

            let phase_offset = if lfo.phase.is_finite() {
                f64::from(lfo.phase) / 360.0
            } else {
                0.0
            };
            let phase = state.cycles + phase_offset;
            state.phase = phase.rem_euclid(1.0) as f32;
            if lfo.wave == Wave::Random {
                state.random = random(phase.floor() as i64, index);
            }
            let target = if lfo.bypass || !lfo.depth.is_finite() {
                0.0
            } else {
                lfo.depth * lfo.polar(lfo.wave.sample(state.phase, state.random))
            };

            if state.target != Some(lfo.target) {
                state.offset = 0.0;
                state.target = Some(lfo.target);
            }

            let alpha = 1.0 - (-dt / 0.004).exp();
            state.offset += alpha * (target - state.offset);
            if target == 0.0 && state.offset.abs() < 1e-6 {
                state.offset = 0.0;
            }

            if state.offset != 0.0 {
                lfo_base = lfo.target.value(*band);
                let value = lfo.target.offset(lfo_base, state.offset, rate);
                lfo.target.set(band, value, rate);
            }
        } else {
            state.offset = 0.0;
            state.target = None;
        }

        let mapping_input = mapping.source.value(*band);
        let mapping_base = mapping.target.value(*band);
        if mapping.enabled
            && mapping.source != mapping.target
            && mapping.target.supported(*band)
            && mapping.source.supported(*band)
        {
            let target = if mapping.bypass || !mapping.strength.is_finite() {
                0.0
            } else {
                mapping.strength
            };
            let alpha = 1.0 - (-dt / 0.008).exp();
            state.strength += alpha * (target - state.strength);
            if target == 0.0 && state.strength.abs() < 1e-6 {
                state.strength = 0.0;
            }

            if state.strength != 0.0 {
                let value = Mapping {
                    strength: state.strength,
                    ..mapping
                }
                .output(mapping_input, mapping_base, rate);
                mapping.target.set(band, value, rate);
            }
        } else {
            state.strength = 0.0;
        }

        state.live = Some(Live {
            frequency: band.frequency,
            gain: band.gain,
            q: band.q,
            mapping_input,
            mapping_base,
            phase: state.phase,
            lfo_offset: state.offset,
            random: state.random,
            lfo_base,
        });
    }
}
