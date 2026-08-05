use crate::comparison::{ComparisonOutcome, ComparisonPolicy, RgbaSource, RgbaView, compare};
use crate::runtime::DEFAULT_MAX_PIXELS;

use super::artifacts::{write_mismatch_artifacts, write_missing_actual};
use super::codec::{decode_png, encode_png};
use super::error::{SnapshotMismatch, SnapshotOutcome, SnapshotTestError};
use super::mode::SnapshotMode;
use super::name::{InvalidSnapshotName, SnapshotName};
use super::store::{SnapshotStore, WriteMode, atomic_write};

/// Configures and performs one file-backed snapshot assertion.
///
/// The default mode is [`SnapshotMode::Verify`], the default comparison is
/// [`ComparisonPolicy::Exact`], and the default store writes baselines below
/// `tests/snapshots` and artifacts below `target/slint-snapshots`.
#[derive(Debug)]
pub struct SnapshotAssertion<I> {
    name: SnapshotName,
    actual: I,
    store: SnapshotStore,
    policy: ComparisonPolicy,
    mode: SnapshotMode,
    max_pixels: u64,
}

impl<I> SnapshotAssertion<I>
where
    I: RgbaSource,
{
    /// Creates an assertion from a previously validated logical name.
    #[must_use]
    pub fn new(name: SnapshotName, actual: I) -> Self {
        Self {
            name,
            actual,
            store: SnapshotStore::default(),
            policy: ComparisonPolicy::default(),
            mode: SnapshotMode::default(),
            max_pixels: DEFAULT_MAX_PIXELS,
        }
    }

    /// Validates a string name and creates an assertion.
    ///
    /// # Errors
    ///
    /// Returns an error when the name does not satisfy [`SnapshotName`]'s
    /// portable grammar.
    pub fn try_new(name: impl AsRef<str>, actual: I) -> Result<Self, InvalidSnapshotName> {
        Ok(Self::new(SnapshotName::new(name)?, actual))
    }

    /// Selects the filesystem store for baselines and failure artifacts.
    #[must_use]
    pub fn store(mut self, store: SnapshotStore) -> Self {
        self.store = store;
        self
    }

    /// Sets the RGBA comparison policy.
    #[must_use]
    pub fn policy(mut self, policy: ComparisonPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Sets how this invocation may create or update a baseline.
    #[must_use]
    pub fn mode(mut self, mode: SnapshotMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the maximum number of pixels accepted from actual images,
    /// decoded baselines, and generated diff canvases.
    #[must_use]
    pub fn max_pixels(mut self, max_pixels: u64) -> Self {
        self.max_pixels = max_pixels;
        self
    }

    /// Checks, creates, or accepts the snapshot according to the configured
    /// mode.
    ///
    /// Mismatches return [`SnapshotTestError::Mismatch`] with structured
    /// statistics and artifact paths. Failure artifacts are retained until a
    /// later failure for the same snapshot replaces them.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid image data, an invalid pixel budget,
    /// missing baselines in verification mode, PNG or filesystem failures,
    /// and comparison mismatches.
    pub fn check(self) -> Result<SnapshotOutcome, SnapshotTestError> {
        if self.max_pixels == 0 {
            return Err(SnapshotTestError::InvalidPixelLimit {
                max_pixels: self.max_pixels,
            });
        }

        let actual =
            RgbaView::from_source(&self.actual).map_err(SnapshotTestError::InvalidActualImage)?;
        validate_actual_pixel_limit(actual.dimensions(), self.max_pixels)?;
        let paths = self.store.resolve(&self.name);
        let baseline_exists =
            paths
                .baseline
                .try_exists()
                .map_err(|source| SnapshotTestError::Io {
                    operation: "inspect",
                    path: paths.baseline.clone(),
                    source,
                })?;

        match self.mode {
            SnapshotMode::Accept => {
                let encoded = encode_png(actual, &paths.baseline)?;
                atomic_write(&paths.baseline, &encoded, WriteMode::Replace)?;
                return Ok(SnapshotOutcome::accepted(paths.baseline, baseline_exists));
            }
            SnapshotMode::CreateMissing if !baseline_exists => {
                let encoded = encode_png(actual, &paths.baseline)?;
                atomic_write(&paths.baseline, &encoded, WriteMode::CreateNew)?;
                return Ok(SnapshotOutcome::created(paths.baseline));
            }
            SnapshotMode::Verify if !baseline_exists => {
                let actual_path = write_missing_actual(&paths.artifact_dir, actual)?;
                return Err(SnapshotTestError::MissingBaseline {
                    baseline_path: paths.baseline,
                    actual_path,
                });
            }
            SnapshotMode::Verify | SnapshotMode::CreateMissing => {}
        }

        let expected = decode_png(&paths.baseline, self.max_pixels)?;
        let expected = expected.as_view();
        match compare(expected, actual, self.policy) {
            ComparisonOutcome::Match(stats) => Ok(SnapshotOutcome::matched(paths.baseline, stats)),
            ComparisonOutcome::Mismatch(difference) => {
                let artifacts = write_mismatch_artifacts(
                    &paths.artifact_dir,
                    expected,
                    actual,
                    self.policy,
                    self.max_pixels,
                )?;
                Err(SnapshotTestError::Mismatch(Box::new(
                    SnapshotMismatch::new(paths.baseline, self.policy, difference, artifacts),
                )))
            }
        }
    }

    /// Performs [`Self::check`] and panics with its detailed error on failure.
    ///
    /// # Panics
    ///
    /// Panics when the snapshot is missing, mismatched, invalid, or cannot be
    /// read or written.
    pub fn assert_match(self) -> SnapshotOutcome {
        self.check()
            .unwrap_or_else(|error| panic!("snapshot assertion failed: {error}"))
    }
}

fn validate_actual_pixel_limit(
    (width, height): (u32, u32),
    max_pixels: u64,
) -> Result<(), SnapshotTestError> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels > max_pixels {
        return Err(SnapshotTestError::ActualPixelLimitExceeded {
            width,
            height,
            pixels,
            max_pixels,
        });
    }
    Ok(())
}
