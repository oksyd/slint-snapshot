//! PNG baseline management and assertions for visual regression tests.
//!
//! This module is available with the `testing` Cargo feature. [`SnapshotName`]
//! accepts a deliberately small portable identifier grammar and prevents path
//! traversal. Baseline and artifact roots remain caller-controlled trusted
//! directories; name validation is not a sandbox around symlinked trees.
//! Checks reject equal or nested roots (including existing symlink aliases)
//! before writing files. Callers must not concurrently change those directories
//! or symlinks during a check.
//!
//! Give concurrently running tests unique snapshot names. Baselines and each
//! artifact are written atomically, but a group of artifacts is not a
//! transaction. Artifacts from an earlier failure are not removed after a
//! successful check; paths in the current structured result are authoritative.
//! A mismatch retains its comparison statistics even if artifact generation
//! fails; [`SnapshotMismatch::artifacts`] returns that error separately.
//! [`SnapshotTestError::MissingBaseline`] also retains an [`SnapshotWriteError`]
//! if writing the actual image fails, rather than replacing the missing-baseline error.
//!
//! ```no_run
//! use slint_snapshot::comparison::{ComparisonPolicy, RgbaView};
//! use slint_snapshot::testing::{
//!     SnapshotAssertion, SnapshotMode, SnapshotName, SnapshotStore,
//! };
//!
//! # fn check() -> Result<(), Box<dyn std::error::Error>> {
//! let pixels = [20, 40, 60, 255];
//! let actual = RgbaView::new(1, 1, &pixels)?;
//! let name = SnapshotName::new("settings/default.zh-CN.light")?;
//! let store = SnapshotStore::new("tests/snapshots", "target/slint-snapshots");
//! SnapshotAssertion::new(name, actual)
//!     .store(store)
//!     .policy(ComparisonPolicy::Exact)
//!     .mode(SnapshotMode::Verify)
//!     .check()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Create, verify, inspect a failure, and accept
//!
//! This executable example uses a temporary store. Real tests should keep
//! reviewed baselines in version control and put failure artifacts elsewhere.
//! [`SnapshotMode::Verify`] is the default; environment variables only affect
//! the mode if the caller explicitly uses [`SnapshotMode::from_env`].
//!
//! ```
//! use slint_snapshot::comparison::RgbaView;
//! use slint_snapshot::testing::{
//!     SnapshotAssertion, SnapshotMode, SnapshotStore, SnapshotTestError,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let directory = tempfile::tempdir()?;
//! let store = SnapshotStore::new(
//!     directory.path().join("baselines"),
//!     directory.path().join("artifacts"),
//! );
//! let original_pixels = [20, 40, 60, 255];
//! let original = RgbaView::new(1, 1, &original_pixels)?;
//! let check = |image| {
//!     SnapshotAssertion::try_new("settings/default", image)
//!         .map(|assertion| assertion.store(store.clone()))
//! };
//!
//! // Verification never creates a baseline, but attempts to save the actual image.
//! let missing = check(original)?.check().unwrap_err();
//! if let SnapshotTestError::MissingBaseline { actual_artifact, .. } = missing {
//!     match actual_artifact {
//!         Ok(path) => println!("actual image: {}", path.display()),
//!         Err(error) => eprintln!("could not save actual image: {error}"),
//!     }
//! }
//!
//! // CreateMissing writes only when no baseline exists; otherwise it compares.
//! check(original)?.mode(SnapshotMode::CreateMissing).check()?;
//! check(original)?.check()?;
//!
//! let changed_pixels = [30, 40, 60, 255];
//! let changed = RgbaView::new(1, 1, &changed_pixels)?;
//! let error = check(changed)?.check().unwrap_err();
//! let mismatch = error.mismatch().expect("pixel mismatch");
//! println!("difference: {:?}", mismatch.difference());
//! match mismatch.artifacts() {
//!     Ok(paths) => {
//!         println!("expected: {}", paths.expected().display());
//!         println!("actual: {}", paths.actual().display());
//!         if let Some(diff) = paths.diff() {
//!             println!("diff: {}", diff.display());
//!         }
//!     }
//!     Err(error) => eprintln!("comparison failed; artifacts unavailable: {error}"),
//! }
//!
//! // Accept is an explicit update. Use it only after reviewing the change.
//! check(changed)?.mode(SnapshotMode::Accept).check()?;
//! check(changed)?.check()?;
//! # Ok(())
//! # }
//! ```

mod artifacts;
mod assertion;
mod codec;
mod error;
mod mode;
mod name;
mod store;

pub use assertion::SnapshotAssertion;
pub use error::{
    ArtifactPaths, SnapshotMismatch, SnapshotOutcome, SnapshotOutcomeKind, SnapshotTestError,
    SnapshotWriteError,
};
pub use mode::{ParseSnapshotModeError, SNAPSHOT_MODE_ENV, SnapshotMode, SnapshotModeEnvError};
pub use name::{InvalidSnapshotName, SnapshotName};
pub use store::SnapshotStore;

#[cfg(test)]
mod tests;
