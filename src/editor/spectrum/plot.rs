use super::*;

/// top padding for the vertical scale so labels don't touch the header
pub(super) const SCALE_TOP_INSET: f32 = 22.0;

/// EQ and spectrum share the x axis, but use separate y scales
pub(crate) struct Plot {
    pub(crate) rect: Rect,
    pub(crate) max_hz: f32,
    /// EQ scale range (-range..+range dB)
    pub(crate) range: f32,
    /// spectrum range (0..-range dB)
    pub(crate) spectrum_range: f32,
    pub(super) spectrum_top: f32,
    pub(super) spectrum_bottom: f32,
}

impl Plot {
    pub(crate) fn new(outer: Rect, rate: f32) -> Self {
        Self::with_ranges(
            outer,
            rate,
            DEFAULT_AXIS_RANGE as f32,
            DEFAULT_SPECTRUM_RANGE as f32,
        )
    }

    pub(crate) fn with_ranges(outer: Rect, rate: f32, range: f32, spectrum_range: f32) -> Self {
        let rect = Rect::from_min_max(
            outer.min + Vec2::new(48.0, Layout::HEADER + SCALE_TOP_INSET),
            // leave room for axis labels and keyboard
            outer.max - Vec2::new(30.0, KEYS_BAND + 34.0),
        );
        Self {
            max_hz: if rate > 0.0 {
                max_frequency(rate)
            } else {
                20_000.0
            },
            range,
            spectrum_range,
            spectrum_top: outer.min.y + Layout::HEADER + SCALE_TOP_INSET,
            spectrum_bottom: rect.bottom(),
            rect,
        }
    }

    pub(crate) fn x(&self, hz: f32) -> f32 {
        self.x_unchecked(hz.max(MIN_HZ))
    }

    /// same as x(), just without clamping to MIN_HZ (mostly for piano keys)
    pub(super) fn x_unchecked(&self, hz: f32) -> f32 {
        self.rect.left() + (hz / MIN_HZ).ln() / (self.max_hz / MIN_HZ).ln() * self.rect.width()
    }

    pub(crate) fn hz(&self, x: f32) -> f32 {
        let t = ((x - self.rect.left()) / self.rect.width()).clamp(0.0, 1.0);
        MIN_HZ * (self.max_hz / MIN_HZ).powf(t)
    }

    /// hz() without clamping, so curves can keep going past the plot edges
    pub(crate) fn hz_extended(&self, x: f32) -> f32 {
        let t = (x - self.rect.left()) / self.rect.width();
        MIN_HZ * (self.max_hz / MIN_HZ).powf(t)
    }

    /// clamped y for node markers
    pub(crate) fn y(&self, db: f32) -> f32 {
        self.rect.center().y
            - db.clamp(-self.range, self.range) / (2.0 * self.range) * self.rect.height()
    }

    /// unclamped y for curves (painter clip handles out-of-range)
    pub(crate) fn y_free(&self, db: f32) -> f32 {
        self.rect.center().y - db / (2.0 * self.range) * self.rect.height()
    }

    pub(crate) fn db(&self, y: f32) -> f32 {
        self.db_free(y).clamp(-self.range, self.range)
    }

    /// unclamped db, e.g. for cut handle Q
    pub(crate) fn db_free(&self, y: f32) -> f32 {
        (self.rect.center().y - y) / self.rect.height() * 2.0 * self.range
    }

    /// map dBFS to y on the spectrum scale
    pub(crate) fn spectrum_y(&self, db: f32) -> f32 {
        self.spectrum_top - db / self.spectrum_range * (self.spectrum_bottom - self.spectrum_top)
    }

    /// inverse of spectrum_y, unclamped for level meter
    pub(crate) fn spectrum_db(&self, y: f32) -> f32 {
        (self.spectrum_top - y) / (self.spectrum_bottom - self.spectrum_top) * self.spectrum_range
    }

    pub(crate) fn spectrum_floor(&self) -> f32 {
        self.spectrum_range
    }
}
