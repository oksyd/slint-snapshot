use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::Builder as TempFileBuilder;

use super::{SnapshotName, SnapshotTestError};

/// Filesystem roots used to store accepted baselines and failure artifacts.
///
/// The roots are trusted caller configuration. [`SnapshotName`] validation
/// prevents lexical traversal but does not turn symlinked roots into a
/// filesystem sandbox.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotStore {
    baseline_dir: PathBuf,
    artifact_dir: PathBuf,
}

impl SnapshotStore {
    /// Creates a store with distinct baseline and artifact roots.
    #[must_use]
    pub fn new(baseline_dir: impl Into<PathBuf>, artifact_dir: impl Into<PathBuf>) -> Self {
        Self {
            baseline_dir: baseline_dir.into(),
            artifact_dir: artifact_dir.into(),
        }
    }

    /// Returns the directory containing accepted baseline PNGs.
    #[must_use]
    pub fn baseline_dir(&self) -> &Path {
        &self.baseline_dir
    }

    /// Returns the directory containing failure artifacts.
    #[must_use]
    pub fn artifact_dir(&self) -> &Path {
        &self.artifact_dir
    }

    pub(crate) fn resolve(&self, name: &SnapshotName) -> SnapshotPaths {
        let relative_name = name.as_path();
        let mut baseline_relative = relative_name.to_path_buf();
        let mut file_name = baseline_relative
            .file_name()
            .expect("validated snapshot name has a file name")
            .to_os_string();
        file_name.push(".png");
        baseline_relative.set_file_name(file_name);

        SnapshotPaths {
            baseline: self.baseline_dir.join(baseline_relative),
            artifact_dir: self.artifact_dir.join(relative_name),
        }
    }
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::new("tests/snapshots", "target/slint-snapshots")
    }
}

#[derive(Debug)]
pub(crate) struct SnapshotPaths {
    pub(crate) baseline: PathBuf,
    pub(crate) artifact_dir: PathBuf,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum WriteMode {
    Replace,
    CreateNew,
}

pub(crate) fn atomic_write(
    path: &Path,
    bytes: &[u8],
    mode: WriteMode,
) -> Result<(), SnapshotTestError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| SnapshotTestError::Io {
        operation: "create the parent directory for",
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".slint-snapshot-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|source| SnapshotTestError::Io {
            operation: "create a temporary file for",
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file_mut().sync_all())
        .map_err(|source| SnapshotTestError::Io {
            operation: "write a temporary file for",
            path: path.to_path_buf(),
            source,
        })?;

    let (operation, persist_result) = match mode {
        WriteMode::Replace => ("atomically replace", temporary.persist(path).map(|_| ())),
        WriteMode::CreateNew => (
            "atomically create",
            temporary.persist_noclobber(path).map(|_| ()),
        ),
    };
    persist_result.map_err(|error| SnapshotTestError::Io {
        operation,
        path: path.to_path_buf(),
        source: error.error,
    })
}

pub(crate) fn remove_if_present(path: &Path) -> Result<(), SnapshotTestError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SnapshotTestError::Io {
            operation: "remove stale",
            path: path.to_path_buf(),
            source,
        }),
    }
}
