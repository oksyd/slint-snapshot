use std::env;
use std::error::Error as StdError;
use std::ffi::OsString;
use std::fmt;
use std::str::FromStr;

/// Environment variable read by [`SnapshotMode::from_env`].
pub const SNAPSHOT_MODE_ENV: &str = "SLINT_SNAPSHOT_MODE";

/// How a snapshot check may modify its baseline.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum SnapshotMode {
    /// Only verifies existing baselines and never creates or modifies one.
    #[default]
    Verify,
    /// Creates a missing baseline but never replaces an existing one.
    CreateMissing,
    /// Creates or atomically replaces the baseline with the actual image.
    Accept,
}

impl SnapshotMode {
    /// Reads an explicitly requested mode from [`SNAPSHOT_MODE_ENV`].
    ///
    /// The variable is not read automatically. Accepted values are `verify`,
    /// `create-missing`, and `accept`; an absent variable returns `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns an error when the variable is not Unicode or cannot be parsed.
    pub fn from_env() -> Result<Option<Self>, SnapshotModeEnvError> {
        let Some(value) = env::var_os(SNAPSHOT_MODE_ENV) else {
            return Ok(None);
        };
        let value = value
            .into_string()
            .map_err(|value| SnapshotModeEnvError::NotUnicode { value })?;
        value
            .parse()
            .map(Some)
            .map_err(SnapshotModeEnvError::Invalid)
    }
}

impl FromStr for SnapshotMode {
    type Err = ParseSnapshotModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "verify" => Ok(Self::Verify),
            "create-missing" => Ok(Self::CreateMissing),
            "accept" => Ok(Self::Accept),
            _ => Err(ParseSnapshotModeError {
                value: value.to_owned(),
            }),
        }
    }
}

/// A string was not a documented snapshot mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseSnapshotModeError {
    value: String,
}

impl ParseSnapshotModeError {
    /// Returns the rejected string.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for ParseSnapshotModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid snapshot mode {:?}: expected verify, create-missing, or accept",
            self.value
        )
    }
}

impl StdError for ParseSnapshotModeError {}

/// Reading [`SNAPSHOT_MODE_ENV`] did not produce a valid mode.
#[derive(Debug)]
#[non_exhaustive]
pub enum SnapshotModeEnvError {
    /// The environment variable was not valid Unicode.
    NotUnicode {
        /// Environment variable contents that could not be decoded as Unicode.
        value: OsString,
    },
    /// The Unicode value was not a documented mode.
    Invalid(ParseSnapshotModeError),
}

impl fmt::Display for SnapshotModeEnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUnicode { value } => write!(
                formatter,
                "{SNAPSHOT_MODE_ENV} was not valid Unicode: {}",
                value.to_string_lossy()
            ),
            Self::Invalid(error) => write!(formatter, "invalid {SNAPSHOT_MODE_ENV}: {error}"),
        }
    }
}

impl StdError for SnapshotModeEnvError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Invalid(source) => Some(source),
            Self::NotUnicode { .. } => None,
        }
    }
}
