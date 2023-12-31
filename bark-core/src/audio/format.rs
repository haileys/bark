const SCALE: f32 = 32768.0; // i16::MIN.abs() as f32

/// Converts f32 sample to i16 at scale 32768.
/// Will clip if sample is < -1.0, or >= 1.0. This function's counterpart
/// in the other direction never produces 1.0 however.
pub fn f32_to_i16(sample: f32) -> i16 {
    (sample * SCALE) as i16
}

/// Converts i16 sample to f32 at scale 32768.
/// This means i16::MAX becomes not quite 1.0, but the output sample
/// remains strictly in the range [-1.0, 1.0] to prevent clipping on
/// the return.
pub fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / SCALE
}
