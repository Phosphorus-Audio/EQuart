use crate::band::{BAND_COUNT, Band, BandControl, COEFF_FRAMES};
use crate::dsp;
use crate::oversample::{DownsamplePath, HIGH, Oversampling, Resampler};
use crate::precision::Sample;
use crate::region::{REGION_COUNT, Region};

pub(crate) struct AudioPath<T: Sample> {
    pub(crate) bands: [Band<T>; BAND_COUNT],
    pub(crate) regions: [Region<T>; REGION_COUNT],
    pub(crate) resampler: Resampler<T>,
    output_gain: T,
    target_gain: T,
    target_db: f32,
}

pub(crate) struct OutputMix {
    pub(crate) wet: f32,
    pub(crate) delta: Option<(usize, f32)>,
}

impl<T: Sample> AudioPath<T> {
    pub(crate) fn new() -> Self {
        Self {
            bands: std::array::from_fn(|_| Band::new()),
            regions: std::array::from_fn(|_| Region::new()),
            resampler: Resampler::default(),
            output_gain: T::one(),
            target_gain: T::one(),
            target_db: 0.0,
        }
    }

    pub(crate) fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }

        for region in &mut self.regions {
            region.reset();
        }

        self.resampler.reset();
    }

    pub(crate) fn transfer_from<S: Sample>(&mut self, source: &AudioPath<S>) {
        for (band, previous) in self.bands.iter_mut().zip(&source.bands) {
            band.transfer_from(previous);
        }

        for (region, previous) in self.regions.iter_mut().zip(&source.regions) {
            region.transfer_from(previous);
        }

        self.resampler.transfer_from(&source.resampler);
        self.output_gain = T::value(source.output_gain.double());
        self.set_output_target(source.target_db);
    }

    fn set_output_target(&mut self, db: f32) {
        if self.target_db != db {
            self.target_db = db;
            self.target_gain = T::value(10.0).powf(T::value(f64::from(db)) / T::value(20.0));
        }
    }

    pub(crate) fn refresh_output_gain(&mut self, db: f32, alpha: f32) {
        self.set_output_target(db);
        self.output_gain += T::value(f64::from(alpha)) * (self.target_gain - self.output_gain);
    }

    pub(crate) fn output_gain(&self) -> f64 {
        self.output_gain.double()
    }

    pub(crate) fn invalidate_bands(&mut self) {
        for band in &mut self.bands {
            band.invalidate();
        }
    }

    pub(crate) fn set_oversampling(&mut self, mode: Oversampling) -> bool {
        let changed = self.resampler.set_mode(mode);
        if changed {
            self.invalidate_bands();
        }

        changed
    }

    pub(crate) fn process(
        &mut self,
        left: &mut [T],
        right: &mut [T],
        controls: &[BandControl; BAND_COUNT],
        output: OutputMix,
        delta_mix: &mut f32,
    ) {
        let len = left.len();
        let mode = self.resampler.mode();

        let mut dry_left = [T::zero(); COEFF_FRAMES];
        let mut dry_right = [T::zero(); COEFF_FRAMES];
        let dry_l = &mut dry_left[..len];
        let dry_r = &mut dry_right[..len];

        if mode != Oversampling::Off || output.wet < 1.0 {
            self.resampler.delayed_dry(left, right, dry_l, dry_r);
        }

        let mut delta_left = [T::zero(); COEFF_FRAMES];
        let mut delta_right = [T::zero(); COEFF_FRAMES];

        if mode == Oversampling::Off {
            let tap = output
                .delta
                .map(|(index, _)| (index, &mut delta_left[..len], &mut delta_right[..len]));
            self.process_filters(left, right, controls, tap);
        } else {
            let mut wide_left = [T::zero(); HIGH];
            let mut wide_right = [T::zero(); HIGH];
            let mut delta_wide_l = [T::zero(); HIGH];
            let mut delta_wide_r = [T::zero(); HIGH];
            let wide = len * mode.factor();

            self.resampler
                .upsample(left, right, &mut wide_left, &mut wide_right);

            let tap = output
                .delta
                .map(|(index, _)| (index, &mut delta_wide_l[..wide], &mut delta_wide_r[..wide]));

            self.process_filters(
                &mut wide_left[..wide],
                &mut wide_right[..wide],
                controls,
                tap,
            );

            self.resampler.downsample(
                DownsamplePath::Main,
                &wide_left[..wide],
                &wide_right[..wide],
                // left,
                left,
                right,
                len,
            );

            if output.delta.is_some() {
                self.resampler.downsample(
                    DownsamplePath::Delta,
                    &delta_wide_l[..wide],
                    &delta_wide_r[..wide],
                    &mut delta_left[..len],
                    &mut delta_right[..len],
                    len,
                );
            }
        }

        if output.wet == 0.0 {
            left.copy_from_slice(dry_l);
            right.copy_from_slice(dry_r);
        } else {
            if self.output_gain != T::one() {
                T::scale(left, self.output_gain);
                T::scale(right, self.output_gain);
            }
            if output.wet < 1.0 {
                dsp::blend(dry_l, left, output.wet);
                dsp::blend(dry_r, right, output.wet);
            }
        }

        if let Some((_, step)) = output.delta {
            for i in 0..len {
                *delta_mix = (*delta_mix + step).clamp(0.0, 1.0);
                if *delta_mix == 1.0 {
                    left[i] = delta_left[i];
                    right[i] = delta_right[i];
                } else {
                    let mix = T::value(f64::from(*delta_mix));
                    left[i] += mix * (delta_left[i] - left[i]);
                    right[i] += mix * (delta_right[i] - right[i]);
                }
            }
        }
    }

    fn process_filters(
        &mut self,
        left: &mut [T],
        right: &mut [T],
        controls: &[BandControl; BAND_COUNT],
        tap: Option<(usize, &mut [T], &mut [T])>,
    ) {
        for (band, control) in self.bands.iter_mut().zip(controls) {
            band.process(left, right, control.channel);
        }

        let remaining = if let Some((index, delta_l, delta_r)) = tap {
            let (before, from_target) = self.regions.split_at_mut(index);
            for region in before {
                region.process(left, right);
            }
            from_target[0].process_with_delta(left, right, delta_l, delta_r);
            &mut from_target[1..]
        } else {
            &mut self.regions[..]
        };

        for region in remaining {
            region.process(left, right);
        }
    }
}
