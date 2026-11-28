use std::ops::{Add, Mul, Sub};

use crate::band::ChannelMode;

pub(crate) fn cascade<T>([ll, lr, rl, rr]: [T; 4], route: ChannelMode, gain: T) -> [T; 4]
where
    T: Copy + From<f32> + Add<Output = T> + Sub<Output = T> + Mul<Output = T>,
{
    match route {
        ChannelMode::Stereo => [ll * gain, lr * gain, rl * gain, rr * gain],
        ChannelMode::Left => [ll * gain, lr * gain, rl, rr],
        ChannelMode::Right => [ll, lr, rl * gain, rr * gain],
        ChannelMode::Mid | ChannelMode::Side => {
            let one = T::from(1.0);
            let half = T::from(0.5);
            let same = (gain + one) * half;
            let cross = if route == ChannelMode::Mid {
                gain - one
            } else {
                one - gain
            } * half;

            [
                same * ll + cross * rl,
                same * lr + cross * rr,
                cross * ll + same * rl,
                cross * lr + same * rr,
            ]
        }
    }
}
