use wrac_clap_adapter::GuiSize;
use wxp::dpi::{LogicalPosition, LogicalSize, Size};

/// Conversion between CLAP GUI sizes and wxp bounds.
///
/// The unit expected by WebView differs between macOS/Windows and Linux.
/// Centralising the DPI branch here avoids duplicating it in every product.
pub struct DpiConverter {
    scale_factor: f64,
    uses_logical: bool,
}

impl DpiConverter {
    pub fn new(scale_factor: f64) -> Self {
        Self {
            scale_factor,
            uses_logical: cfg!(any(target_os = "macos", target_os = "windows")),
        }
    }

    pub fn set_scale(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
    }

    pub fn create_webview_bounds(&self, size: LogicalSize<f64>) -> wxp::Rect {
        wxp::Rect {
            position: LogicalPosition::new(0, 0).into(),
            size: if self.uses_logical {
                Size::Logical(size)
            } else {
                Size::Physical(size.to_physical(self.scale_factor))
            },
        }
    }
}

pub fn gui_size_to_logical(size: GuiSize) -> LogicalSize<f64> {
    LogicalSize::new(size.width as f64, size.height as f64)
}

/// Converts a wxp logical size back to a CLAP [`GuiSize`].
///
/// Keeping this as a public API prevents per-platform conversion logic from leaking
/// into product code when returning a WebView- or layout-derived size as a resize
/// request or state value.
pub fn logical_size_to_gui(size: LogicalSize<f64>) -> GuiSize {
    GuiSize {
        width: size.width as u32,
        height: size.height as u32,
    }
}
