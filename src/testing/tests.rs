use std::fs;

use tempfile::tempdir;

use super::{
    SnapshotAssertion, SnapshotMode, SnapshotName, SnapshotOutcomeKind, SnapshotStore,
    SnapshotTestError,
};
use crate::comparison::{ComparisonPolicy, Difference, RgbaSource};

#[derive(Debug)]
struct TestImage {
    size: (u32, u32),
    pixels: Vec<u8>,
}

impl TestImage {
    fn solid(size: (u32, u32), color: [u8; 4]) -> Self {
        let mut pixels = Vec::new();
        for _ in 0..u64::from(size.0) * u64::from(size.1) {
            pixels.extend_from_slice(&color);
        }
        Self { size, pixels }
    }
}

impl RgbaSource for TestImage {
    fn dimensions(&self) -> (u32, u32) {
        self.size
    }

    fn rgba8(&self) -> &[u8] {
        &self.pixels
    }
}

fn assertion(name: &str, image: TestImage, store: &SnapshotStore) -> SnapshotAssertion<TestImage> {
    SnapshotAssertion::try_new(name, image)
        .expect("valid test snapshot name")
        .store(store.clone())
}

#[test]
fn baseline_lifecycle_is_explicit_and_preserves_failed_baselines() {
    let workspace = tempdir().expect("temporary workspace");
    let store = SnapshotStore::new(
        workspace.path().join("snapshots"),
        workspace.path().join("artifacts"),
    );
    let baseline = store.baseline_dir().join("settings/default.png");
    let artifact_dir = store.artifact_dir().join("settings/default");

    let missing = assertion(
        "settings/default",
        TestImage::solid((2, 2), [10, 20, 30, 255]),
        &store,
    )
    .check()
    .expect_err("verify must not create a baseline");
    assert!(matches!(missing, SnapshotTestError::MissingBaseline { .. }));
    assert!(!baseline.exists());
    assert!(artifact_dir.join("actual.png").exists());

    let created = assertion(
        "settings/default",
        TestImage::solid((2, 2), [10, 20, 30, 255]),
        &store,
    )
    .mode(SnapshotMode::CreateMissing)
    .check()
    .expect("create missing baseline");
    assert_eq!(created.kind(), SnapshotOutcomeKind::Created);
    let accepted_bytes = fs::read(&baseline).expect("accepted baseline bytes");

    let matched = assertion(
        "settings/default",
        TestImage::solid((2, 2), [10, 20, 30, 255]),
        &store,
    )
    .check()
    .expect("matching baseline");
    assert_eq!(matched.kind(), SnapshotOutcomeKind::Matched);
    assert_eq!(
        matched
            .stats()
            .expect("match statistics")
            .different_pixels(),
        0
    );
    assert!(artifact_dir.join("actual.png").exists());

    let mismatch = assertion(
        "settings/default",
        TestImage::solid((2, 2), [40, 50, 60, 255]),
        &store,
    )
    .check()
    .expect_err("changed image must fail verification");
    let report = mismatch.mismatch().expect("structured mismatch");
    assert!(matches!(report.difference(), Difference::Pixels(_)));
    assert!(report.artifacts().expected().exists());
    assert!(report.artifacts().actual().exists());
    assert!(report.artifacts().diff().expect("diff path").exists());
    assert_eq!(
        fs::read(&baseline).expect("unchanged baseline"),
        accepted_bytes
    );

    let accepted = assertion(
        "settings/default",
        TestImage::solid((2, 2), [40, 50, 60, 255]),
        &store,
    )
    .mode(SnapshotMode::Accept)
    .check()
    .expect("accept changed baseline");
    assert_eq!(
        accepted.kind(),
        SnapshotOutcomeKind::Accepted { replaced: true }
    );
    assert_ne!(
        fs::read(&baseline).expect("updated baseline"),
        accepted_bytes
    );
    assert!(artifact_dir.join("diff.png").exists());
}

#[test]
fn create_missing_never_replaces_an_existing_baseline() {
    let workspace = tempdir().expect("temporary workspace");
    let store = SnapshotStore::new(
        workspace.path().join("snapshots"),
        workspace.path().join("artifacts"),
    );
    assertion("card", TestImage::solid((1, 1), [0, 0, 0, 255]), &store)
        .mode(SnapshotMode::CreateMissing)
        .check()
        .expect("create baseline");
    let original = fs::read(store.baseline_dir().join("card.png")).expect("baseline");

    let error = assertion("card", TestImage::solid((1, 1), [1, 0, 0, 255]), &store)
        .mode(SnapshotMode::CreateMissing)
        .check()
        .expect_err("existing mismatch must not be replaced");
    assert!(matches!(error, SnapshotTestError::Mismatch(_)));
    assert_eq!(
        fs::read(store.baseline_dir().join("card.png")).expect("baseline"),
        original
    );
}

#[test]
fn tolerance_and_dimension_mismatches_are_structured() {
    let workspace = tempdir().expect("temporary workspace");
    let store = SnapshotStore::new(
        workspace.path().join("snapshots"),
        workspace.path().join("artifacts"),
    );
    assertion("card", TestImage::solid((2, 1), [10, 20, 30, 255]), &store)
        .mode(SnapshotMode::CreateMissing)
        .check()
        .expect("create baseline");

    let matched = assertion("card", TestImage::solid((2, 1), [12, 20, 30, 255]), &store)
        .policy(ComparisonPolicy::PixelTolerance {
            channel_delta_threshold: 2,
            max_pixels_over_threshold: 0,
        })
        .check()
        .expect("delta at threshold is tolerated");
    assert_eq!(matched.stats().expect("stats").maximum_channel_delta(), 2);

    let error = assertion("card", TestImage::solid((3, 1), [10, 20, 30, 255]), &store)
        .check()
        .expect_err("dimensions must differ");
    let Difference::Dimensions(dimensions) =
        error.mismatch().expect("mismatch report").difference()
    else {
        panic!("expected dimension mismatch");
    };
    assert_eq!(dimensions.expected(), (2, 1));
    assert_eq!(dimensions.actual(), (3, 1));
}

#[test]
fn snapshot_names_use_a_portable_closed_grammar() {
    for name in [
        "",
        " ",
        "/absolute",
        "../escape",
        "a/../escape",
        "a//b",
        "a\\b",
        "CON",
        "nested/com1.txt",
        "trailing.",
        "already.png",
        "already.PNG",
        "中文",
    ] {
        assert!(
            SnapshotName::new(name).is_err(),
            "name should fail: {name:?}"
        );
    }
    let name = SnapshotName::new("settings/default.zh-CN.light").expect("portable name");
    assert_eq!(name.as_str(), "settings/default.zh-CN.light");
}

#[test]
fn parses_only_documented_modes() {
    assert_eq!("verify".parse(), Ok(SnapshotMode::Verify));
    assert_eq!("create-missing".parse(), Ok(SnapshotMode::CreateMissing));
    assert_eq!("accept".parse(), Ok(SnapshotMode::Accept));
    assert!("ACCEPT".parse::<SnapshotMode>().is_err());
    assert!("update".parse::<SnapshotMode>().is_err());
}

#[test]
fn normalizes_grayscale_png_baselines_to_rgba8() {
    let workspace = tempdir().expect("temporary workspace");
    let store = SnapshotStore::new(
        workspace.path().join("snapshots"),
        workspace.path().join("artifacts"),
    );
    fs::create_dir_all(store.baseline_dir()).expect("baseline directory");
    let path = store.baseline_dir().join("gray.png");
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, 2, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("grayscale header")
            .write_image_data(&[10, 200])
            .expect("grayscale pixels");
    }
    fs::write(path, png_bytes).expect("grayscale baseline");

    let actual = TestImage {
        size: (2, 1),
        pixels: vec![10, 10, 10, 255, 200, 200, 200, 255],
    };
    let outcome = assertion("gray", actual, &store)
        .check()
        .expect("normalized grayscale baseline should match");
    assert_eq!(outcome.kind(), SnapshotOutcomeKind::Matched);
}

#[test]
fn enforces_actual_and_decoded_baseline_pixel_limits() {
    let workspace = tempdir().expect("temporary workspace");
    let store = SnapshotStore::new(
        workspace.path().join("snapshots"),
        workspace.path().join("artifacts"),
    );

    let zero = assertion("zero", TestImage::solid((1, 1), [0; 4]), &store)
        .max_pixels(0)
        .check()
        .expect_err("zero limit must fail");
    assert!(matches!(zero, SnapshotTestError::InvalidPixelLimit { .. }));

    let actual = assertion("actual-too-large", TestImage::solid((2, 2), [0; 4]), &store)
        .max_pixels(3)
        .check()
        .expect_err("actual image must respect the limit");
    assert!(matches!(
        actual,
        SnapshotTestError::ActualPixelLimitExceeded { pixels: 4, .. }
    ));

    assertion(
        "baseline-too-large",
        TestImage::solid((2, 2), [0; 4]),
        &store,
    )
    .mode(SnapshotMode::CreateMissing)
    .check()
    .expect("create larger baseline");
    let baseline = assertion(
        "baseline-too-large",
        TestImage::solid((1, 1), [0; 4]),
        &store,
    )
    .max_pixels(3)
    .check()
    .expect_err("decoded baseline must respect the limit");
    assert!(matches!(
        baseline,
        SnapshotTestError::BaselinePixelLimitExceeded { pixels: 4, .. }
    ));
}

#[test]
fn omits_an_oversized_diff_canvas_and_removes_a_stale_diff() {
    let workspace = tempdir().expect("temporary workspace");
    let store = SnapshotStore::new(
        workspace.path().join("snapshots"),
        workspace.path().join("artifacts"),
    );
    assertion("cross", TestImage::solid((1, 4), [0; 4]), &store)
        .mode(SnapshotMode::CreateMissing)
        .check()
        .expect("create baseline");
    let artifact_dir = store.artifact_dir().join("cross");
    fs::create_dir_all(&artifact_dir).expect("artifact directory");
    fs::write(artifact_dir.join("diff.png"), b"stale").expect("stale diff");

    let error = assertion("cross", TestImage::solid((4, 1), [0; 4]), &store)
        .max_pixels(4)
        .check()
        .expect_err("dimensions differ");
    let artifacts = error.mismatch().expect("mismatch").artifacts();
    assert!(artifacts.diff().is_none());
    assert!(!artifact_dir.join("diff.png").exists());
    assert!(artifacts.expected().exists());
    assert!(artifacts.actual().exists());
}
