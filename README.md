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
use slint_snapshot::PreviewRuntime;

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
    let runtime = PreviewRuntime::new()?;
    let ui = Preview::new()?;

    runtime.set_size(ui.window(), (320, 180), 1.0)?;
    runtime
        .render_rgba(ui.window())?
        .write_png(std::path::Path::new("preview.png"))?;

    Ok(())
}
```

Use `render_rgba` to access pixels, `encode_png` for in-memory PNG data, or
`write_png` to save an image.

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

- Create one `PreviewRuntime` per process and keep all Slint operations on its
  creating thread.
- `PreviewRuntime` is neither `Send` nor `Sync`.
- Frames are limited to 16,777,216 physical pixels by default. Use
  `PreviewRuntime::with_max_pixels` for trusted larger canvases.
