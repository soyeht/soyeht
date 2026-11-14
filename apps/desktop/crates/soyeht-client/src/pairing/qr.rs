//! The QR code of a pairing window: the modules of the code for a text, with no drawing in it, so
//! that a window of any system draws the same square. The encoder is the `qrcode` crate (ADR 0149).

use qrcode::{Color, QrCode};

/// The width of the light border a QR code needs around it to be read, in modules (the QR
/// specification's).
pub const QUIET_ZONE: usize = 4;

/// The dark and light modules of a QR code, row by row, without its quiet zone.
#[derive(Clone, PartialEq, Eq)]
pub struct Matrix {
    side: usize,
    dark: Vec<bool>,
}

/// The modules are the window's code, which is the secret of the link: nothing prints them.
impl std::fmt::Debug for Matrix {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Matrix")
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}

impl Matrix {
    /// The code of `text`, or `None` when the text is too long for any QR code.
    pub fn encode(text: &str) -> Option<Self> {
        let Ok(code) = QrCode::new(text.as_bytes()) else {
            return None;
        };
        Some(Self {
            side: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|module| module == Color::Dark)
                .collect(),
        })
    }

    /// How many modules a side has.
    pub fn side(&self) -> usize {
        self.side
    }

    /// Whether the module at column `x` of row `y` is dark. Outside the square is light, which is
    /// what the quiet zone around it is.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.side && y < self.side && self.dark[y * self.side + x]
    }

    /// The dark modules, as `(column, row)`: what a window paints.
    pub fn dark_modules(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..self.side)
            .flat_map(move |y| (0..self.side).map(move |x| (x, y)))
            .filter(|&(x, y)| self.is_dark(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::pairing_link;

    fn link() -> String {
        pairing_link::qr(
            "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b",
            "100.64.0.7:7777",
            &"A".repeat(43),
            &"B".repeat(43),
        )
    }

    /// Three corners of a QR code hold a finder pattern: a dark ring, a light ring and a dark
    /// square of three by three. The fourth holds none, so the code was not mirrored or turned.
    #[test]
    fn the_finder_patterns_are_at_three_corners_and_not_the_fourth() {
        let matrix = Matrix::encode(&link()).unwrap();
        let far = matrix.side() - 7;
        for (left, top) in [(0, 0), (far, 0), (0, far)] {
            assert!(matrix.is_dark(left, top), "outer ring at {left},{top}");
            assert!(matrix.is_dark(left + 3, top + 3), "centre at {left},{top}");
            assert!(
                !matrix.is_dark(left + 1, top + 1),
                "light ring at {left},{top}"
            );
        }
        assert!(
            !matrix.is_dark(far + 1, far + 1) || !matrix.is_dark(far + 3, far + 3),
            "the fourth corner holds no finder pattern"
        );
    }

    #[test]
    fn the_dark_modules_are_the_ones_the_square_says_are_dark() {
        let matrix = Matrix::encode(&link()).unwrap();
        assert!((21..=177).contains(&matrix.side()));
        let listed: Vec<_> = matrix.dark_modules().collect();
        assert!(listed.iter().all(|&(x, y)| matrix.is_dark(x, y)));
        let counted = matrix.dark.iter().filter(|dark| **dark).count();
        assert_eq!(listed.len(), counted);
        assert!(!matrix.is_dark(matrix.side(), 0) && !matrix.is_dark(0, matrix.side()));
    }

    #[test]
    fn a_text_no_code_can_hold_has_no_matrix() {
        assert!(Matrix::encode(&"x".repeat(4000)).is_none());
    }
}
