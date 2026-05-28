use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;
use wrac_clap_adapter::HostGuiResizeRequester;
use wrac_wxp_gui::{WxpGuiResizeHandle, global_pointer_position};
use wxp::WxpCommandHandler;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestGuiResizeRequest {
    width: f64,
    height: f64,
    drag_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeginGuiResizeDragRequest {
    drag_id: u64,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndGuiResizeDragRequest {
    drag_id: u64,
}

#[derive(Debug, Clone, Copy)]
struct NativeResizeDrag {
    drag_id: u64,
    start_mouse_x: f64,
    start_mouse_y: f64,
    start_width: f64,
    start_height: f64,
}

pub(super) fn register_resize_commands(
    command_handler: &Rc<WxpCommandHandler>,
    host_gui_resize_requester: Arc<dyn HostGuiResizeRequester>,
    gui_resize_handle: WxpGuiResizeHandle,
) {
    // Split resize dragging between two responsibilities. JS owns the gesture lifetime
    // (pointer capture/release are browser concepts). Rust owns the macOS coordinates
    // (because the browser coordinate space is exactly the surface the host is moving).
    // The drag ID links the browser-side trigger to this native snapshot so that every
    // resize request can be recomputed from the original desktop cursor position,
    // avoiding accumulated error from WebView-relative coordinates.
    let native_resize_drag = Rc::new(RefCell::new(None::<NativeResizeDrag>));

    {
        let native_resize_drag = native_resize_drag.clone();
        command_handler.register_sync("begin_gui_resize_drag", move |ctx| {
            let request = ctx
                .arg::<BeginGuiResizeDragRequest>("request")
                .map_err(|e| e.to_string())?;
            let Some(mouse) = global_pointer_position() else {
                return Ok::<_, String>(json!({ "ok": false }));
            };

            *native_resize_drag.borrow_mut() = Some(NativeResizeDrag {
                drag_id: request.drag_id,
                start_mouse_x: mouse.x,
                start_mouse_y: mouse.y,
                start_width: request.width,
                start_height: request.height,
            });
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    {
        let native_resize_drag = native_resize_drag.clone();
        command_handler.register_sync("end_gui_resize_drag", move |ctx| {
            let request = ctx
                .arg::<EndGuiResizeDragRequest>("request")
                .map_err(|e| e.to_string())?;
            let mut drag = native_resize_drag.borrow_mut();
            if drag
                .as_ref()
                .is_some_and(|drag| drag.drag_id == request.drag_id)
            {
                *drag = None;
            }
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    {
        let native_resize_drag = native_resize_drag.clone();
        command_handler.register_sync("request_gui_resize", move |ctx| {
            let request = ctx
                .arg::<RequestGuiResizeRequest>("request")
                .map_err(|e| e.to_string())?;

            let native_request = request.drag_id.and_then(|drag_id| {
                let drag = native_resize_drag.borrow();
                let drag = drag.as_ref().filter(|drag| drag.drag_id == drag_id)?;
                let mouse = global_pointer_position()?;
                Some((
                    drag.start_width + (mouse.x - drag.start_mouse_x),
                    drag.start_height + (mouse.y - drag.start_mouse_y),
                ))
            });

            let (width, height) = native_request.unwrap_or((request.width, request.height));
            let size = gui_resize_handle
                .request_resize(
                    wxp::dpi::LogicalSize::new(width, height),
                    ctx.webview(),
                    host_gui_resize_requester.as_ref(),
                )
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(json!({
                "ok": true,
                "width": size.width,
                "height": size.height,
            }))
        });
    }
}
