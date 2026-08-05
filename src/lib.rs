//! Headless RGBA and PNG snapshots for compiled Slint components.
//!
//! Application-specific concerns such as scenario registration, fixtures,
//! locales, command-line parsing, and asset export belong in each project's
//! preview runner.
//!
//! Pure in-memory RGBA comparison is available in [`comparison`]. The optional
//! `testing` Cargo feature adds PNG baselines, explicit create/accept modes,
//! failure artifacts, and test assertions.
//!
//! The runtime does not connect to a desktop or windowing system. By default,
//! it also does not enable Slint's system-font stack or link the final preview
//! executable to Fontconfig.
//!
//! Slint permits only one platform per process, so a snapshot runtime should
//! normally live in a dedicated preview or test executable.
//!
//! # Threading
//!
//! The default feature set uses Slint's `unsafe-single-threaded` runtime.
//! [`SnapshotRuntime`] is neither `Send` nor `Sync`; create, use, and drop it on
//! one thread. The optional `system-fonts` feature enables `slint/std`.
//!
//! # Fonts and resources
//!
//! Consumers that render text or images without `system-fonts` must compile
//! their `.slint` files with
//! `slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer`. Import a fixed
//! font and select it with `default-font-family` when snapshot reproducibility
//! matters. Cargo features are additive, so another dependency enabling
//! `slint/std` also enables Slint's system-font stack for the final binary.
//!
//! # Example
//!
//! ```
//! use slint::ComponentHandle;
//! use slint_snapshot::SnapshotRuntime;
//!
//! slint::slint! {
//!     export component ColorCard inherits Window {
//!         background: #202538;
//!         Rectangle {
//!             width: 96px;
//!             height: 64px;
//!             background: #7c5cff;
//!             border-radius: 12px;
//!         }
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let runtime = SnapshotRuntime::new()?;
//! let ui = ColorCard::new().expect("create the Slint component");
//! runtime.set_size(ui.window(), (320, 180), 1.0)?;
//!
//! let frame = runtime.render(ui.window())?;
//! assert_eq!(frame.dimensions(), (320, 180));
//! let png = frame.encode_png()?;
//! assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
//! # Ok(())
//! # }
//! ```

pub mod comparison;
pub mod frame;
mod png_codec;
pub mod runtime;

#[cfg(feature = "testing")]
#[cfg_attr(docsrs, doc(cfg(feature = "testing")))]
pub mod testing;

pub use frame::RenderedFrame;
pub use runtime::SnapshotRuntime;
