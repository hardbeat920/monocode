//! Per-controller randomness; the composer has an independent stream.
pub trait Random {
    /// Uniform sample in [0, 1).
    fn next_f64(&mut self) -> f64;
}

#[derive(Clone, Debug)]
pub struct Mulberry32 {
    state: u32,
}
impl Mulberry32 {
    pub const fn new(seed: u32) -> Self {
        Self { state: seed }
    }
}
impl Random for Mulberry32 {
    fn next_f64(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6d2b79f5);
        let mut t = (self.state ^ (self.state >> 15)).wrapping_mul(1 | self.state);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4294967296.0
    }
}
