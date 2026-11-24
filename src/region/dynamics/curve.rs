use super::{Control, FLOOR};
use serde::{Deserialize, Serialize};

pub(crate) const CEILING: f32 = 12.0;
pub(crate) const MAX_POINTS: usize = 12;
const MIN_GAP: f32 = 0.25;

fn input_tick(input: f32) -> i32 {
    (input * 100.0).round() as i32
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub(crate) enum Segment {
    Linear,
    #[default]
    Bend,
    Smooth,
    SCurve,
    Hold,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Point {
    pub input: f32,
    pub output: f32,
    pub bend: f32,
    pub segment: Segment,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Curve {
    points: [Point; MAX_POINTS],
    len: u8,
    enabled: bool,
}

impl Curve {
    pub(crate) fn from_standard(control: Control) -> Self {
        let half = (control.knee * 0.5).max(MIN_GAP);
        let slope = match control.mode {
            super::Mode::Compressor => 1.0 - control.ratio.recip(),
            super::Mode::Expander => control.ratio - 1.0,
        };
        let cap = control.range / slope.max(f32::EPSILON);
        let cap = match control.mode {
            super::Mode::Compressor => control.threshold + cap,
            super::Mode::Expander => control.threshold - cap,
        };
        let point = |input| Point {
            input,
            output: input + control.reduction(input),
            segment: Segment::Linear,
            ..Point::default()
        };
        let mut curve = Self {
            len: 2,
            enabled: true,
            ..Self::default()
        };
        curve.points[..2].copy_from_slice(&[point(FLOOR), point(CEILING)]);

        if slope > 0.0 && control.range > 0.0 {
            for db in [
                cap,
                control.threshold - half,
                control.threshold,
                control.threshold + half,
            ] {
                if (FLOOR + MIN_GAP..CEILING - MIN_GAP).contains(&db) {
                    curve.insert(point(db));
                }
            }
        }

        curve
    }

    pub(crate) fn validated(mut self) -> Self {
        if self.len == 0 {
            return Self::default();
        }

        let Some(points) = self.points.get(..usize::from(self.len)) else {
            return Self::default();
        };
        if points.len() < 2
            || points[0].input < FLOOR
            || points[points.len() - 1].input > CEILING
            || !points
                .windows(2)
                .all(|pair| input_tick(pair[0].input) < input_tick(pair[1].input))
            || !points
                .iter()
                .all(|p| p.input.is_finite() && p.output.is_finite() && p.bend.is_finite())
        {
            return Self::default();
        }

        for point in &mut self.points[..usize::from(self.len)] {
            point.bend = point.bend.clamp(-1.0, 1.0);
        }

        self
    }

    pub(crate) fn points(&self) -> &[Point] {
        &self.points[..usize::from(self.len)]
    }

    pub(crate) fn is_custom(&self) -> bool {
        self.enabled && self.len != 0
    }

    pub(crate) fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub(crate) fn insert(&mut self, mut point: Point) -> bool {
        point.input = point.input.clamp(FLOOR, CEILING);
        let index = self.points().partition_point(|p| p.input < point.input);
        let len = usize::from(self.len);
        if len == MAX_POINTS
            || (index > 0 && point.input - self.points[index - 1].input < MIN_GAP)
            || (index < len && self.points[index].input - point.input < MIN_GAP)
        {
            return false;
        }

        self.insert_at(index, point);
        true
    }

    pub(crate) fn remove(&mut self, index: usize) {
        if self.len > 2 {
            self.remove_at(index);
        }
    }

    fn insert_at(&mut self, index: usize, point: Point) {
        self.points
            .copy_within(index..usize::from(self.len), index + 1);
        self.points[index] = point;
        self.len += 1;
    }

    fn remove_at(&mut self, index: usize) {
        let len = usize::from(self.len);
        self.points.copy_within(index + 1..len, index);
        self.len -= 1;
        self.points[usize::from(self.len)] = Point::default();
    }

    pub(crate) fn set(&mut self, index: usize, mut point: Point) {
        self.remove_at(index);
        let target = input_tick(point.input.clamp(FLOOR, CEILING));
        let input = (0..=i32::from(self.len))
            .flat_map(|step| [target + step, target - step])
            .find(|&input| {
                (input_tick(FLOOR)..=input_tick(CEILING)).contains(&input)
                    && self.points().iter().all(|p| input_tick(p.input) != input)
            })
            .expect("the curve has fewer points than available input coordinates");
        point.input = input as f32 / 100.0;
        point.output = point.output.clamp(FLOOR, CEILING);
        point.bend = point.bend.clamp(-1.0, 1.0);
        let index = self.points().partition_point(|p| p.input < point.input);
        self.insert_at(index, point);
    }

    pub(crate) fn output(&self, input: f32) -> Option<f32> {
        if !self.is_custom() {
            return None;
        }

        let points = self.points();
        let first = points.first()?;
        let last = &points[points.len() - 1];
        if input <= first.input {
            return Some(input + (first.output - first.input));
        }
        if input >= last.input {
            return Some(input + (last.output - last.input));
        }

        let end = points.partition_point(|p| p.input <= input);
        let [a, b] = [points[end - 1], points[end]];
        let t = (input - a.input) / (b.input - a.input);
        let bend = t + a.bend * t * (1.0 - t);
        let blend = match a.segment {
            Segment::Linear => t,
            Segment::Bend => bend,
            Segment::Smooth => bend * bend * (3.0 - 2.0 * bend),
            Segment::SCurve => {
                let ease = bend * bend * (3.0 - 2.0 * bend);
                ease * ease * (3.0 - 2.0 * ease)
            }
            Segment::Hold => 0.0,
        };
        Some(a.output + blend * (b.output - a.output))
    }

    pub(super) fn encode(self) -> [u64; MAX_POINTS] {
        std::array::from_fn(|index| {
            let Some(p) = self.points().get(index) else {
                return 0;
            };
            let pack = |n: f32| u64::from((n * 100.0).round() as i16 as u16);
            (1 << 63)
                | (u64::from(self.enabled) << 62)
                | (u64::from(p.segment as u8) << 48)
                | (pack(p.bend) << 32)
                | (pack(p.output) << 16)
                | pack(p.input)
        })
    }

    pub(super) fn decode(words: [u64; MAX_POINTS]) -> Self {
        let mut curve = Self::default();
        curve.enabled = words[0] & (1 << 62) != 0;
        for word in words.into_iter().take_while(|&word| word != 0) {
            let value = |shift: u32| f32::from((word >> shift) as u16 as i16) / 100.0;
            let segment = match (word >> 48) & 7 {
                0 => Segment::Linear,
                2 => Segment::Smooth,
                3 => Segment::SCurve,
                4 => Segment::Hold,
                _ => Segment::Bend,
            };
            curve.points[usize::from(curve.len)] = Point {
                input: value(0),
                output: value(16),
                bend: value(32),
                segment,
            };
            curve.len += 1;
        }
        curve
    }
}
