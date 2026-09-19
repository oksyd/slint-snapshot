//! Rendered RGBA frames and PNG output.

use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::png_codec;

/// A rendered Slint frame in eight-bit RGBA channel order.
///
/// The frame is the reusable boundary between Slint and image tools, snapshot
/// tests, encoders, and file exporters.
#[derive(Clone, Debug)]
pub struct RenderedFrame {
    pixels: SharedPixelBuffer<Rgba8Pixel>,
}

impl RenderedFrame {
    pub(crate) fn from_pixels(pixels: SharedPixelBuffer<Rgba8Pixel>) -> Self {
        Self { pixels }
    }

    /// Returns the physical width and height in pixels.
    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width(), self.height())
    }

    /// Returns the physical width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.pixels.width()
    }

    /// Returns the physical height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.pixels.height()
    }

    /// Returns tightly packed RGBA bytes in row-major order.
    #[must_use]
    pub fn rgba8(&self) -> &[u8] {
        self.pixels.as_bytes()
    }

    /// Encodes this frame as an in-memory RGBA8 PNG.
    ///
    /// # Errors
    ///
    /// Returns an error when the PNG encoder rejects the frame.
    pub fn encode_png(&self) -> Result<Vec<u8>, FrameEncodeError> {
        png_codec::encode_rgba8(self.width(), self.height(), self.rgba8())
            .map_err(FrameEncodeError::new)
    }

    /// Encodes this frame and writes it to a PNG file.
    ///
    /// Missing parent directories are created after the frame has been encoded.
    /// This general export helper does not promise an atomic file replacement;
    /// baseline updates in the testing layer use a separate atomic store.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-PNG output path, PNG encoding failure,
    /// directory creation failure, or file write failure.
    pub fn write_png(&self, output: &Path) -> Result<(), FrameWriteError> {
        let is_png = output
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
        if !is_png {
            return Err(FrameWriteError::InvalidOutputExtension {
                path: output.to_path_buf(),
            });
        }

        let bytes = self.encode_png()?;
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| FrameWriteError::Io {
                operation: "create the parent directory for",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(output, bytes).map_err(|source| FrameWriteError::Io {
            operation: "write",
            path: output.to_path_buf(),
            source,
        })
    }
}

/// An RGBA frame could not be encoded as PNG.
#[derive(Debug)]
pub struct FrameEncodeError {
    source: png::EncodingError,
}

impl FrameEncodeError {
    fn new(source: png::EncodingError) -> Self {
        Self { source }
    }
}

impl fmt::Display for FrameEncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "failed to encode the rendered frame as PNG: {}",
            self.source
        )
    }
}

impl StdError for FrameEncodeError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}

/// A rendered frame could not be written to a PNG path.
#[derive(Debug)]
#[non_exhaustive]
pub enum FrameWriteError {
    /// Frame output paths must end in `.png`.
    InvalidOutputExtension {
        /// Path associated with the failed operation.
        path: PathBuf,
    },
    /// Encoding the frame failed before any file was written.
    Encoding(FrameEncodeError),
    /// Creating the parent directory or writing the file failed.
    Io {
        /// Description of the attempted filesystem or platform operation.
        operation: &'static str,
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
}

impl fmt::Display for FrameWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOutputExtension { path } => write!(
                formatter,
                "frame output must use the .png extension: {}",
                path.display()
            ),
            Self::Encoding(source) => source.fmt(formatter),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} frame path {}: {source}",
                path.display()
            ),
        }
    }
}

impl StdError for FrameWriteError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Encoding(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::InvalidOutputExtension { .. } => None,
        }
    }
}

impl From<FrameEncodeError> for FrameWriteError {
    fn from(source: FrameEncodeError) -> Self {
        Self::Encoding(source)
    }
}
