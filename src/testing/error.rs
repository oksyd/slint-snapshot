use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::comparison::{ComparisonPolicy, DiffStats, Difference, InvalidRgbaImage};

/// Encoding or filesystem failure while writing a baseline or failure artifact.
#[derive(Debug)]
#[non_exhaustive]
pub enum SnapshotWriteError {
    /// PNG encoding failed.
    PngEncoding {
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Underlying error.
        source: png::EncodingError,
    },
    /// A filesystem operation failed.
    Io {
        /// Description of the attempted filesystem or platform operation.
        operation: &'static str,
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
}

impl fmt::Display for SnapshotWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PngEncoding { path, source } => write!(
                formatter,
                "failed to encode snapshot PNG {}: {source}",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} snapshot path {}: {source}",
                path.display()
            ),
        }
    }
}

impl StdError for SnapshotWriteError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::PngEncoding { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
        }
    }
}

impl From<SnapshotWriteError> for SnapshotTestError {
    fn from(error: SnapshotWriteError) -> Self {
        Self::Write(error)
    }
}

/// Files written for a snapshot mismatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactPaths {
    expected: PathBuf,
    actual: PathBuf,
    diff: Option<PathBuf>,
}

impl ArtifactPaths {
    pub(crate) fn new(expected: PathBuf, actual: PathBuf, diff: Option<PathBuf>) -> Self {
        Self {
            expected,
            actual,
            diff,
        }
    }

    /// Returns the copy of the accepted baseline used in the comparison.
    #[must_use]
    pub fn expected(&self) -> &Path {
        &self.expected
    }

    /// Returns the rendered image that failed comparison.
    #[must_use]
    pub fn actual(&self) -> &Path {
        &self.actual
    }

    /// Returns the visual diff path, when its canvas fit the pixel budget.
    #[must_use]
    pub fn diff(&self) -> Option<&Path> {
        self.diff.as_deref()
    }
}

/// Structured information about a failed snapshot comparison.
#[derive(Debug)]
pub struct SnapshotMismatch {
    baseline_path: PathBuf,
    policy: ComparisonPolicy,
    difference: Difference,
    artifacts: Result<ArtifactPaths, SnapshotWriteError>,
}

impl SnapshotMismatch {
    pub(crate) fn new(
        baseline_path: PathBuf,
        policy: ComparisonPolicy,
        difference: Difference,
        artifacts: Result<ArtifactPaths, SnapshotWriteError>,
    ) -> Self {
        Self {
            baseline_path,
            policy,
            difference,
            artifacts,
        }
    }

    /// Returns the accepted baseline path.
    #[must_use]
    pub fn baseline_path(&self) -> &Path {
        &self.baseline_path
    }

    /// Returns the policy used for comparison and diff highlighting.
    #[must_use]
    pub fn policy(&self) -> ComparisonPolicy {
        self.policy
    }

    /// Returns the structured comparison difference.
    #[must_use]
    pub fn difference(&self) -> &Difference {
        &self.difference
    }

    /// Returns paths to the failure artifacts, or the error encountered while writing them.
    ///
    /// # Errors
    ///
    /// Returns the artifact encoding or filesystem error without losing comparison statistics.
    pub fn artifacts(&self) -> Result<&ArtifactPaths, &SnapshotWriteError> {
        self.artifacts.as_ref()
    }
}

impl fmt::Display for SnapshotMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.difference {
            Difference::Dimensions(dimensions) => {
                let expected = dimensions.expected();
                let actual = dimensions.actual();
                write!(
                    formatter,
                    "snapshot dimensions differ: expected {}x{}, actual {}x{}",
                    expected.0, expected.1, actual.0, actual.1
                )?;
            }
            Difference::Pixels(stats) => write!(
                formatter,
                "snapshot pixels differ: {}/{} pixels ({:.4}%) exceed the configured channel threshold; maximum channel delta is {}",
                stats.different_pixels(),
                stats.total_pixels(),
                stats.difference_ratio() * 100.0,
                stats.maximum_channel_delta()
            )?,
        }

        write!(formatter, "; baseline: {}", self.baseline_path.display())?;
        let artifacts = match self.artifacts() {
            Ok(artifacts) => artifacts,
            Err(error) => return write!(formatter, "; failed to produce artifacts: {error}"),
        };
        write!(
            formatter,
            "; expected artifact: {}; actual: {}",
            artifacts.expected.display(),
            artifacts.actual.display()
        )?;
        if let Some(diff) = &artifacts.diff {
            write!(formatter, "; diff: {}", diff.display())?;
        }
        Ok(())
    }
}

/// The kind of successful snapshot operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SnapshotOutcomeKind {
    /// An existing baseline satisfied the policy.
    Matched,
    /// `CreateMissing` created a baseline.
    Created,
    /// `Accept` wrote the current image as the baseline.
    Accepted {
        /// Whether the baseline existed when inspected before writing; not an atomic existence report.
        replaced: bool,
    },
}

/// Successful result of checking or updating a snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotOutcome {
    kind: SnapshotOutcomeKind,
    baseline_path: PathBuf,
    stats: Option<DiffStats>,
}

impl SnapshotOutcome {
    pub(crate) fn matched(baseline_path: PathBuf, stats: DiffStats) -> Self {
        Self {
            kind: SnapshotOutcomeKind::Matched,
            baseline_path,
            stats: Some(stats),
        }
    }

    pub(crate) fn created(baseline_path: PathBuf) -> Self {
        Self {
            kind: SnapshotOutcomeKind::Created,
            baseline_path,
            stats: None,
        }
    }

    pub(crate) fn accepted(baseline_path: PathBuf, replaced: bool) -> Self {
        Self {
            kind: SnapshotOutcomeKind::Accepted { replaced },
            baseline_path,
            stats: None,
        }
    }

    /// Returns which successful operation occurred.
    #[must_use]
    pub fn kind(&self) -> SnapshotOutcomeKind {
        self.kind
    }

    /// Returns the baseline path read or written by the operation.
    #[must_use]
    pub fn baseline_path(&self) -> &Path {
        &self.baseline_path
    }

    /// Returns comparison statistics for a matched existing baseline.
    #[must_use]
    pub fn stats(&self) -> Option<&DiffStats> {
        self.stats.as_ref()
    }
}

/// Errors from image validation, PNG processing, file handling, or comparison.
#[derive(Debug)]
#[non_exhaustive]
pub enum SnapshotTestError {
    /// Baseline and artifact roots must not be equal or nested.
    OverlappingRoots {
        /// Resolved baseline root that overlaps the artifact root.
        baseline: PathBuf,
        /// Resolved artifact root that overlaps the baseline root.
        artifacts: PathBuf,
    },
    /// The actual image did not contain valid tightly packed RGBA8 data.
    InvalidActualImage(InvalidRgbaImage),
    /// A decoded baseline did not contain a representable RGBA8 image.
    InvalidBaselineImage {
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Underlying error.
        source: InvalidRgbaImage,
    },
    /// The actual image exceeded the configured pixel limit.
    ActualPixelLimitExceeded {
        /// Requested width in physical pixels.
        width: u32,
        /// Requested height in physical pixels.
        height: u32,
        /// Total physical pixel count (width multiplied by height).
        pixels: u64,
        /// Configured maximum physical pixel count; must be non-zero.
        max_pixels: u64,
    },
    /// A zero decoded-pixel limit was requested.
    InvalidPixelLimit {
        /// Configured maximum physical pixel count; must be non-zero.
        max_pixels: u64,
    },
    /// The baseline is missing in `Verify` mode.
    MissingBaseline {
        /// Path at which the baseline was expected.
        baseline_path: PathBuf,
        /// Actual image path on success, or the artifact generation failure.
        actual_artifact: Result<PathBuf, SnapshotWriteError>,
    },
    /// A decoded baseline exceeded the configured pixel limit.
    BaselinePixelLimitExceeded {
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Requested width in physical pixels.
        width: u32,
        /// Requested height in physical pixels.
        height: u32,
        /// Total physical pixel count (width multiplied by height).
        pixels: u64,
        /// Configured maximum physical pixel count; must be non-zero.
        max_pixels: u64,
    },
    /// The decoded PNG format could not be normalized to RGBA8.
    UnsupportedPngFormat {
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Color representation returned by the PNG decoder.
        color_type: png::ColorType,
        /// Channel bit depth returned by the PNG decoder.
        bit_depth: png::BitDepth,
    },
    /// Decoding a baseline PNG failed.
    PngDecoding {
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Underlying error.
        source: png::DecodingError,
    },
    /// Encoding or writing the accepted baseline failed.
    Write(SnapshotWriteError),
    /// Reading or inspecting a snapshot path failed.
    Io {
        /// Description of the attempted filesystem or platform operation.
        operation: &'static str,
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// The actual image did not satisfy the baseline comparison.
    Mismatch(Box<SnapshotMismatch>),
}

impl SnapshotTestError {
    /// Returns the structured mismatch, if comparison was the failure cause.
    #[must_use]
    pub fn mismatch(&self) -> Option<&SnapshotMismatch> {
        match self {
            Self::Mismatch(mismatch) => Some(mismatch),
            _ => None,
        }
    }
}

impl fmt::Display for SnapshotTestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverlappingRoots {
                baseline,
                artifacts,
            } => write!(
                formatter,
                "baseline root {} and artifact root {} overlap",
                baseline.display(),
                artifacts.display()
            ),
            Self::InvalidActualImage(error) => write!(formatter, "invalid actual image: {error}"),
            Self::InvalidBaselineImage { path, source } => write!(
                formatter,
                "baseline {} did not decode to a valid RGBA8 image: {source}",
                path.display()
            ),
            Self::ActualPixelLimitExceeded {
                width,
                height,
                pixels,
                max_pixels,
            } => write!(
                formatter,
                "actual snapshot is {width}x{height} ({pixels} pixels), exceeding the configured limit of {max_pixels}"
            ),
            Self::InvalidPixelLimit { max_pixels } => write!(
                formatter,
                "snapshot decoded-pixel limit must be non-zero, received {max_pixels}"
            ),
            Self::MissingBaseline {
                baseline_path,
                actual_artifact,
            } => {
                write!(
                    formatter,
                    "snapshot baseline is missing: {}; use CreateMissing or Accept to create it",
                    baseline_path.display()
                )?;
                match actual_artifact {
                    Ok(path) => write!(formatter, "; actual image: {}", path.display()),
                    Err(error) => write!(formatter, "; failed to produce actual image: {error}"),
                }
            }
            Self::BaselinePixelLimitExceeded {
                path,
                width,
                height,
                pixels,
                max_pixels,
            } => write!(
                formatter,
                "baseline {} is {width}x{height} ({pixels} pixels), exceeding the configured limit of {max_pixels}",
                path.display()
            ),
            Self::UnsupportedPngFormat {
                path,
                color_type,
                bit_depth,
            } => write!(
                formatter,
                "baseline {} decoded to unsupported PNG format {color_type:?}/{bit_depth:?}",
                path.display()
            ),
            Self::PngDecoding { path, source } => {
                write!(
                    formatter,
                    "failed to decode baseline PNG {}: {source}",
                    path.display()
                )
            }
            Self::Write(error) => error.fmt(formatter),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} snapshot path {}: {source}",
                path.display()
            ),
            Self::Mismatch(mismatch) => mismatch.fmt(formatter),
        }
    }
}

impl StdError for SnapshotTestError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InvalidActualImage(source) | Self::InvalidBaselineImage { source, .. } => {
                Some(source)
            }
            Self::PngDecoding { source, .. } => Some(source),
            Self::Write(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::Mismatch(mismatch) => mismatch
                .artifacts()
                .err()
                .map(|error| error as &dyn StdError),
            Self::MissingBaseline {
                actual_artifact, ..
            } => actual_artifact
                .as_ref()
                .err()
                .map(|error| error as &dyn StdError),
            Self::OverlappingRoots { .. }
            | Self::ActualPixelLimitExceeded { .. }
            | Self::InvalidPixelLimit { .. }
            | Self::BaselinePixelLimitExceeded { .. }
            | Self::UnsupportedPngFormat { .. } => None,
        }
    }
}
