use std::path::{Path, PathBuf};

use crate::comparison::{ComparisonPolicy, RgbaView};

use super::codec::{OwnedRgbaImage, encode_png};
use super::error::{ArtifactPaths, SnapshotTestError};
use super::store::{WriteMode, atomic_write, remove_if_present};

pub(crate) fn write_missing_actual(
    artifact_dir: &Path,
    actual: RgbaView<'_>,
) -> Result<PathBuf, SnapshotTestError> {
    let expected_path = artifact_dir.join("expected.png");
    let actual_path = artifact_dir.join("actual.png");
    let diff_path = artifact_dir.join("diff.png");
    let actual_png = encode_png(actual, &actual_path)?;

    remove_if_present(&expected_path)?;
    remove_if_present(&diff_path)?;
    atomic_write(&actual_path, &actual_png, WriteMode::Replace)?;
    Ok(actual_path)
}

pub(crate) fn write_mismatch_artifacts(
    artifact_dir: &Path,
    expected: RgbaView<'_>,
    actual: RgbaView<'_>,
    policy: ComparisonPolicy,
    max_pixels: u64,
) -> Result<ArtifactPaths, SnapshotTestError> {
    let expected_path = artifact_dir.join("expected.png");
    let actual_path = artifact_dir.join("actual.png");
    let diff_path = artifact_dir.join("diff.png");
    let expected_png = encode_png(expected, &expected_path)?;
    let actual_png = encode_png(actual, &actual_path)?;
    let diff = create_diff_image(expected, actual, policy, max_pixels);
    let diff_png = diff
        .as_ref()
        .map(|diff| encode_png(diff.as_view(), &diff_path))
        .transpose()?;

    if diff_png.is_none() {
        remove_if_present(&diff_path)?;
    }
    atomic_write(&expected_path, &expected_png, WriteMode::Replace)?;
    atomic_write(&actual_path, &actual_png, WriteMode::Replace)?;
    if let Some(diff_png) = diff_png {
        atomic_write(&diff_path, &diff_png, WriteMode::Replace)?;
        Ok(ArtifactPaths::new(
            expected_path,
            actual_path,
            Some(diff_path),
        ))
    } else {
        Ok(ArtifactPaths::new(expected_path, actual_path, None))
    }
}

fn create_diff_image(
    expected: RgbaView<'_>,
    actual: RgbaView<'_>,
    policy: ComparisonPolicy,
    max_pixels: u64,
) -> Option<OwnedRgbaImage> {
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let pixels = u64::from(width) * u64::from(height);
    if pixels > max_pixels {
        return None;
    }

    let capacity = usize::try_from(pixels.checked_mul(4)?).ok()?;
    let mut output = Vec::with_capacity(capacity);
    for y in 0..height {
        for x in 0..width {
            let expected_pixel = pixel_at(expected, x, y);
            let actual_pixel = pixel_at(actual, x, y);
            let display_pixel = match (expected_pixel, actual_pixel) {
                (Some(expected), Some(actual))
                    if policy.pixel_exceeds_threshold(expected, actual) =>
                {
                    [255, 0, 255, 255]
                }
                (Some(expected), Some(_)) => {
                    [expected[0] / 3, expected[1] / 3, expected[2] / 3, 255]
                }
                (Some(_), None) => [255, 0, 0, 255],
                (None, Some(_)) => [0, 255, 255, 255],
                (None, None) => unreachable!("diff canvas contains at least one image"),
            };
            output.extend_from_slice(&display_pixel);
        }
    }
    Some(OwnedRgbaImage::new(width, height, output))
}

fn pixel_at(image: RgbaView<'_>, x: u32, y: u32) -> Option<&[u8]> {
    if x >= image.width() || y >= image.height() {
        return None;
    }
    let index = (u64::from(y) * u64::from(image.width()) + u64::from(x)) * 4;
    let index = usize::try_from(index).expect("validated image index fits usize");
    Some(&image.rgba8()[index..index + 4])
}
