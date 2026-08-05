use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::comparison::{InvalidRgbaImage, RgbaView};
use crate::png_codec;

use super::SnapshotTestError;

pub(crate) struct OwnedRgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl OwnedRgbaImage {
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels,
        }
    }

    pub(crate) fn as_view(&self) -> RgbaView<'_> {
        RgbaView::new(self.width, self.height, &self.pixels)
            .expect("owned RGBA image is validated when constructed")
    }
}

pub(crate) fn encode_png(image: RgbaView<'_>, path: &Path) -> Result<Vec<u8>, SnapshotTestError> {
    png_codec::encode_rgba8(image.width(), image.height(), image.rgba8()).map_err(|source| {
        SnapshotTestError::PngEncoding {
            path: path.to_path_buf(),
            source,
        }
    })
}

pub(crate) fn decode_png(
    path: &Path,
    max_pixels: u64,
) -> Result<OwnedRgbaImage, SnapshotTestError> {
    let file = File::open(path).map_err(|source| SnapshotTestError::Io {
        operation: "open",
        path: path.to_path_buf(),
        source,
    })?;
    let rgba_limit = max_pixels
        .checked_mul(4)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .unwrap_or(usize::MAX);
    let decoder_limit = rgba_limit.saturating_add(1024 * 1024);
    let mut decoder = png::Decoder::new_with_limits(
        BufReader::new(file),
        png::Limits {
            bytes: decoder_limit,
        },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());

    let (width, height) = {
        let info = decoder
            .read_header_info()
            .map_err(|source| SnapshotTestError::PngDecoding {
                path: path.to_path_buf(),
                source,
            })?;
        (info.width, info.height)
    };
    validate_baseline_pixel_limit((width, height), max_pixels, path)?;

    let mut reader = decoder
        .read_info()
        .map_err(|source| SnapshotTestError::PngDecoding {
            path: path.to_path_buf(),
            source,
        })?;
    let output_buffer_size =
        reader
            .output_buffer_size()
            .ok_or_else(|| SnapshotTestError::PngDecoding {
                path: path.to_path_buf(),
                source: png::DecodingError::LimitsExceeded,
            })?;
    let mut output_bytes = vec![0; output_buffer_size];
    let output =
        reader
            .next_frame(&mut output_bytes)
            .map_err(|source| SnapshotTestError::PngDecoding {
                path: path.to_path_buf(),
                source,
            })?;
    let output_bytes = &output_bytes[..output.buffer_size()];
    if output.bit_depth != png::BitDepth::Eight {
        return Err(SnapshotTestError::UnsupportedPngFormat {
            path: path.to_path_buf(),
            color_type: output.color_type,
            bit_depth: output.bit_depth,
        });
    }

    let rgba = normalize_decoded_png(path, &output, output_bytes)?;
    RgbaView::new(width, height, &rgba).map_err(|source| {
        SnapshotTestError::InvalidBaselineImage {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(OwnedRgbaImage::new(width, height, rgba))
}

fn validate_baseline_pixel_limit(
    (width, height): (u32, u32),
    max_pixels: u64,
    path: &Path,
) -> Result<(), SnapshotTestError> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels > max_pixels {
        return Err(SnapshotTestError::BaselinePixelLimitExceeded {
            path: path.to_path_buf(),
            width,
            height,
            pixels,
            max_pixels,
        });
    }
    Ok(())
}

fn normalize_decoded_png(
    path: &Path,
    output: &png::OutputInfo,
    output_bytes: &[u8],
) -> Result<Vec<u8>, SnapshotTestError> {
    let capacity = u64::from(output.width)
        .checked_mul(u64::from(output.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| SnapshotTestError::InvalidBaselineImage {
            path: path.to_path_buf(),
            source: InvalidRgbaImage::DimensionsOverflow {
                width: output.width,
                height: output.height,
            },
        })?;
    let mut rgba = Vec::with_capacity(capacity);
    match output.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(output_bytes),
        png::ColorType::Rgb => {
            for pixel in output_bytes.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in output_bytes.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &value in output_bytes {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        png::ColorType::Indexed => {
            return Err(SnapshotTestError::UnsupportedPngFormat {
                path: path.to_path_buf(),
                color_type: output.color_type,
                bit_depth: output.bit_depth,
            });
        }
    }
    Ok(rgba)
}
