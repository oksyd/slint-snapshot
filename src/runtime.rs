//! Headless Slint runtime configuration and rendering.

use std::cell::Cell;
use std::error::Error as StdError;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{RepaintBufferType, SoftwareRenderer};
use slint::platform::{
    Platform, PlatformError, Renderer, WindowAdapter, WindowEvent, WindowProperties,
};
use slint::{LogicalSize, PhysicalSize, WindowSize};

use crate::frame::RenderedFrame;

/// Default upper bound for a rendered frame: 16,777,216 physical pixels.
///
/// An RGBA frame at this limit occupies 64 MiB before renderer and PNG encoder
/// overhead.
pub const DEFAULT_MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// Time source exposed to Slint timers and animations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ClockMode {
    /// Uses elapsed wall-clock time from runtime construction.
    #[default]
    RealTime,
    /// Starts at zero and advances only through [`SnapshotRuntime::advance_time`].
    Manual,
}

/// Configures a [`SnapshotRuntime`] before installing the global Slint
/// platform.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeBuilder {
    max_pixels: u64,
    clock_mode: ClockMode,
}

impl RuntimeBuilder {
    /// Creates a builder with [`DEFAULT_MAX_PIXELS`] and a real-time clock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum physical pixel count for one rendered frame.
    #[must_use]
    pub fn max_pixels(mut self, max_pixels: u64) -> Self {
        self.max_pixels = max_pixels;
        self
    }

    /// Selects the time source used by Slint timers and animations.
    #[must_use]
    pub fn clock_mode(mut self, clock_mode: ClockMode) -> Self {
        self.clock_mode = clock_mode;
        self
    }

    /// Installs the configured Slint platform.
    ///
    /// # Errors
    ///
    /// Returns an error when the pixel budget is zero or another Slint
    /// platform is already installed in this process.
    pub fn build(self) -> Result<SnapshotRuntime, RuntimeError> {
        if self.max_pixels == 0 {
            return Err(RuntimeError::InvalidPixelLimit {
                max_pixels: self.max_pixels,
            });
        }

        let window = HeadlessWindow::new(self.max_pixels);
        let clock = RuntimeClock::new(self.clock_mode);
        slint::platform::set_platform(Box::new(SnapshotPlatform {
            window: Rc::clone(&window),
            clock: clock.clone(),
        }))
        .map_err(|_| RuntimeError::PlatformAlreadyInitialized)?;
        Ok(SnapshotRuntime { window, clock })
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self {
            max_pixels: DEFAULT_MAX_PIXELS,
            clock_mode: ClockMode::RealTime,
        }
    }
}

/// Errors produced while configuring or using the headless Slint runtime.
#[derive(Debug)]
#[non_exhaustive]
pub enum RuntimeError {
    /// Slint already has a platform installed for the current process.
    PlatformAlreadyInitialized,
    /// Slint failed while operating on the component.
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
    /// Manual advancement was requested from a real-time clock.
    ManualClockRequired,
    /// Advancing the manual clock overflowed [`Duration`].
    ClockOverflow {
        elapsed: Duration,
        advance: Duration,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformAlreadyInitialized => {
                formatter.write_str("the Slint platform is already initialized")
            }
            Self::Platform { operation, message } => {
                write!(formatter, "Slint failed to {operation}: {message}")
            }
            Self::InvalidLogicalSize { width, height } => write!(
                formatter,
                "snapshot logical size must be non-zero, received {width}x{height}"
            ),
            Self::InvalidScaleFactor { scale_factor } => write!(
                formatter,
                "snapshot scale factor must be finite and positive, received {scale_factor}"
            ),
            Self::PhysicalSizeOverflow {
                width,
                height,
                scale_factor,
            } => write!(
                formatter,
                "snapshot size {width}x{height} at scale {scale_factor} cannot be represented"
            ),
            Self::InvalidPreferredSize { width, height } => write!(
                formatter,
                "Slint reported an invalid preferred size of {width}x{height}"
            ),
            Self::EmptyPhysicalSize { width, height } => write!(
                formatter,
                "snapshot physical size must be non-zero, received {width}x{height}"
            ),
            Self::PixelLimitExceeded {
                width,
                height,
                pixels,
                max_pixels,
            } => write!(
                formatter,
                "snapshot physical size {width}x{height} contains {pixels} pixels, exceeding the configured limit of {max_pixels}"
            ),
            Self::InvalidPixelLimit { max_pixels } => write!(
                formatter,
                "snapshot pixel limit must be non-zero, received {max_pixels}"
            ),
            Self::ManualClockRequired => {
                formatter.write_str("advancing time requires a manual runtime clock")
            }
            Self::ClockOverflow { elapsed, advance } => write!(
                formatter,
                "advancing the manual clock from {elapsed:?} by {advance:?} exceeds Duration"
            ),
        }
    }
}

impl StdError for RuntimeError {}

/// Owns the software-rendered, display-free Slint platform.
///
/// Slint permits only one platform per process. Create one runtime and reuse it
/// for every component rendered by a preview or test executable. This type is
/// neither `Send` nor `Sync`.
pub struct SnapshotRuntime {
    window: Rc<HeadlessWindow>,
    clock: RuntimeClock,
}

impl SnapshotRuntime {
    /// Installs a runtime with [`RuntimeBuilder::default`] configuration.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::PlatformAlreadyInitialized`] if another Slint
    /// platform was already installed.
    pub fn new() -> Result<Self, RuntimeError> {
        RuntimeBuilder::default().build()
    }

    /// Returns a configurable runtime builder.
    #[must_use]
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    /// Returns the underlying Slint window for advanced event injection.
    #[must_use]
    pub fn window(&self) -> &slint::Window {
        &self.window.window
    }

    /// Returns the maximum physical pixel count accepted for one frame.
    #[must_use]
    pub fn max_pixels(&self) -> u64 {
        self.window.max_pixels
    }

    /// Returns the configured clock mode.
    #[must_use]
    pub fn clock_mode(&self) -> ClockMode {
        self.clock.mode()
    }

    /// Returns the elapsed time currently exposed to Slint.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    /// Advances a manual clock and returns its new elapsed time.
    ///
    /// The next render updates Slint timers and animations at the new time.
    ///
    /// # Errors
    ///
    /// Returns an error when the runtime uses real time or the addition would
    /// overflow [`Duration`].
    pub fn advance_time(&self, advance: Duration) -> Result<Duration, RuntimeError> {
        self.clock.advance(advance)
    }

    /// Configures a component's logical size and display scale.
    ///
    /// Fractional scale factors such as `1.25` and `1.5` are supported. The
    /// resulting physical size is returned for snapshot metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty logical size, an invalid scale factor,
    /// physical dimension overflow, or a frame over the configured budget.
    pub fn set_size(
        &self,
        component_window: &slint::Window,
        logical_size: (u32, u32),
        scale_factor: f32,
    ) -> Result<PhysicalSize, RuntimeError> {
        let physical_size = checked_physical_size(logical_size, scale_factor)?;
        self.window.validate_physical_size(physical_size)?;

        self.window
            .window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        component_window.set_size(physical_size);
        Ok(physical_size)
    }

    /// Applies the component's current Slint preferred size and returns the
    /// selected logical dimensions.
    ///
    /// # Errors
    ///
    /// Returns an error when Slint cannot show or hide the component, when the
    /// preferred size is invalid, or when the resulting frame exceeds the
    /// configured budget.
    pub fn use_preferred_size(
        &self,
        component_window: &slint::Window,
        scale_factor: f32,
    ) -> Result<(u32, u32), RuntimeError> {
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
    /// Two passes are performed so layout and property changes requested
    /// during the first pass are reflected in the returned frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the component has no rendering surface, exceeds
    /// the configured pixel budget, or Slint cannot produce a snapshot.
    pub fn render(&self, component_window: &slint::Window) -> Result<RenderedFrame, RuntimeError> {
        slint::platform::update_timers_and_animations();
        self.window
            .validate_physical_size(self.window.physical_size())?;
        self.window.window.request_redraw();
        let _ = component_window
            .take_snapshot()
            .map_err(|error| platform_error("produce the initial snapshot pass", &error))?;
        slint::platform::update_timers_and_animations();
        self.window
            .validate_physical_size(self.window.physical_size())?;
        self.window.window.request_redraw();
        let pixels = component_window
            .take_snapshot()
            .map_err(|error| platform_error("produce the final snapshot pass", &error))?;
        Ok(RenderedFrame::from_pixels(pixels))
    }
}

impl fmt::Debug for SnapshotRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SnapshotRuntime")
            .field("max_pixels", &self.max_pixels())
            .field("clock_mode", &self.clock_mode())
            .field("elapsed", &self.elapsed())
            .finish_non_exhaustive()
    }
}

fn platform_error(operation: &'static str, error: &PlatformError) -> RuntimeError {
    RuntimeError::Platform {
        operation,
        message: error.to_string(),
    }
}

fn checked_preferred_size(size: LogicalSize) -> Result<(u32, u32), RuntimeError> {
    if !size.width.is_finite()
        || !size.height.is_finite()
        || size.width <= 0.0
        || size.height <= 0.0
        || f64::from(size.width.ceil()) > f64::from(u32::MAX)
        || f64::from(size.height.ceil()) > f64::from(u32::MAX)
    {
        return Err(RuntimeError::InvalidPreferredSize {
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
) -> Result<PhysicalSize, RuntimeError> {
    let (width, height) = logical_size;
    if width == 0 || height == 0 {
        return Err(RuntimeError::InvalidLogicalSize { width, height });
    }
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(RuntimeError::InvalidScaleFactor { scale_factor });
    }

    let physical_width = f64::from(width) * f64::from(scale_factor);
    let physical_height = f64::from(height) * f64::from(scale_factor);
    if physical_width < 1.0
        || physical_height < 1.0
        || physical_width > f64::from(u32::MAX)
        || physical_height > f64::from(u32::MAX)
    {
        return Err(RuntimeError::PhysicalSizeOverflow {
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

#[derive(Clone)]
enum RuntimeClock {
    RealTime { started_at: Instant },
    Manual { elapsed: Rc<Cell<Duration>> },
}

impl RuntimeClock {
    fn new(mode: ClockMode) -> Self {
        match mode {
            ClockMode::RealTime => Self::RealTime {
                started_at: Instant::now(),
            },
            ClockMode::Manual => Self::Manual {
                elapsed: Rc::new(Cell::new(Duration::ZERO)),
            },
        }
    }

    fn mode(&self) -> ClockMode {
        match self {
            Self::RealTime { .. } => ClockMode::RealTime,
            Self::Manual { .. } => ClockMode::Manual,
        }
    }

    fn elapsed(&self) -> Duration {
        match self {
            Self::RealTime { started_at } => started_at.elapsed(),
            Self::Manual { elapsed } => elapsed.get(),
        }
    }

    fn advance(&self, advance: Duration) -> Result<Duration, RuntimeError> {
        let Self::Manual { elapsed } = self else {
            return Err(RuntimeError::ManualClockRequired);
        };
        let current = elapsed.get();
        let updated = current
            .checked_add(advance)
            .ok_or(RuntimeError::ClockOverflow {
                elapsed: current,
                advance,
            })?;
        elapsed.set(updated);
        Ok(updated)
    }
}

struct SnapshotPlatform {
    window: Rc<HeadlessWindow>,
    clock: RuntimeClock,
}

struct HeadlessWindow {
    window: slint::Window,
    renderer: SoftwareRenderer,
    size: Cell<PhysicalSize>,
    preferred_size: Cell<LogicalSize>,
    max_pixels: u64,
}

impl HeadlessWindow {
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

    fn validate_physical_size(&self, size: PhysicalSize) -> Result<(), RuntimeError> {
        if size.width == 0 || size.height == 0 {
            return Err(RuntimeError::EmptyPhysicalSize {
                width: size.width,
                height: size.height,
            });
        }

        let pixels = u64::from(size.width) * u64::from(size.height);
        if pixels > self.max_pixels {
            return Err(RuntimeError::PixelLimitExceeded {
                width: size.width,
                height: size.height,
                pixels,
                max_pixels: self.max_pixels,
            });
        }
        Ok(())
    }
}

impl WindowAdapter for HeadlessWindow {
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

impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(Rc::clone(&self.window) as Rc<dyn WindowAdapter>)
    }

    fn duration_since_start(&self) -> Duration {
        self.clock.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use slint::{ComponentHandle, LogicalSize};

    use super::{
        ClockMode, RuntimeBuilder, RuntimeError, SnapshotRuntime, checked_physical_size,
        checked_preferred_size,
    };
    use crate::frame::FrameWriteError;

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
    fn renders_scaled_rgba_with_a_manual_clock_and_round_trips_png() {
        let runtime = SnapshotRuntime::builder()
            .max_pixels(100_000)
            .clock_mode(ClockMode::Manual)
            .build()
            .expect("snapshot runtime");
        let ui = DynamicPreferredSize::new().expect("dynamic snapshot component");

        assert_eq!(runtime.max_pixels(), 100_000);
        assert_eq!(runtime.clock_mode(), ClockMode::Manual);
        assert_eq!(runtime.elapsed(), Duration::ZERO);
        assert_eq!(
            runtime
                .advance_time(Duration::from_millis(16))
                .expect("advance manual clock"),
            Duration::from_millis(16)
        );
        assert_eq!(
            runtime
                .use_preferred_size(ui.window(), 1.5)
                .expect("base preferred size"),
            (240, 120)
        );
        assert_eq!(
            runtime.window.physical_size(),
            slint::PhysicalSize::new(360, 180)
        );

        ui.set_expanded(true);
        assert_eq!(
            runtime
                .use_preferred_size(ui.window(), 1.0)
                .expect("expanded preferred size"),
            (240, 180)
        );

        let frame = runtime.render(ui.window()).expect("RGBA frame");
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
            Err(FrameWriteError::InvalidOutputExtension { .. })
        ));
        assert!(matches!(
            runtime.set_size(ui.window(), (240, 180), 2.0),
            Err(RuntimeError::PixelLimitExceeded {
                pixels: 172_800,
                max_pixels: 100_000,
                ..
            })
        ));
        assert!(matches!(
            SnapshotRuntime::new(),
            Err(RuntimeError::PlatformAlreadyInitialized)
        ));
    }

    #[test]
    fn validates_configuration_and_sizes_before_platform_use() {
        assert!(matches!(
            checked_physical_size((0, 100), 1.0),
            Err(RuntimeError::InvalidLogicalSize {
                width: 0,
                height: 100
            })
        ));
        assert!(matches!(
            checked_physical_size((100, 100), 0.0),
            Err(RuntimeError::InvalidScaleFactor { .. })
        ));
        assert!(matches!(
            checked_physical_size((100, 100), f32::NAN),
            Err(RuntimeError::InvalidScaleFactor { .. })
        ));
        assert!(matches!(
            checked_physical_size((u32::MAX, 1), 2.0),
            Err(RuntimeError::PhysicalSizeOverflow { .. })
        ));
        assert!(matches!(
            checked_preferred_size(LogicalSize::new(f32::INFINITY, 100.0)),
            Err(RuntimeError::InvalidPreferredSize { .. })
        ));
        assert!(matches!(
            RuntimeBuilder::new().max_pixels(0).build(),
            Err(RuntimeError::InvalidPixelLimit { max_pixels: 0 })
        ));
    }

    #[test]
    fn real_time_clock_rejects_manual_advancement_without_installing_a_platform() {
        let clock = super::RuntimeClock::new(ClockMode::RealTime);
        assert_eq!(clock.mode(), ClockMode::RealTime);
        assert!(matches!(
            clock.advance(Duration::from_millis(1)),
            Err(RuntimeError::ManualClockRequired)
        ));
    }
}
