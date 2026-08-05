//! Pure comparison for tightly packed, row-major RGBA8 images.
//!
//! ```
//! use slint_snapshot::comparison::{
//!     ComparisonOutcome, ComparisonPolicy, RgbaView, compare,
//! };
//!
//! let expected = [10, 20, 30, 255];
//! let actual = [10, 20, 30, 255];
//! let expected = RgbaView::new(1, 1, &expected)?;
//! let actual = RgbaView::new(1, 1, &actual)?;
//! assert!(matches!(
//!     compare(expected, actual, ComparisonPolicy::Exact),
//!     ComparisonOutcome::Match(_)
//! ));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::error::Error as StdError;
use std::fmt;

use crate::RenderedFrame;

/// A source of tightly packed, row-major RGBA8 pixels.
///
/// This trait is intentionally open for downstream image types. Implementors
/// report their dimensions and bytes; [`RgbaView::from_source`] validates that
/// the representation is consistent before comparison or file access.
pub trait RgbaSource {
    /// Returns the physical width and height in pixels.
    fn dimensions(&self) -> (u32, u32);

    /// Returns tightly packed RGBA8 bytes in row-major order.
    fn rgba8(&self) -> &[u8];
}

impl RgbaSource for RenderedFrame {
    fn dimensions(&self) -> (u32, u32) {
        self.dimensions()
    }

    fn rgba8(&self) -> &[u8] {
        self.rgba8()
    }
}

impl<T> RgbaSource for &T
where
    T: RgbaSource + ?Sized,
{
    fn dimensions(&self) -> (u32, u32) {
        T::dimensions(self)
    }

    fn rgba8(&self) -> &[u8] {
        T::rgba8(self)
    }
}

/// A validated borrowed view of tightly packed RGBA8 pixels.
#[derive(Clone, Copy, Debug)]
pub struct RgbaView<'a> {
    width: u32,
    height: u32,
    pixels: &'a [u8],
}

impl<'a> RgbaView<'a> {
    /// Creates an RGBA8 view after validating its dimensions and length.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty image, overflowing dimensions, or a byte
    /// slice whose length is not exactly `width * height * 4`.
    pub fn new(width: u32, height: u32, pixels: &'a [u8]) -> Result<Self, InvalidRgbaImage> {
        let expected_bytes = checked_rgba_len(width, height)?;
        if pixels.len() != expected_bytes {
            return Err(InvalidRgbaImage::InvalidBufferLength {
                width,
                height,
                expected_bytes,
                actual_bytes: pixels.len(),
            });
        }

        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Creates a validated view over an [`RgbaSource`].
    ///
    /// # Errors
    ///
    /// Returns an error when the source reports invalid dimensions or an
    /// inconsistent byte length.
    pub fn from_source<T>(source: &'a T) -> Result<Self, InvalidRgbaImage>
    where
        T: RgbaSource + ?Sized,
    {
        let (width, height) = source.dimensions();
        Self::new(width, height, source.rgba8())
    }

    /// Returns the physical width and height in pixels.
    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the physical width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the physical height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the validated tightly packed RGBA8 bytes.
    #[must_use]
    pub fn rgba8(&self) -> &'a [u8] {
        self.pixels
    }
}

impl RgbaSource for RgbaView<'_> {
    fn dimensions(&self) -> (u32, u32) {
        self.dimensions()
    }

    fn rgba8(&self) -> &[u8] {
        self.rgba8()
    }
}

impl<'a> From<&'a RenderedFrame> for RgbaView<'a> {
    fn from(frame: &'a RenderedFrame) -> Self {
        Self {
            width: frame.width(),
            height: frame.height(),
            pixels: frame.rgba8(),
        }
    }
}

/// Why an RGBA8 view could not be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InvalidRgbaImage {
    /// Width and height must both be non-zero.
    EmptyDimensions { width: u32, height: u32 },
    /// Computing the required RGBA byte length overflowed this platform.
    DimensionsOverflow { width: u32, height: u32 },
    /// The pixel slice is not tightly packed RGBA8 data for the dimensions.
    InvalidBufferLength {
        width: u32,
        height: u32,
        expected_bytes: usize,
        actual_bytes: usize,
    },
}

impl fmt::Display for InvalidRgbaImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDimensions { width, height } => write!(
                formatter,
                "RGBA image dimensions must be non-zero, received {width}x{height}"
            ),
            Self::DimensionsOverflow { width, height } => write!(
                formatter,
                "RGBA image dimensions {width}x{height} cannot be represented"
            ),
            Self::InvalidBufferLength {
                width,
                height,
                expected_bytes,
                actual_bytes,
            } => write!(
                formatter,
                "RGBA image {width}x{height} requires {expected_bytes} bytes, received {actual_bytes}"
            ),
        }
    }
}

impl StdError for InvalidRgbaImage {}

/// Policy used to decide whether two RGBA images match.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ComparisonPolicy {
    /// Dimensions and every RGBA channel must be identical.
    #[default]
    Exact,
    /// Ignores small per-channel changes and permits a bounded number of
    /// pixels whose change exceeds that threshold.
    ///
    /// A pixel is over the threshold when any of its four channel deltas is
    /// strictly greater than `channel_delta_threshold`. The images match when
    /// the number of such pixels is at most `max_pixels_over_threshold`.
    PixelTolerance {
        channel_delta_threshold: u8,
        max_pixels_over_threshold: u64,
    },
}

impl ComparisonPolicy {
    pub(crate) fn channel_delta_threshold(self) -> u8 {
        match self {
            Self::Exact => 0,
            Self::PixelTolerance {
                channel_delta_threshold,
                ..
            } => channel_delta_threshold,
        }
    }

    fn max_pixels_over_threshold(self) -> u64 {
        match self {
            Self::Exact => 0,
            Self::PixelTolerance {
                max_pixels_over_threshold,
                ..
            } => max_pixels_over_threshold,
        }
    }

    pub(crate) fn pixel_exceeds_threshold(self, expected: &[u8], actual: &[u8]) -> bool {
        let threshold = self.channel_delta_threshold();
        expected
            .iter()
            .zip(actual)
            .any(|(&expected, &actual)| expected.abs_diff(actual) > threshold)
    }
}

/// Pixel statistics for two equal-sized images.
///
/// Fields are private so the size, count, and derived ratio cannot become
/// inconsistent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffStats {
    size: (u32, u32),
    different_pixels: u64,
    maximum_channel_delta: u8,
}

impl DiffStats {
    /// Returns the compared physical dimensions.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Returns the total number of compared pixels.
    #[must_use]
    pub fn total_pixels(&self) -> u64 {
        u64::from(self.size.0) * u64::from(self.size.1)
    }

    /// Returns pixels with at least one channel over the configured threshold.
    #[must_use]
    pub fn different_pixels(&self) -> u64 {
        self.different_pixels
    }

    /// Returns the fraction of pixels over the configured threshold.
    #[must_use]
    pub fn difference_ratio(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        {
            self.different_pixels as f64 / self.total_pixels() as f64
        }
    }

    /// Returns the largest absolute delta across every compared RGBA channel.
    #[must_use]
    pub fn maximum_channel_delta(&self) -> u8 {
        self.maximum_channel_delta
    }
}

/// Expected and actual dimensions for a size mismatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DimensionMismatch {
    expected: (u32, u32),
    actual: (u32, u32),
}

impl DimensionMismatch {
    /// Returns the baseline dimensions.
    #[must_use]
    pub fn expected(&self) -> (u32, u32) {
        self.expected
    }

    /// Returns the actual dimensions.
    #[must_use]
    pub fn actual(&self) -> (u32, u32) {
        self.actual
    }
}

/// A comparison failure.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Difference {
    /// Expected and actual dimensions differ.
    Dimensions(DimensionMismatch),
    /// Equal-sized images exceeded the configured pixel budget.
    Pixels(DiffStats),
}

/// Structured result of comparing two RGBA8 images.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ComparisonOutcome {
    /// The images satisfy the configured policy. Tolerated differences remain
    /// visible in the statistics.
    Match(DiffStats),
    /// The images do not satisfy the configured policy.
    Mismatch(Difference),
}

impl ComparisonOutcome {
    /// Returns whether the comparison matched.
    #[must_use]
    pub fn is_match(&self) -> bool {
        matches!(self, Self::Match(_))
    }

    /// Returns statistics when the images matched.
    #[must_use]
    pub fn match_stats(&self) -> Option<&DiffStats> {
        match self {
            Self::Match(stats) => Some(stats),
            Self::Mismatch(_) => None,
        }
    }

    /// Returns the structured difference when the images mismatched.
    #[must_use]
    pub fn difference(&self) -> Option<&Difference> {
        match self {
            Self::Mismatch(difference) => Some(difference),
            Self::Match(_) => None,
        }
    }
}

/// Compares two validated RGBA8 views.
///
/// Dimension mismatches are reported separately. For equal dimensions, all
/// four RGBA channels participate in comparison and statistics.
#[must_use]
pub fn compare(
    expected: RgbaView<'_>,
    actual: RgbaView<'_>,
    policy: ComparisonPolicy,
) -> ComparisonOutcome {
    if expected.dimensions() != actual.dimensions() {
        return ComparisonOutcome::Mismatch(Difference::Dimensions(DimensionMismatch {
            expected: expected.dimensions(),
            actual: actual.dimensions(),
        }));
    }

    let mut different_pixels = 0_u64;
    let mut maximum_channel_delta = 0_u8;
    for (expected_pixel, actual_pixel) in expected
        .rgba8()
        .chunks_exact(4)
        .zip(actual.rgba8().chunks_exact(4))
    {
        if policy.pixel_exceeds_threshold(expected_pixel, actual_pixel) {
            different_pixels += 1;
        }
        for (&expected_channel, &actual_channel) in expected_pixel.iter().zip(actual_pixel) {
            maximum_channel_delta =
                maximum_channel_delta.max(expected_channel.abs_diff(actual_channel));
        }
    }

    let stats = DiffStats {
        size: expected.dimensions(),
        different_pixels,
        maximum_channel_delta,
    };
    if different_pixels <= policy.max_pixels_over_threshold() {
        ComparisonOutcome::Match(stats)
    } else {
        ComparisonOutcome::Mismatch(Difference::Pixels(stats))
    }
}

fn checked_rgba_len(width: u32, height: u32) -> Result<usize, InvalidRgbaImage> {
    if width == 0 || height == 0 {
        return Err(InvalidRgbaImage::EmptyDimensions { width, height });
    }

    u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(InvalidRgbaImage::DimensionsOverflow { width, height })
}

#[cfg(test)]
mod tests {
    use super::{
        ComparisonOutcome, ComparisonPolicy, Difference, InvalidRgbaImage, RgbaView, compare,
    };

    #[test]
    fn validates_rgba_shape() {
        assert!(matches!(
            RgbaView::new(0, 1, &[]),
            Err(InvalidRgbaImage::EmptyDimensions {
                width: 0,
                height: 1
            })
        ));
        assert!(matches!(
            RgbaView::new(2, 1, &[0; 4]),
            Err(InvalidRgbaImage::InvalidBufferLength {
                expected_bytes: 8,
                actual_bytes: 4,
                ..
            })
        ));
    }

    #[test]
    fn exact_comparison_reports_pixel_statistics() {
        let expected_bytes = [10, 20, 30, 255, 40, 50, 60, 255];
        let actual_bytes = [10, 20, 30, 255, 40, 55, 60, 255];
        let expected = RgbaView::new(2, 1, &expected_bytes).expect("expected image");
        let actual = RgbaView::new(2, 1, &actual_bytes).expect("actual image");

        let outcome = compare(expected, actual, ComparisonPolicy::Exact);
        let ComparisonOutcome::Mismatch(Difference::Pixels(stats)) = outcome else {
            panic!("expected a pixel mismatch: {outcome:?}");
        };
        assert_eq!(stats.total_pixels(), 2);
        assert_eq!(stats.different_pixels(), 1);
        assert!((stats.difference_ratio() - 0.5).abs() < f64::EPSILON);
        assert_eq!(stats.maximum_channel_delta(), 5);
    }

    #[test]
    fn tolerance_uses_a_strict_channel_threshold_and_pixel_budget() {
        let expected_bytes = [10, 20, 30, 255, 40, 50, 60, 255];
        let actual_bytes = [12, 18, 30, 255, 40, 53, 60, 255];
        let expected = RgbaView::new(2, 1, &expected_bytes).expect("expected image");
        let actual = RgbaView::new(2, 1, &actual_bytes).expect("actual image");
        let policy = ComparisonPolicy::PixelTolerance {
            channel_delta_threshold: 2,
            max_pixels_over_threshold: 1,
        };

        let ComparisonOutcome::Match(stats) = compare(expected, actual, policy) else {
            panic!("one over-threshold pixel should be tolerated");
        };
        assert_eq!(stats.different_pixels(), 1);
        assert_eq!(stats.maximum_channel_delta(), 3);
    }

    #[test]
    fn reports_dimensions_without_inventing_pixel_statistics() {
        let expected_bytes = [0; 8];
        let actual_bytes = [0; 16];
        let expected = RgbaView::new(2, 1, &expected_bytes).expect("expected image");
        let actual = RgbaView::new(2, 2, &actual_bytes).expect("actual image");

        let ComparisonOutcome::Mismatch(Difference::Dimensions(dimensions)) =
            compare(expected, actual, ComparisonPolicy::Exact)
        else {
            panic!("expected dimension mismatch");
        };
        assert_eq!(dimensions.expected(), (2, 1));
        assert_eq!(dimensions.actual(), (2, 2));
    }
}
