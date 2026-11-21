use crate::oversample::Oversampling;
use nice_plug::prelude::Enum;

#[derive(Clone, Copy, Debug, Default, Enum, Eq, PartialEq)]
pub(crate) enum ProcessingMode {
    #[default]
    #[id = "zero_latency"]
    #[name = "Zero Latency"]
    ZeroLatency,
    #[id = "natural_phase"]
    #[name = "Natural Phase"]
    NaturalPhase,
    #[id = "linear_phase"]
    #[name = "Linear Phase"]
    LinearPhase,
}

impl ProcessingMode {
    pub(crate) const ALL: [Self; 3] = [Self::ZeroLatency, Self::NaturalPhase, Self::LinearPhase];

    pub(crate) fn oversampling(self, quality: Oversampling) -> Oversampling {
        match (self, quality) {
            (Self::NaturalPhase, Oversampling::X2) => Oversampling::X2,
            (Self::NaturalPhase, _) => Oversampling::X4,
            _ => Oversampling::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Enum, Eq, PartialEq)]
pub enum LinearQuality {
    #[id = "short"]
    #[name = "Short"]
    Short,
    #[default]
    #[id = "standard"]
    #[name = "Standard"]
    Standard,
    #[id = "high"]
    #[name = "High"]
    High,
    #[id = "extended"]
    #[name = "Extended"]
    Extended,
}

impl LinearQuality {
    pub(crate) const ALL: [Self; 4] = [Self::Short, Self::Standard, Self::High, Self::Extended];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    pub(crate) const MAX_CENTER: usize = 2 * Self::Extended.center();

    pub(crate) fn center_at(self, rate: f32) -> usize {
        self.center() * if rate >= 64_000.0 { 2 } else { 1 }
    }

    pub(crate) const fn center(self) -> usize {
        match self {
            Self::Short => 512,
            Self::Standard => 2048,
            Self::High => 4096,
            Self::Extended => 16384,
        }
    }
}
