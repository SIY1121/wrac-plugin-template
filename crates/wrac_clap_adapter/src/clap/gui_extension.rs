use std::ffi::{CStr, c_char};
use std::ops::Deref;
use std::sync::Arc;

use clap_sys::ext::gui::{
    CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_gui_resize_hints,
    clap_plugin_gui, clap_window,
};
use clap_sys::plugin::clap_plugin;
use parking_lot::MutexGuard;

use super::PluginInstance;
use super::ffi::{ffi_bool, ffi_unit};
use crate::{ClapWindow, GuiApi, GuiConfiguration, GuiSize, PluginGui};

pub(super) static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(gui_is_api_supported),
    get_preferred_api: Some(gui_get_preferred_api),
    create: Some(gui_create),
    destroy: Some(gui_destroy),
    set_scale: Some(gui_set_scale),
    get_size: Some(gui_get_size),
    can_resize: Some(gui_can_resize),
    get_resize_hints: Some(gui_get_resize_hints),
    adjust_size: Some(gui_adjust_size),
    set_size: Some(gui_set_size),
    set_parent: Some(gui_set_parent),
    set_transient: Some(gui_set_transient),
    suggest_title: Some(gui_suggest_title),
    show: Some(gui_show),
    hide: Some(gui_hide),
};

unsafe extern "C" fn gui_is_api_supported(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_query(plugin) }) else {
            return false;
        };
        let Some(api) = gui_api_from_c(api) else {
            return false;
        };
        gui.is_api_supported(api, is_floating)
    })
}

unsafe extern "C" fn gui_get_preferred_api(
    plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    ffi_bool(|| {
        if api.is_null() || is_floating.is_null() {
            return false;
        }
        let Some(gui) = (unsafe { plugin_gui_query(plugin) }) else {
            return false;
        };
        let Some(configuration) = gui.preferred_api() else {
            return false;
        };

        unsafe {
            *api = gui_api_cstr(configuration.api).as_ptr();
            *is_floating = configuration.is_floating;
        }
        true
    })
}

unsafe extern "C" fn gui_create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "create") }) else {
            return false;
        };
        let Some(api) = gui_api_from_c(api) else {
            return false;
        };
        gui.create(GuiConfiguration { api, is_floating }).is_ok()
    })
}

unsafe extern "C" fn gui_destroy(plugin: *const clap_plugin) {
    ffi_unit(|| {
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "destroy") }) else {
            return;
        };
        gui.destroy();
    });
}

unsafe extern "C" fn gui_set_scale(plugin: *const clap_plugin, scale: f64) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "set_scale") }) else {
            return false;
        };
        gui.set_scale(scale).is_ok()
    })
}

unsafe extern "C" fn gui_get_size(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    ffi_bool(|| {
        if width.is_null() || height.is_null() {
            return false;
        }
        let Some(gui) = (unsafe { plugin_gui_query(plugin) }) else {
            return false;
        };
        let Ok(size) = gui.get_size() else {
            return false;
        };
        unsafe {
            *width = size.width;
            *height = size.height;
        }
        true
    })
}

unsafe extern "C" fn gui_can_resize(plugin: *const clap_plugin) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_query(plugin) }) else {
            return false;
        };
        gui.can_resize()
    })
}

unsafe extern "C" fn gui_get_resize_hints(
    plugin: *const clap_plugin,
    hints: *mut clap_gui_resize_hints,
) -> bool {
    ffi_bool(|| {
        if hints.is_null() {
            return false;
        }
        let Some(gui) = (unsafe { plugin_gui_query(plugin) }) else {
            return false;
        };
        let Some(resize_hints) = gui.resize_hints() else {
            return false;
        };
        unsafe {
            (*hints).can_resize_horizontally = resize_hints.can_resize_horizontally;
            (*hints).can_resize_vertically = resize_hints.can_resize_vertically;
            (*hints).preserve_aspect_ratio = resize_hints.preserve_aspect_ratio;
            (*hints).aspect_ratio_width = resize_hints.aspect_ratio_width;
            (*hints).aspect_ratio_height = resize_hints.aspect_ratio_height;
        }
        true
    })
}

unsafe extern "C" fn gui_adjust_size(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    ffi_bool(|| {
        if width.is_null() || height.is_null() {
            return false;
        }
        let Some(gui) = (unsafe { plugin_gui_query(plugin) }) else {
            return false;
        };
        let requested = unsafe {
            GuiSize {
                width: *width,
                height: *height,
            }
        };
        let Ok(adjusted) = gui.adjust_size(requested) else {
            return false;
        };
        unsafe {
            *width = adjusted.width;
            *height = adjusted.height;
        }
        true
    })
}

unsafe extern "C" fn gui_set_size(plugin: *const clap_plugin, width: u32, height: u32) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "set_size") }) else {
            return false;
        };
        gui.set_size(GuiSize { width, height }).is_ok()
    })
}

unsafe extern "C" fn gui_set_parent(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    ffi_bool(|| {
        if window.is_null() {
            return false;
        }
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "set_parent") }) else {
            return false;
        };
        let Some(parent) = (unsafe { clap_window_to_rust(&*window) }) else {
            return false;
        };
        gui.set_parent(parent).is_ok()
    })
}

unsafe extern "C" fn gui_set_transient(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    ffi_bool(|| {
        if window.is_null() {
            return false;
        }
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "set_transient") }) else {
            return false;
        };
        let Some(parent) = (unsafe { clap_window_to_rust(&*window) }) else {
            return false;
        };
        gui.set_transient(parent).is_ok()
    })
}

unsafe extern "C" fn gui_suggest_title(plugin: *const clap_plugin, title: *const c_char) {
    ffi_unit(|| {
        if title.is_null() {
            return;
        }
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "suggest_title") }) else {
            return;
        };
        let Ok(title) = (unsafe { CStr::from_ptr(title) }).to_str() else {
            return;
        };
        gui.suggest_title(title);
    });
}

unsafe extern "C" fn gui_show(plugin: *const clap_plugin) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "show") }) else {
            return false;
        };
        gui.show().is_ok()
    })
}

unsafe extern "C" fn gui_hide(plugin: *const clap_plugin) -> bool {
    ffi_bool(|| {
        let Some(gui) = (unsafe { plugin_gui_mutation(plugin, "hide") }) else {
            return false;
        };
        gui.hide().is_ok()
    })
}

unsafe fn plugin_gui_query(plugin: *const clap_plugin) -> Option<Arc<dyn PluginGui>> {
    let instance = unsafe { PluginInstance::from_plugin(plugin) }?;
    instance.gui.clone()
}

struct GuiMutationAccess {
    gui: Arc<dyn PluginGui>,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for GuiMutationAccess {
    type Target = dyn PluginGui;

    fn deref(&self) -> &Self::Target {
        self.gui.as_ref()
    }
}

unsafe fn plugin_gui_mutation(
    plugin: *const clap_plugin,
    callback_name: &'static str,
) -> Option<GuiMutationAccess> {
    let instance = unsafe { PluginInstance::from_plugin(plugin) }?;
    let Some(guard) = instance.gui_callback_busy.try_lock() else {
        log::error!("rejecting reentrant or concurrent CLAP GUI callback: {callback_name}");
        return None;
    };
    // GUI runtime は独自の thread-affinity と同期規則を持つ。adapter 向け handle を
    // 保持しておくことで、GUI callback を無関係な `PluginCore` lifecycle/state lock
    // と結合させない。
    instance
        .gui
        .clone()
        .map(|gui| GuiMutationAccess { gui, _guard: guard })
}

unsafe fn clap_window_to_rust(window: &clap_window) -> Option<ClapWindow> {
    let api = gui_api_from_c(window.api)?;
    match api {
        GuiApi::Cocoa => ClapWindow::cocoa(unsafe { window.specific.cocoa }),
        GuiApi::Win32 => ClapWindow::win32(unsafe { window.specific.win32 }),
        GuiApi::X11 => ClapWindow::x11(unsafe { window.specific.x11 }),
    }
}

fn gui_api_from_c(api: *const c_char) -> Option<GuiApi> {
    if api.is_null() {
        return None;
    }
    let api = unsafe { CStr::from_ptr(api) };
    if api == CLAP_WINDOW_API_COCOA {
        Some(GuiApi::Cocoa)
    } else if api == CLAP_WINDOW_API_WIN32 {
        Some(GuiApi::Win32)
    } else if api == CLAP_WINDOW_API_X11 {
        Some(GuiApi::X11)
    } else {
        None
    }
}

fn gui_api_cstr(api: GuiApi) -> &'static CStr {
    match api {
        GuiApi::Cocoa => CLAP_WINDOW_API_COCOA,
        GuiApi::Win32 => CLAP_WINDOW_API_WIN32,
        GuiApi::X11 => CLAP_WINDOW_API_X11,
    }
}
