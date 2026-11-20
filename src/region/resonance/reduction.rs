use super::{Candidate, PEAKS, RegionControl, ResonanceControl};
use crate::band::{BandControl, ChannelMode};
use crate::filter::CutSlope;

/// One filter per selected peak, with a second set for their release tails.
/// With every slot occupied, new peaks wait for a release or a matching slot.
pub(crate) const REDUCTIONS: usize = PEAKS * 2;
const RELEASE_FLOOR_DB: f32 = 0.001;

#[derive(Clone, Copy, Default)]
pub(crate) struct Reduction {
    pub(crate) hz: f32,
    pub(crate) attenuation_db: f32,
    pub(crate) channel: ChannelMode,
}

impl Reduction {
    pub(crate) fn band(self, region: RegionControl) -> BandControl {
        let res = region.resonance;
        let cut = self.attenuation_db.min(res.depth);
        let [low, high] = region.bounds();
        BandControl {
            enabled: region.enabled
                && !region.bypass
                && res.enabled
                && cut > 0.0
                && (low..=high).contains(&self.hz),
            frequency: self.hz,
            gain: -cut,
            q: res.q,
            slope: CutSlope::Db12,
            channel: self.channel,
            ..BandControl::default()
        }
    }
}

pub(super) fn update(
    reductions: &mut [Reduction; REDUCTIONS],
    ranked: [Candidate; PEAKS],
    control: ResonanceControl,
    elapsed_seconds: f32,
) {
    let attack = -(-elapsed_seconds / (0.001 * control.attack_ms.max(0.5))).exp_m1();
    let release = -(-elapsed_seconds / (0.001 * control.release_ms.max(1.0))).exp_m1();
    let ratio = control.ratio * 0.01;
    let mut pending = ranked.map(|peak| {
        let desired = ((peak.prominence_db - control.threshold) * ratio).clamp(0.0, control.depth);
        (peak, desired)
    });
    let mut matched = [false; REDUCTIONS];

    for (peak, desired) in pending.iter_mut().filter(|(_, desired)| *desired > 0.0) {
        let nearest = reductions
            .iter()
            .zip(matched)
            .enumerate()
            .filter(|(_, (cut, used))| !used && cut.hz > 0.0 && cut.channel == peak.channel)
            .map(|(index, (cut, _))| (index, (cut.hz - peak.hz).abs(), cut.hz))
            .filter(|&(_, distance, center)| distance <= center / (2.0 * control.q))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, _, _)) = nearest {
            let attenuation = &mut reductions[index].attenuation_db;
            let alpha = if *desired > *attenuation {
                attack
            } else {
                release
            };
            *attenuation += alpha * (*desired - *attenuation);
            matched[index] = true;
            *desired = 0.0;
        }
    }

    for (cut, used) in reductions.iter_mut().zip(matched) {
        if !used && cut.hz > 0.0 {
            cut.attenuation_db *= 1.0 - release;
            if cut.attenuation_db <= RELEASE_FLOOR_DB {
                *cut = Reduction::default();
            }
        }
    }

    let free = reductions.iter_mut().filter(|cut| cut.hz == 0.0);
    for (slot, (peak, desired)) in
        free.zip(pending.into_iter().filter(|(_, desired)| *desired > 0.0))
    {
        *slot = Reduction {
            hz: peak.hz,
            attenuation_db: attack * desired,
            channel: peak.channel,
        };
    }
}
