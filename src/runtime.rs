use slint::platform::software_renderer::{RepaintBufferType, SoftwareRenderer};
use slint::platform::{
    Platform, PlatformError, Renderer, WindowAdapter, WindowEvent, WindowProperties,
};
use slint::{LogicalSize, PhysicalSize, Rgba8Pixel, SharedPixelBuffer, WindowSize};
use std::cell::Cell;
use std::error::Error as StdError;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

/// Default upper bound for a rendered frame: 16,777,216 physical pixels.
///
/// An RGBA frame at this limit occupies 64 MiB before renderer and PNG encoder
/// overhead. Use [`PreviewRuntime::with_max_pixels`] when a trusted preview
/// genuinely needs a larger canvas.
pub const DEFAULT_MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// Errors produced while configuring, rendering, encoding, or writing a
/// snapshot.
#[derive(Debug)]
#[non_exhaustive]
pub enum SnapshotError {
    /// Slint already has a platform installed for the current process.
    PlatformAlreadyInitialized,
    /// Slint failed while operating on the preview component.
    Platform {
        operation: &'static str,
        message: String,
    },
    /// The requested logical width or height is zero.
    InvalidLogicalSize { width: u32, height: u32 },
    /// The scale factor is zero, negative, infinite, or NaN.
    InvalidScaleFactor { scale_factor: f32 },
    /// Scaling the logical size cannot produce a valid physical size.
    PhysicalSizeOverflow {
        width: u32,
        height: u32,
        scale_factor: f32,
    },
    /// Slint reported an invalid preferred size.
    InvalidPreferredSize { width: f32, height: f32 },
    /// The component has an empty physical rendering surface.
    EmptyPhysicalSize { width: u32, height: u32 },
    /// The requested frame exceeds the runtime's memory safety budget.
    PixelLimitExceeded {
        width: u32,
        height: u32,
        pixels: u64,
        max_pixels: u64,
    },
    /// A zero-pixel runtime budget was requested.
    InvalidPixelLimit { max_pixels: u64 },
    /// Snapshot output paths must end in `.png`.
    InvalidOutputExtension { path: PathBuf },
    /// Encoding the frame as PNG failed.
    PngEncoding(png::EncodingError),
    /// Creating an output directory or writing a snapshot failed.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformAlreadyInitialized => {
                formatter.write_str("the Slint platform is already initialized")
            }
            Self::Platform { operation, message } => {
                write!(formatter, "Slint failed to {operation}: {message}")
            }
            Self::InvalidLogicalSize { width, height } => {
                write!(
                    formatter,
                    "snapshot logical size must be non-zero, received {width}x{height}"
                )
            }
            Self::InvalidScaleFactor { scale_factor } => {
                write!(
                    formatter,
                    "snapshot scale factor must be finite and positive, received {scale_factor}"
                )
            }
            Self::PhysicalSizeOverflow {
                width,
                height,
                scale_factor,
            } => {
                write!(
                    formatter,
                    "snapshot size {width}x{height} at scale {scale_factor} cannot be represented"
                )
            }
            Self::InvalidPreferredSize { width, height } => {
                write!(
                    formatter,
                    "Slint reported an invalid preferred size of {width}x{height}"
                )
            }
            Self::EmptyPhysicalSize { width, height } => {
                write!(
                    formatter,
                    "snapshot physical size must be non-zero, received {width}x{height}"
                )
            }
            Self::PixelLimitExceeded {
                width,
                height,
                pixels,
                max_pixels,
            } => {
                write!(
                    formatter,
                    "snapshot physical size {width}x{height} contains {pixels} pixels, exceeding the configured limit of {max_pixels}"
                )
            }
            Self::InvalidPixelLimit { max_pixels } => {
                write!(
                    formatter,
                    "snapshot pixel limit must be non-zero, received {max_pixels}"
                )
            }
            Self::InvalidOutputExtension { path } => {
                write!(
                    formatter,
                    "snapshot output must use the .png extension: {}",
                    path.display()
                )
            }
            Self::PngEncoding(_) => formatter.write_str("failed to encode the snapshot as PNG"),
            Self::Io {
                operation, path, ..
            } => {
                write!(
                    formatter,
                    "failed to {operation} snapshot path {}",
                    path.display()
                )
            }
        }
    }
}

impl StdError for SnapshotError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::PngEncoding(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::PlatformAlreadyInitialized
            | Self::Platform { .. }
            | Self::InvalidLogicalSize { .. }
            | Self::InvalidScaleFactor { .. }
            | Self::PhysicalSizeOverflow { .. }
            | Self::InvalidPreferredSize { .. }
            | Self::EmptyPhysicalSize { .. }
            | Self::PixelLimitExceeded { .. }
            | Self::InvalidPixelLimit { .. }
            | Self::InvalidOutputExtension { .. } => None,
        }
    }
}

impl From<png::EncodingError> for SnapshotError {
    fn from(source: png::EncodingError) -> Self {
        Self::PngEncoding(source)
    }
}

/// Owns the headless Slint platform used to render snapshots.
///
/// Slint permits only one platform per process. Create one runtime and reuse it
/// for every component rendered by a preview executable.
pub struct PreviewRuntime {
    window: Rc<PreviewWindow>,
}

impl PreviewRuntime {
    /// Installs a software-rendered Slint platform with
    /// [`DEFAULT_MAX_PIXELS`] as its frame budget.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::PlatformAlreadyInitialized`] if another Slint
    /// platform was already installed.
    pub fn new() -> Result<Self, SnapshotError> {
        Self::with_max_pixels(DEFAULT_MAX_PIXELS)
    }

    /// Installs a software-rendered Slint platform with a custom physical pixel
    /// budget.
    ///
    /// # Errors
    ///
    /// Returns an error when `max_pixels` is zero or another Slint platform was
    /// already installed.
    pub fn with_max_pixels(max_pixels: u64) -> Result<Self, SnapshotError> {
        if max_pixels == 0 {
            return Err(SnapshotError::InvalidPixelLimit { max_pixels });
        }

        let window = PreviewWindow::new(max_pixels);
        slint::platform::set_platform(Box::new(PreviewPlatform {
            window: Rc::clone(&window),
            started_at: Instant::now(),
        }))
        .map_err(|_| SnapshotError::PlatformAlreadyInitialized)?;
        Ok(Self { window })
    }

    /// Returns the underlying headless window for advanced event injection.
    #[must_use]
    pub fn window(&self) -> &PreviewWindow {
        &self.window
    }

    /// Returns the maximum number of physical pixels allowed in one frame.
    #[must_use]
    pub fn max_pixels(&self) -> u64 {
        self.window.max_pixels
    }

    /// Configures a component's logical size and display scale.
    ///
    /// Fractional scale factors such as `1.25` and `1.5` are supported. The
    /// resulting physical size is returned for callers that need to report
    /// snapshot metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty logical size, an invalid scale factor,
    /// physical dimension overflow, or a frame that exceeds the configured
    /// pixel budget.
    pub fn set_size(
        &self,
        component_window: &slint::Window,
        logical_size: (u32, u32),
        scale_factor: f32,
    ) -> Result<PhysicalSize, SnapshotError> {
        let physical_size = checked_physical_size(logical_size, scale_factor)?;
        self.window.validate_physical_size(physical_size)?;

        self.window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        component_window.set_size(physical_size);
        Ok(physical_size)
    }

    /// Applies the component's current Slint preferred size and returns the
    /// selected logical dimensions.
    ///
    /// This supports components whose preferred size changes with their state,
    /// such as validation and error messages.
    ///
    /// # Errors
    ///
    /// Returns an error when Slint cannot show or hide the component, when the
    /// preferred size is invalid, or when the resulting frame exceeds the
    /// configured pixel budget.
    pub fn use_preferred_size(
        &self,
        component_window: &slint::Window,
        scale_factor: f32,
    ) -> Result<(u32, u32), SnapshotError> {
        component_window.show().map_err(|error| {
            platform_error(
                "show the component while measuring its preferred size",
                &error,
            )
        })?;
        let preferred_size = self.window.preferred_size();
        component_window.hide().map_err(|error| {
            platform_error(
                "hide the component after measuring its preferred size",
                &error,
            )
        })?;

        let logical_size = checked_preferred_size(preferred_size)?;
        self.set_size(component_window, logical_size, scale_factor)?;
        Ok(logical_size)
    }

    /// Renders a component window into an in-memory RGBA frame.
    ///
    /// Two render passes are performed so layout and property changes requested
    /// during the first pass are reflected in the returned frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the component has no rendering surface, exceeds
    /// the configured pixel budget, or Slint cannot produce a snapshot.
    pub fn render_rgba(
        &self,
        component_window: &slint::Window,
    ) -> Result<RenderedFrame, SnapshotError> {
        slint::platform::update_timers_and_animations();
        self.window
            .validate_physical_size(self.window.physical_size())?;
        self.window.request_redraw();
        let _ = component_window
            .take_snapshot()
            .map_err(|error| platform_error("produce the initial snapshot pass", &error))?;
        slint::platform::update_timers_and_animations();
        self.window
            .validate_physical_size(self.window.physical_size())?;
        self.window.request_redraw();
        let pixels = component_window
            .take_snapshot()
            .map_err(|error| platform_error("produce the final snapshot pass", &error))?;
        Ok(RenderedFrame { pixels })
    }

    /// Renders a component and writes the resulting frame to a PNG file.
    ///
    /// # Errors
    ///
    /// Returns an error when rendering, PNG encoding, directory creation, or
    /// file writing fails.
    pub fn render_to_png(
        &self,
        component_window: &slint::Window,
        output: &Path,
    ) -> Result<RenderedFrame, SnapshotError> {
        let frame = self.render_rgba(component_window)?;
        frame.write_png(output)?;
        Ok(frame)
    }
}

fn platform_error(operation: &'static str, error: &PlatformError) -> SnapshotError {
    SnapshotError::Platform {
        operation,
        message: error.to_string(),
    }
}

fn checked_preferred_size(size: LogicalSize) -> Result<(u32, u32), SnapshotError> {
    if !size.width.is_finite()
        || !size.height.is_finite()
        || size.width <= 0.0
        || size.height <= 0.0
        || f64::from(size.width.ceil()) > f64::from(u32::MAX)
        || f64::from(size.height.ceil()) > f64::from(u32::MAX)
    {
        return Err(SnapshotError::InvalidPreferredSize {
            width: size.width,
            height: size.height,
        });
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok((size.width.ceil() as u32, size.height.ceil() as u32))
}

fn checked_physical_size(
    logical_size: (u32, u32),
    scale_factor: f32,
) -> Result<PhysicalSize, SnapshotError> {
    let (width, height) = logical_size;
    if width == 0 || height == 0 {
        return Err(SnapshotError::InvalidLogicalSize { width, height });
    }
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(SnapshotError::InvalidScaleFactor { scale_factor });
    }

    let physical_width = f64::from(width) * f64::from(scale_factor);
    let physical_height = f64::from(height) * f64::from(scale_factor);
    if physical_width < 1.0
        || physical_height < 1.0
        || physical_width > f64::from(u32::MAX)
        || physical_height > f64::from(u32::MAX)
    {
        return Err(SnapshotError::PhysicalSizeOverflow {
            width,
            height,
            scale_factor,
        });
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(PhysicalSize::new(
        physical_width as u32,
        physical_height as u32,
    ))
}

struct PreviewPlatform {
    window: Rc<PreviewWindow>,
    started_at: Instant,
}

/// Headless Slint window used by [`PreviewRuntime`].
///
/// Most consumers only need this type when dispatching synthetic input events
/// before rendering a scenario.
pub struct PreviewWindow {
    window: slint::Window,
    renderer: SoftwareRenderer,
    size: Cell<PhysicalSize>,
    preferred_size: Cell<LogicalSize>,
    max_pixels: u64,
}

impl PreviewWindow {
    fn new(max_pixels: u64) -> Rc<Self> {
        Rc::new_cyclic(|weak: &std::rc::Weak<Self>| Self {
            window: slint::Window::new(weak.clone()),
            renderer: SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::NewBuffer),
            size: Cell::new(PhysicalSize::default()),
            preferred_size: Cell::new(LogicalSize::default()),
            max_pixels,
        })
    }

    fn preferred_size(&self) -> LogicalSize {
        self.preferred_size.get()
    }

    fn physical_size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn validate_physical_size(&self, size: PhysicalSize) -> Result<(), SnapshotError> {
        if size.width == 0 || size.height == 0 {
            return Err(SnapshotError::EmptyPhysicalSize {
                width: size.width,
                height: size.height,
            });
        }

        let pixels = u64::from(size.width) * u64::from(size.height);
        if pixels > self.max_pixels {
            return Err(SnapshotError::PixelLimitExceeded {
                width: size.width,
                height: size.height,
                pixels,
                max_pixels: self.max_pixels,
            });
        }
        Ok(())
    }
}

impl std::ops::Deref for PreviewWindow {
    type Target = slint::Window;

    fn deref(&self) -> &Self::Target {
        &self.window
    }
}

impl WindowAdapter for PreviewWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }

    fn set_size(&self, size: WindowSize) {
        let scale_factor = self.window.scale_factor();
        self.size.set(size.to_physical(scale_factor));
        self.window.dispatch_event(WindowEvent::Resized {
            size: size.to_logical(scale_factor),
        });
    }

    fn size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }

    fn update_window_properties(&self, properties: WindowProperties<'_>) {
        self.preferred_size
            .set(properties.layout_constraints().preferred);
    }
}

impl Platform for PreviewPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(Rc::clone(&self.window) as Rc<dyn WindowAdapter>)
    }

    fn duration_since_start(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

/// A rendered Slint frame in eight-bit RGBA channel order.
///
/// The frame is the reusable boundary between Slint and image tools, snapshot
/// tests, encoders, and file exporters.
#[derive(Clone, Debug)]
pub struct RenderedFrame {
    pixels: SharedPixelBuffer<Rgba8Pixel>,
}

impl RenderedFrame {
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

    /// Encodes this frame as an in-memory PNG.
    ///
    /// The returned bytes can be sent directly as an `image/png` tool result
    /// without creating an intermediate file.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::PngEncoding`] when PNG encoding fails.
    pub fn encode_png(&self) -> Result<Vec<u8>, SnapshotError> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, self.width(), self.height());
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(self.rgba8())?;
        }
        Ok(bytes)
    }

    /// Encodes this frame and writes it to a PNG file.
    ///
    /// Missing parent directories are created after the frame has been encoded.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-PNG output path, PNG encoding failure,
    /// directory creation failure, or file write failure.
    pub fn write_png(&self, output: &Path) -> Result<(), SnapshotError> {
        let is_png = output
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
        if !is_png {
            return Err(SnapshotError::InvalidOutputExtension {
                path: output.to_path_buf(),
            });
        }

        let bytes = self.encode_png()?;
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| SnapshotError::Io {
                operation: "create the parent directory for",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(output, bytes).map_err(|source| SnapshotError::Io {
            operation: "write",
            path: output.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use slint::{ComponentHandle, LogicalSize};

    use super::{PreviewRuntime, SnapshotError, checked_physical_size, checked_preferred_size};

    slint::slint! {
        export component DynamicPreferredSize inherits Window {
            in property <bool> expanded;
            preferred-width: 240px;
            preferred-height: root.expanded ? 180px : 120px;
            min-width: self.preferred-width;
            min-height: self.preferred-height;
            background: #336699;
        }
    }

    #[test]
    fn renders_scaled_rgba_and_round_trips_png() {
        let runtime = PreviewRuntime::with_max_pixels(100_000).expect("preview runtime");
        let ui = DynamicPreferredSize::new().expect("dynamic preview component");

        assert_eq!(runtime.max_pixels(), 100_000);
        assert_eq!(
            runtime
                .use_preferred_size(ui.window(), 1.5)
                .expect("base preferred size"),
            (240, 120)
        );
        assert_eq!(
            runtime.window().physical_size(),
            slint::PhysicalSize::new(360, 180)
        );

        ui.set_expanded(true);
        assert_eq!(
            runtime
                .use_preferred_size(ui.window(), 1.0)
                .expect("expanded preferred size"),
            (240, 180)
        );

        let frame = runtime.render_rgba(ui.window()).expect("RGBA frame");
        assert_eq!(frame.dimensions(), (240, 180));
        assert_eq!(frame.rgba8().len(), 240 * 180 * 4);
        assert!(
            frame
                .rgba8()
                .chunks_exact(4)
                .all(|pixel| pixel == [0x33, 0x66, 0x99, 0xff])
        );

        let encoded = frame.encode_png().expect("in-memory PNG");
        let mut reader = png::Decoder::new(Cursor::new(encoded))
            .read_info()
            .expect("PNG header");
        let mut decoded = vec![
            0;
            reader
                .output_buffer_size()
                .expect("bounded PNG output buffer")
        ];
        let output = reader.next_frame(&mut decoded).expect("decoded PNG frame");
        assert_eq!((output.width, output.height), frame.dimensions());
        assert_eq!(output.color_type, png::ColorType::Rgba);
        assert_eq!(output.bit_depth, png::BitDepth::Eight);
        assert_eq!(&decoded[..output.buffer_size()], frame.rgba8());

        let invalid_output = std::env::temp_dir().join("slint-snapshot.invalid");
        assert!(matches!(
            frame.write_png(&invalid_output),
            Err(SnapshotError::InvalidOutputExtension { .. })
        ));

        assert!(matches!(
            runtime.set_size(ui.window(), (240, 180), 2.0),
            Err(SnapshotError::PixelLimitExceeded {
                pixels: 172_800,
                max_pixels: 100_000,
                ..
            })
        ));
        assert!(matches!(
            PreviewRuntime::new(),
            Err(SnapshotError::PlatformAlreadyInitialized)
        ));
    }

    #[test]
    fn rejects_invalid_sizes_before_rendering() {
        assert!(matches!(
            checked_physical_size((0, 100), 1.0),
            Err(SnapshotError::InvalidLogicalSize {
                width: 0,
                height: 100
            })
        ));
        assert!(matches!(
            checked_physical_size((100, 100), 0.0),
            Err(SnapshotError::InvalidScaleFactor { .. })
        ));
        assert!(matches!(
            checked_physical_size((100, 100), f32::NAN),
            Err(SnapshotError::InvalidScaleFactor { .. })
        ));
        assert!(matches!(
            checked_physical_size((u32::MAX, 1), 2.0),
            Err(SnapshotError::PhysicalSizeOverflow { .. })
        ));
        assert!(matches!(
            checked_preferred_size(LogicalSize::new(f32::INFINITY, 100.0)),
            Err(SnapshotError::InvalidPreferredSize { .. })
        ));
        assert!(matches!(
            PreviewRuntime::with_max_pixels(0),
            Err(SnapshotError::InvalidPixelLimit { max_pixels: 0 })
        ));
    }
}
