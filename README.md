# slint-snapshot

[![Crates.io](https://img.shields.io/crates/v/slint-snapshot.svg)](https://crates.io/crates/slint-snapshot)
[![Documentation](https://docs.rs/slint-snapshot/badge.svg)](https://docs.rs/slint-snapshot)
[![CI](https://github.com/oksyd/slint-snapshot/actions/workflows/ci.yml/badge.svg)](https://github.com/oksyd/slint-snapshot/actions/workflows/ci.yml)

Headless software rendering and visual regression testing for compiled Slint
components. Frames are available as RGBA8 pixels, in-memory PNG data, or PNG
files.

## Rendering

```rust
use slint::ComponentHandle;
use slint_snapshot::SnapshotRuntime;

slint::slint! {
    export component Preview inherits Window {
        background: #202538;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = SnapshotRuntime::new()?;
    let ui = Preview::new()?;
    runtime.set_size(ui.window(), (320, 180), 1.0)?;
    runtime.render(ui.window())?
        .write_png(std::path::Path::new("preview.png"))?;
    Ok(())
}
```

`RenderedFrame` also provides `rgba8()` and `encode_png()`.

## Visual regression tests

```toml
[dev-dependencies]
slint-snapshot = { version = "0.1", features = ["testing"] }
```

```rust
use slint_snapshot::testing::{SnapshotAssertion, SnapshotMode};

fn check(frame: slint_snapshot::RenderedFrame) -> Result<(), Box<dyn std::error::Error>> {
    let mode = SnapshotMode::from_env()?.unwrap_or_default();
    SnapshotAssertion::try_new("settings/default.zh-CN.light", frame)?
        .mode(mode)
        .assert_match();
    Ok(())
}
```

The default mode is `Verify` and never changes a baseline.

| Mode | Behavior |
| --- | --- |
| `Verify` | Compare with an existing baseline |
| `CreateMissing` | Create only a missing baseline |
| `Accept` | Create or replace the baseline |

Tests that use `SnapshotMode::from_env()` can accept reviewed output with:

```bash
SLINT_SNAPSHOT_MODE=accept cargo test --features testing
```

Baselines and failure artifacts are kept separate:

```text
tests/snapshots/settings/default.zh-CN.light.png

target/slint-snapshots/settings/default.zh-CN.light/
├── expected.png
├── actual.png
└── diff.png
```

Exact RGBA comparison is the default. Use
`ComparisonPolicy::PixelTolerance` only when explicit renderer tolerance is
required. Structured mismatch statistics and artifact paths are available
through `SnapshotTestError::mismatch()`. Its `artifacts()` method returns a
`Result<_, &SnapshotWriteError>`: artifact write failures preserve the comparison
statistics. `MissingBaseline` similarly retains its primary cause, with
`actual_artifact: Result<PathBuf, SnapshotWriteError>` recording the artifact outcome.
Baseline and artifact roots must not be equal or nested.

## Deterministic output

Use a manual clock for timers and animations:

```rust
use slint_snapshot::{runtime::ClockMode, SnapshotRuntime};

let runtime = SnapshotRuntime::builder()
    .clock_mode(ClockMode::Manual)
    .build()?;
runtime.advance_time(std::time::Duration::from_millis(250))?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Also fix the logical size, scale factor, fonts, and embedded resources. The
default build does not use host fonts or link Fontconfig. Consumers should
compile resources for the software renderer:

```rust
let config = slint_build::CompilerConfiguration::new()
    .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
slint_build::compile_with_config("ui/preview.slint", config)?;
# Ok::<(), slint_build::CompileError>(())
```

```toml
[build-dependencies]
slint-build = "1.18.0"
```

Enable host fonts with the `system-fonts` feature. On Linux this requires
Fontconfig development files. With Slint 1.18 it also enables software
rendering for `Path` elements; the default Fontconfig-free configuration does
not render `Path` elements.

## Constraints

- Slint permits one platform per process; reuse one `SnapshotRuntime`.
- Components have independent windows and renderers. Inject events through
  `component.window()`; the runtime shares only configuration and the clock.
- Keep runtime operations on one thread; the runtime is neither `Send` nor
  `Sync`.
- Frames are limited to 16,777,216 physical pixels by default. Configure
  trusted larger canvases with `SnapshotRuntime::builder().max_pixels(...)`.
- Give concurrently running tests unique snapshot names.
