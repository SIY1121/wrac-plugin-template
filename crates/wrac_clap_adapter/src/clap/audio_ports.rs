use std::ffi::c_char;
use std::ptr;

use clap_sys::ext::audio_ports::{
    CLAP_AUDIO_PORT_IS_MAIN, CLAP_AUDIO_PORT_PREFERS_64BITS,
    CLAP_AUDIO_PORT_REQUIRES_COMMON_SAMPLE_SIZE, CLAP_AUDIO_PORT_SUPPORTS_64BITS, CLAP_PORT_MONO,
    CLAP_PORT_STEREO, clap_audio_port_info, clap_plugin_audio_ports,
};
use clap_sys::plugin::clap_plugin;

use super::PluginInstance;
use super::ffi::{ffi_bool, ffi_u32, fill_c_char_array};
use crate::{AudioPortFlags, AudioPortType};

pub(super) static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(audio_ports_count),
    get: Some(audio_ports_get),
};

unsafe extern "C" fn audio_ports_count(plugin: *const clap_plugin, is_input: bool) -> u32 {
    ffi_u32(|| {
        let Some(instance) = (unsafe { PluginInstance::from_plugin(plugin) }) else {
            return 0;
        };
        // wrapper format では、別の lifecycle callback が core write lock を持つ最中に
        // port metadata を問い合わせることがある。host 側 query path を plugin deadlock
        // に巻き込むより、この瞬間だけ「取得不可」と返す方が安全。
        let Some(core) = instance.core.try_read() else {
            return 0;
        };
        let Some(audio_ports) = core.audio_ports() else {
            return 0;
        };
        audio_ports.audio_port_count(is_input)
    })
}

unsafe extern "C" fn audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
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
        let Some(audio_ports) = core.audio_ports() else {
            return false;
        };
        let Some(port) = audio_ports.audio_port_info(index, is_input) else {
            return false;
        };

        unsafe {
            (*info).id = port.id;
            (*info).flags = audio_port_flags(port.flags);
            (*info).channel_count = port.channel_count;
            (*info).port_type = audio_port_type(port.port_type);
            (*info).in_place_pair = port.in_place_pair.unwrap_or(u32::MAX);
            fill_c_char_array(&mut (*info).name, port.name);
        }
        true
    })
}

fn audio_port_flags(flags: AudioPortFlags) -> u32 {
    let mut raw = 0;
    if flags.is_main {
        raw |= CLAP_AUDIO_PORT_IS_MAIN;
    }
    if flags.supports_64bits {
        raw |= CLAP_AUDIO_PORT_SUPPORTS_64BITS;
    }
    if flags.prefers_64bits {
        raw |= CLAP_AUDIO_PORT_PREFERS_64BITS;
    }
    if flags.requires_common_sample_size {
        raw |= CLAP_AUDIO_PORT_REQUIRES_COMMON_SAMPLE_SIZE;
    }
    raw
}

fn audio_port_type(port_type: AudioPortType) -> *const c_char {
    match port_type {
        AudioPortType::Unspecified => ptr::null(),
        AudioPortType::Mono => CLAP_PORT_MONO.as_ptr(),
        AudioPortType::Stereo => CLAP_PORT_STEREO.as_ptr(),
        AudioPortType::Other(name) => name.as_ptr(),
    }
}
