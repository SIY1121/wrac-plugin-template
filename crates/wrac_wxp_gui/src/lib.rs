//! wxp WebView GUI を `wrac_clap_adapter::PluginGui` として公開する helper。
//!
//! CLAP ABI を知る必要がある部分は `wrac_clap_adapter` に残し、この crate は
//! window handle の toolkit 変換と GUI runtime の thread affinity だけを持つ。

mod controller;
mod dpi;
mod runtime;
mod window;

pub use controller::{GuiSizeLimits, WxpGuiController};
pub use dpi::{DpiConverter, gui_size_to_logical, logical_size_to_gui};
pub use runtime::{WxpGuiFactory, WxpGuiRuntime};
pub use window::ParentWindowHandle;
