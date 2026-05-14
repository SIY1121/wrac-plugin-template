use clap_sys::ext::note_ports::{clap_note_port_info, clap_plugin_note_ports};
use clap_sys::plugin::clap_plugin;

use super::PluginInstance;
use super::ffi::{ffi_bool, ffi_u32, fill_c_char_array};

pub(super) static NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(note_ports_count),
    get: Some(note_ports_get),
};

unsafe extern "C" fn note_ports_count(plugin: *const clap_plugin, is_input: bool) -> u32 {
    ffi_u32(|| {
        let Some(instance) = (unsafe { PluginInstance::from_plugin(plugin) }) else {
            return 0;
        };
        // note port query は軽量 metadata call です。wrapper 経由では lifecycle/state
        // 作業と競合し得るため、この path では `PluginCore` を待たない。
        let Some(core) = instance.core.try_read() else {
            return 0;
        };
        let Some(note_ports) = core.note_ports() else {
            return 0;
        };
        note_ports.note_port_count(is_input)
    })
}

unsafe extern "C" fn note_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    ffi_bool(|| {
        if info.is_null() {
            return false;
        }
        let Some(instance) = (unsafe { PluginInstance::from_plugin(plugin) }) else {
            return false;
        };
        let Some(core) = instance.core.try_read() else {
            return false;
        };
        let Some(note_ports) = core.note_ports() else {
            return false;
        };
        let Some(port) = note_ports.note_port_info(index, is_input) else {
            return false;
        };

        unsafe {
            (*info).id = port.id;
            (*info).supported_dialects = port.supported_dialects.bits();
            (*info).preferred_dialect = port.preferred_dialect.bits();
            fill_c_char_array(&mut (*info).name, port.name);
        }
        true
    })
}
