use std::error::Error as StdError;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

/// Validated, portable logical name for one snapshot.
///
/// Names consist of `/`-separated components. Each component starts with an
/// ASCII letter, digit, or underscore and then contains only ASCII letters,
/// digits, `.`, `_`, or `-`. Components may not end in `.`, and Windows device
/// names and an explicit `.png` suffix are rejected. The suffix is added by
/// the store.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SnapshotName {
    value: String,
}

impl SnapshotName {
    /// Parses and validates a logical snapshot name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty, contains an empty or reserved
    /// component, or falls outside the documented portable grammar.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidSnapshotName> {
        value.as_ref().parse()
    }

    /// Returns the validated logical name without a `.png` suffix.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub(crate) fn as_path(&self) -> &Path {
        Path::new(&self.value)
    }
}

impl fmt::Display for SnapshotName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

impl FromStr for SnapshotName {
    type Err = InvalidSnapshotName;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_name(value)?;
        Ok(Self {
            value: value.to_owned(),
        })
    }
}

impl TryFrom<String> for SnapshotName {
    type Error = InvalidSnapshotName;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_name(&value)?;
        Ok(Self { value })
    }
}

/// A logical snapshot name does not satisfy the portable identifier grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidSnapshotName {
    value: String,
    reason: &'static str,
}

impl InvalidSnapshotName {
    /// Returns the rejected input.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns a stable human-readable explanation.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for InvalidSnapshotName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid snapshot name {:?}: {}",
            self.value, self.reason
        )
    }
}

impl StdError for InvalidSnapshotName {}

fn validate_name(value: &str) -> Result<(), InvalidSnapshotName> {
    if value.is_empty() {
        return Err(invalid(value, "the name must not be empty"));
    }
    for component in value.split('/') {
        validate_component(value, component)?;
    }
    if value
        .rsplit('/')
        .next()
        .is_some_and(|file_name| file_name.to_ascii_lowercase().ends_with(".png"))
    {
        return Err(invalid(
            value,
            "the logical name must not include the .png extension",
        ));
    }
    Ok(())
}

fn validate_component(full_name: &str, component: &str) -> Result<(), InvalidSnapshotName> {
    if component.is_empty() {
        return Err(invalid(full_name, "path components must not be empty"));
    }
    let mut characters = component.chars();
    let first = characters
        .next()
        .expect("an empty component was rejected above");
    if !first.is_ascii_alphanumeric() && first != '_' {
        return Err(invalid(
            full_name,
            "each component must start with an ASCII letter, digit, or underscore",
        ));
    }
    if !characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        return Err(invalid(
            full_name,
            "components may contain only ASCII letters, digits, '.', '_', and '-'",
        ));
    }
    if component.ends_with('.') {
        return Err(invalid(full_name, "components must not end in '.'"));
    }
    if is_windows_device_name(component) {
        return Err(invalid(
            full_name,
            "components must not use a reserved Windows device name",
        ));
    }
    Ok(())
}

fn invalid(value: &str, reason: &'static str) -> InvalidSnapshotName {
    InvalidSnapshotName {
        value: value.to_owned(),
        reason,
    }
}

fn is_windows_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}
