use wrac_clap_adapter::GuiSize;
use wxp::dpi::{LogicalPosition, LogicalSize, Size};

/// Conversion between CLAP GUI sizes and wxp bounds.
///
/// [`GuiSize`] values exchanged with the CLAP host are always in **physical pixels**.
/// The WebView expects **logical** coordinates on macOS/Windows and **physical**
/// coordinates on Linux. This converter centralises the DPI branch so that
/// product code never has to deal with per-platform pixel arithmetic.
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

    /// Converts a physical-pixel [`GuiSize`] from the host into a logical size
    /// suitable for internal layout calculations.
    pub fn gui_size_to_logical(&self, size: GuiSize) -> LogicalSize<f64> {
        LogicalSize::new(
            size.width as f64 / self.scale_factor,
            size.height as f64 / self.scale_factor,
        )
    }

    /// Converts a logical size back to a physical-pixel [`GuiSize`] for the host.
    pub fn logical_size_to_gui(&self, size: LogicalSize<f64>) -> GuiSize {
        GuiSize {
            width: (size.width * self.scale_factor).round() as u32,
            height: (size.height * self.scale_factor).round() as u32,
        }
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
