//! A picture of premultiplied RGBA bytes, and the two things the rasteriser does with one:
//! read it at a point (bilinear, the edge repeated, as a texture with clamp-to-edge does) and lay
//! a texel over it. Integer arithmetic only, on four channels at once (two in each half of a
//! 32-bit word), so the same bytes come out on every system and a pixel costs a few operations.
//!
//! A texel is one `u32` with red in the lowest byte and alpha in the highest, however the
//! machine orders its bytes: the channels are never read from memory by position.

/// A weight is 0 to `ONE`.
const ONE: u32 = 256;
/// The red and blue channels of a texel, each in the low byte of a 16-bit lane.
const LANES_RB: u32 = 0x00FF_00FF;
/// A texel's green and alpha channels, each in the high byte of a 16-bit lane.
const LANES_GA: u32 = 0xFF00_FF00;

/// One half (128) in each 16-bit lane: what rounds a product of two 8-bit numbers.
const HALF: u32 = 0x0080_0080;

/// `a * b / 255`, rounded, for two values in 0 to 255.
pub(super) fn mul255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

pub(super) fn pack(texel: [u8; 4]) -> u32 {
    u32::from_le_bytes(texel)
}

#[cfg(test)]
pub(super) fn unpack(texel: u32) -> [u8; 4] {
    texel.to_le_bytes()
}

/// Every channel of `texel` times `weight / 256`, rounded, for a weight of 0 to 256.
fn scale(texel: u32, weight: u32) -> u32 {
    let rb = (((texel & LANES_RB) * weight + HALF) >> 8) & LANES_RB;
    let ga = (((texel >> 8) & LANES_RB) * weight + HALF) & LANES_GA;
    rb | ga
}

/// `a` and `b` channel by channel, each at most 255, clamped.
fn add(a: u32, b: u32) -> u32 {
    let lanes = |x: u32, y: u32| {
        let sum = x + y;
        // A lane that reached 256 or more overflowed its byte: it becomes 255.
        let over = (sum >> 8) & 0x0001_0001;
        (sum | (over * 255)) & LANES_RB
    };
    lanes(a & LANES_RB, b & LANES_RB) | (lanes((a >> 8) & LANES_RB, (b >> 8) & LANES_RB) << 8)
}

/// The texel `t / 256` of the way from `a` to `b`, for `t` of 0 to 255, rounded: a texel mixed
/// with itself is itself.
fn lerp(a: u32, b: u32, t: u32) -> u32 {
    let (from, to) = (ONE - t, t);
    let rb = ((a & LANES_RB) * from + (b & LANES_RB) * to + HALF) >> 8;
    let ga = ((a >> 8) & LANES_RB) * from + ((b >> 8) & LANES_RB) * to + HALF;
    (rb & LANES_RB) | (ga & LANES_GA)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    width: u32,
    height: u32,
    texels: Vec<u32>,
}

impl Pixmap {
    /// A picture from straight (not premultiplied) RGBA bytes, row by row. `None` when the bytes
    /// are not `width * height` texels or the picture is empty.
    pub fn from_straight_rgba(width: u32, height: u32, bytes: &[u8]) -> Option<Self> {
        let count = (width as usize).checked_mul(height as usize)?;
        if count == 0 || bytes.len() != count.checked_mul(4)? {
            return None;
        }
        let texels = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                let a = u32::from(c[3]);
                let premultiplied = |v: u8| mul255(u32::from(v), a) as u8;
                pack([
                    premultiplied(c[0]),
                    premultiplied(c[1]),
                    premultiplied(c[2]),
                    c[3],
                ])
            })
            .collect();
        Some(Self {
            width,
            height,
            texels,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The rows of the picture, top first.
    pub(super) fn rows(&self) -> impl Iterator<Item = &[u32]> {
        self.texels.chunks_exact(self.width as usize)
    }

    /// The texel at `index` (row by row), as red, green, blue, alpha.
    #[cfg(test)]
    pub(super) fn at(&self, index: usize) -> [u8; 4] {
        unpack(self.texels[index])
    }

    /// Makes this picture the same as `other` without a new allocation when the sizes agree.
    pub(super) fn copy_from(&mut self, other: &Pixmap) {
        if self.texels.len() == other.texels.len() {
            self.texels.copy_from_slice(&other.texels);
        } else {
            self.texels.clone_from(&other.texels);
        }
        self.width = other.width;
        self.height = other.height;
    }

    /// Lays `texel`, taken `coverage` of 256, over the texel at (`x`, `y`). A place outside the
    /// picture is left alone.
    pub(super) fn lay_over(&mut self, x: u32, y: u32, texel: u32, coverage: u32) {
        if x >= self.width || y >= self.height || coverage == 0 {
            return;
        }
        let slot = y as usize * self.width as usize + x as usize;
        let coverage = coverage.min(ONE);
        let opaque = texel >> 24 == 255;
        self.texels[slot] = match (opaque, coverage) {
            (true, ONE) => texel,
            // An opaque texel taken in part is a mix of it and what is below.
            (true, _) => lerp(self.texels[slot], texel, coverage),
            _ => {
                let over = scale(texel, coverage);
                add(over, scale(self.texels[slot], ONE - (over >> 24)))
            }
        };
    }

    /// The picture at (`x`, `y`), in texel units where the centre of the first texel is (0.5, 0.5):
    /// the four nearest texels blended. A point outside reads the nearest edge texel, and one that
    /// is not a number reads the first.
    pub(super) fn sample(&self, x: f32, y: f32) -> u32 {
        // `as i32` saturates and turns a NaN into 0: no panic.
        let fixed = |v: f32| ((v - 0.5) * ONE as f32).round() as i32;
        self.sample_fixed(fixed(x), fixed(y))
    }

    /// The same, at a point in 256ths of a texel with the centre of the first texel at 0.
    pub(super) fn sample_fixed(&self, x: i32, y: i32) -> u32 {
        let (last_x, last_y) = (self.width as i32 - 1, self.height as i32 - 1);
        let x = x.clamp(0, last_x * ONE as i32);
        let y = y.clamp(0, last_y * ONE as i32);
        let (x0, y0) = ((x >> 8) as usize, (y >> 8) as usize);
        let (fx, fy) = ((x & 255) as u32, (y & 255) as u32);
        let x1 = (x0 + 1).min(last_x as usize);
        let y1 = (y0 + 1).min(last_y as usize);
        let (top, bottom) = (y0 * self.width as usize, y1 * self.width as usize);
        let above = lerp(self.texels[top + x0], self.texels[top + x1], fx);
        let below = lerp(self.texels[bottom + x0], self.texels[bottom + x1], fx);
        lerp(above, below, fy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_by_one() -> Pixmap {
        Pixmap::from_straight_rgba(2, 1, &[0, 0, 0, 255, 200, 100, 50, 255]).unwrap()
    }

    #[test]
    fn a_read_at_the_centre_of_a_texel_is_that_texel() {
        let picture = two_by_one();
        assert_eq!(unpack(picture.sample(0.5, 0.5)), [0, 0, 0, 255]);
        assert_eq!(unpack(picture.sample(1.5, 0.5)), [200, 100, 50, 255]);
    }

    #[test]
    fn a_read_between_two_texels_is_their_mean() {
        assert_eq!(unpack(two_by_one().sample(1.0, 0.5)), [100, 50, 25, 255]);
    }

    #[test]
    fn a_read_outside_the_picture_repeats_the_edge() {
        let picture = two_by_one();
        assert_eq!(unpack(picture.sample(-40.0, 9.0)), [0, 0, 0, 255]);
        assert_eq!(unpack(picture.sample(90.0, -3.0)), [200, 100, 50, 255]);
        assert_eq!(unpack(picture.sample(f32::NAN, f32::NAN)), [0, 0, 0, 255]);
        assert_eq!(
            unpack(picture.sample(f32::INFINITY, 0.0)),
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn straight_bytes_are_premultiplied() {
        let picture = Pixmap::from_straight_rgba(1, 1, &[200, 100, 50, 128]).unwrap();
        assert_eq!(picture.at(0), [100, 50, 25, 128]);
    }

    #[test]
    fn bytes_that_are_not_a_picture_are_refused() {
        assert!(Pixmap::from_straight_rgba(2, 2, &[0; 12]).is_none());
        assert!(Pixmap::from_straight_rgba(0, 2, &[]).is_none());
    }

    #[test]
    fn laying_a_texel_over_one_mixes_by_its_alpha() {
        let mut picture = two_by_one();
        picture.lay_over(0, 0, pack([100, 0, 0, 128]), 256);
        assert_eq!(picture.at(0), [100, 0, 0, 255]);
        picture.lay_over(1, 0, pack([0, 0, 0, 0]), 256);
        assert_eq!(picture.at(1), [200, 100, 50, 255]);
        // Half covered: half of the texel over the one below.
        picture.lay_over(1, 0, pack([0, 0, 255, 255]), 128);
        assert_eq!(picture.at(1), [100, 50, 153, 255]);
        // Outside the picture: nothing happens.
        picture.lay_over(2, 0, pack([9, 9, 9, 255]), 256);
        picture.lay_over(0, 1, pack([9, 9, 9, 255]), 256);
    }

    #[test]
    fn a_channel_never_wraps_into_the_next() {
        // Lanes of a texel that is nearly opaque over a bright one add past 255.
        let mut picture = Pixmap::from_straight_rgba(1, 1, &[255, 255, 255, 255]).unwrap();
        picture.lay_over(0, 0, pack([250, 250, 250, 254]), 256);
        assert_eq!(picture.at(0)[3], 255);
        assert!(picture.at(0)[..3].iter().all(|c| *c >= 250));
    }
}
