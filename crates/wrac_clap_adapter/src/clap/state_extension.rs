use clap_sys::ext::state::clap_plugin_state;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::{clap_istream, clap_ostream};

use super::PluginInstance;
use super::ffi::{ffi_bool, read_stream_exact, write_stream};
use crate::PluginState;

pub(super) static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(state_save),
    load: Some(state_load),
};

const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;

// state callback は host format により active 中にも来るため、adapter は core の
// `&mut self` 呼び出しとして lifecycle mutation と直列化する。既存 Processor と共有する
// state を audio-safe に更新する責務は、format 非依存の `PluginCore` 実装側に残す。
unsafe extern "C" fn state_save(plugin: *const clap_plugin, stream: *const clap_ostream) -> bool {
    ffi_bool(|| {
        if stream.is_null() {
            return false;
        }
        let Some(instance) = (unsafe { PluginInstance::from_plugin(plugin) }) else {
            return false;
        };
        // wrapper によっては UI/project-load path の中で state callback と metadata query
        // が再入する。Native CLAP の thread model 外で lock cycle を待つより、この state
        // 操作だけ失敗させる。
        let Some(mut core) = instance.core.try_write() else {
            return false;
        };
        let Some(state_support) = core.state() else {
            return false;
        };
        let Ok(state) = state_support.save_state() else {
            return false;
        };
        let len = state.bytes.len() as u32;
        unsafe { write_stream(stream, &len.to_le_bytes()) && write_stream(stream, &state.bytes) }
    })
}

unsafe extern "C" fn state_load(plugin: *const clap_plugin, stream: *const clap_istream) -> bool {
    ffi_bool(|| {
        if stream.is_null() {
            return false;
        }
        let Some(instance) = (unsafe { PluginInstance::from_plugin(plugin) }) else {
            return false;
        };
        let Some(len_bytes) = (unsafe { read_stream_exact(stream, 4) }) else {
            return false;
        };
        let len_bytes: [u8; 4] = match len_bytes.try_into() {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };
        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > MAX_STATE_BYTES {
            return false;
        }
        let Some(bytes) = (unsafe { read_stream_exact(stream, len) }) else {
            return false;
        };

        let Some(mut core) = instance.core.try_write() else {
            return false;
        };
        let Some(state_support) = core.state() else {
            return false;
        };
        state_support.restore_state(PluginState { bytes }).is_ok()
    })
}
