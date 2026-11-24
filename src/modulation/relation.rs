use nice_plug::prelude::Enum;

#[derive(Clone, Copy)]
pub(crate) enum Scale {
    Ratio,
    Linear(f32),
}

impl Scale {
    pub(crate) fn encode(self, value: f32) -> f32 {
        match self {
            Self::Ratio => value.log2(),
            Self::Linear(step) => value / step,
        }
    }

    pub(crate) fn decode(self, value: f32) -> f32 {
        match self {
            Self::Ratio => value.exp2(),
            Self::Linear(step) => value * step,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Curve {
    Linear,
    Inverse,
    #[default]
    Power,
    #[name = "Smooth"]
    Ease,
}

impl Curve {
    pub(crate) const ALL: [Self; 4] = [Self::Linear, Self::Inverse, Self::Power, Self::Ease];

    pub(crate) fn output(
        self,
        input: (Scale, f32, f32),
        target: (Scale, f32, (f32, f32)),
        strength: f32,
    ) -> f32 {
        let (source, value, reference) = input;
        let (scale, base, (min, max)) = target;
        let distance = match source {
            Scale::Ratio if value > 0.0 && reference > 0.0 => (value / reference).log2(),
            Scale::Ratio => return base,
            Scale::Linear(step) => (value - reference) / step,
        };

        if strength == 0.0 || !strength.is_finite() || !distance.is_finite() {
            return base;
        }

        let shaped = match self {
            Self::Power | Self::Linear => distance,
            Self::Inverse => -distance,
            Self::Ease => {
                let t = distance.abs().min(1.0);
                distance.signum() * t * t * (3.0 - 2.0 * t)
            }
        };
        let delta = shaped * strength.clamp(-1.0, 1.0);
        let result = match (self, scale) {
            (Self::Linear, Scale::Ratio) => base * (1.0 + delta),
            (_, Scale::Ratio) => base * delta.exp2(),
            (_, Scale::Linear(step)) => base + delta * step,
        };
        result.clamp(min, max)
    }
}
