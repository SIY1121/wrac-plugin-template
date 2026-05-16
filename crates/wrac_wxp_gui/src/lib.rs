//! Helper that exposes the wxp WebView GUI as a `wrac_clap_adapter::PluginGui`.
//!
//! Parts that need to know about the CLAP ABI remain in `wrac_clap_adapter`. This crate
//! is responsible only for toolkit conversion of window handles and the thread affinity
//! of the GUI runtime.

mod controller;
mod dpi;
mod runtime;
mod window;

pub use controller::{GuiSizeLimits, WxpGuiController, WxpGuiResizeHandle};
pub use dpi::{DpiConverter, gui_size_to_logical, logical_size_to_gui};
pub use runtime::{WxpGuiFactory, WxpGuiRuntime};
pub use window::ParentWindowHandle;
