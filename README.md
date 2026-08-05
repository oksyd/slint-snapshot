# slint-snapshot

[![Crates.io](https://img.shields.io/crates/v/slint-snapshot.svg)](https://crates.io/crates/slint-snapshot)
[![Documentation](https://docs.rs/slint-snapshot/badge.svg)](https://docs.rs/slint-snapshot)
[![CI](https://github.com/oksyd/slint-snapshot/actions/workflows/ci.yml/badge.svg)](https://github.com/oksyd/slint-snapshot/actions/workflows/ci.yml)

`slint-snapshot` renders compiled Slint components without a display server.
It uses Slint's software renderer and returns tightly packed RGBA pixels or PNG
data, making it suitable for previews, snapshot tests, and image tools.

## Usage

```rust
use slint::ComponentHandle;
use slint_snapshot::SnapshotRuntime;

slint::slint! {
    export component Preview inherits Window {
        background: #202538;

        Rectangle {
            width: 96px;
            height: 64px;
            background: #7c5cff;
            border-radius: 12px;
        }
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

Use `render` to access pixels, `encode_png` for in-memory PNG data, or
`write_png` to save an image.

For deterministic timers and animations, opt into the manual clock before the
Slint platform is installed:

```rust
use std::time::Duration;
use slint_snapshot::{SnapshotRuntime, runtime::ClockMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = SnapshotRuntime::builder()
        .clock_mode(ClockMode::Manual)
        .build()?;
    runtime.advance_time(Duration::from_millis(250))?;
    Ok(())
}
```

## Visual regression tests

Enable the optional baseline-management layer:

```toml
[dev-dependencies]
slint-snapshot = { version = "0.1", features = ["testing"] }
```

The default mode only verifies an existing PNG and never creates or modifies a
baseline. Mismatches against an existing baseline write reviewable artifacts
under `target` while leaving the accepted image unchanged:

```rust
use slint_snapshot::comparison::ComparisonPolicy;
use slint_snapshot::testing::{
    SnapshotAssertion, SnapshotStore,
};

fn check(
    frame: slint_snapshot::RenderedFrame,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = SnapshotStore::new(
        "tests/snapshots",
        "target/slint-snapshots",
    );
    SnapshotAssertion::try_new("settings/default.zh-CN.light", frame)?
        .store(store)
        .policy(ComparisonPolicy::Exact)
        .check()?;
    Ok(())
}
```

The resulting paths are separated deliberately:

```text
tests/snapshots/settings/default.zh-CN.light.png

target/slint-snapshots/settings/default.zh-CN.light/
├── expected.png
├── actual.png
└── diff.png
```

Create only baselines that are missing with an explicit Rust mode:

```rust
use slint_snapshot::testing::{SnapshotAssertion, SnapshotMode};

fn create(
    frame: slint_snapshot::RenderedFrame,
) -> Result<(), Box<dyn std::error::Error>> {
    SnapshotAssertion::try_new("settings/default", frame)?
        .mode(SnapshotMode::CreateMissing)
        .check()?;
    Ok(())
}
```

Accept a reviewed rendering just as explicitly:

```rust
use slint_snapshot::testing::{SnapshotAssertion, SnapshotMode};

fn accept(
    frame: slint_snapshot::RenderedFrame,
) -> Result<(), Box<dyn std::error::Error>> {
    SnapshotAssertion::try_new("settings/default", frame)?
        .mode(SnapshotMode::Accept)
        .check()?;
    Ok(())
}
```

`SnapshotTestError::mismatch()` exposes the policy, statistics and current
artifact paths for custom test reporters. Its `Display` implementation includes
the same actionable paths for ordinary `cargo test` failures.

To review and accept current output across tests that opt into
`SnapshotMode::from_env()`, pass the returned mode to `.mode(...)` and run:

```bash
SLINT_SNAPSHOT_MODE=accept cargo test --features testing
```

`Accept` is intentionally never selected implicitly. Invalid environment values
are errors. For small renderer differences, callers can explicitly choose
`ComparisonPolicy::PixelTolerance`; exact RGBA comparison remains the default.
Snapshot names are validated, but configured baseline and artifact roots must be
trusted directories. Give concurrently running tests unique snapshot names.

Failure artifacts are intentionally retained after a later successful check.
Only paths returned by the current error report describe the current failure;
the next failure for the same snapshot atomically replaces its artifacts.

Image comparison is independent from time control. Use `ClockMode::Manual`, a
fixed logical size and scale, embedded resources, and fixed fonts when stable
visual regression output is required.

## Fonts and resources

The default configuration does not use host fonts or link Fontconfig. For
portable and reproducible rendering, embed fonts and images when compiling the
consumer's `.slint` files:

```rust
fn main() -> Result<(), slint_build::CompileError> {
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    slint_build::compile_with_config("ui/preview.slint", config)
}
```

```toml
[build-dependencies]
slint-build = "=1.17.1"
```

Enable host font discovery only when required:

```toml
slint-snapshot = { version = "0.1", features = ["system-fonts"] }
```

On Linux, `system-fonts` requires Fontconfig development files when building
and Fontconfig with installed fonts at runtime. Rendering may then vary between
hosts.

## Runtime constraints

- Create one `SnapshotRuntime` per process and keep all Slint operations on its
  creating thread.
- `SnapshotRuntime` is neither `Send` nor `Sync`.
- Frames are limited to 16,777,216 physical pixels by default. Use
  `SnapshotRuntime::builder().max_pixels(...)` for trusted larger canvases.
