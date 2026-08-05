//! PNG baseline management and assertions for visual regression tests.
//!
//! This module is available with the `testing` Cargo feature. [`SnapshotName`]
//! accepts a deliberately small portable identifier grammar and prevents path
//! traversal. Baseline and artifact roots remain caller-controlled trusted
//! directories; name validation is not a sandbox around symlinked trees.
//!
//! Give concurrently running tests unique snapshot names. Baselines and each
//! artifact are written atomically, but a group of artifacts is not a
//! transaction. Artifacts from an earlier failure are not removed after a
//! successful check; paths in the current structured result are authoritative.
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
};
pub use mode::{ParseSnapshotModeError, SNAPSHOT_MODE_ENV, SnapshotMode, SnapshotModeEnvError};
pub use name::{InvalidSnapshotName, SnapshotName};
pub use store::SnapshotStore;

#[cfg(test)]
mod tests;
