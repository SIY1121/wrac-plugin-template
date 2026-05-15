//! audio / GUI / host から共有される plugin state。
//!
//! この module は「値の SoT」と「状態不整合を防ぐ最小限の操作」だけを持つ。
//! GUI への配送や host への edit 通知は、それぞれ `gui.rs` / `commands.rs` 側で扱う。

use std::sync::atomic::{AtomicBool, Ordering};

use atomic_float::AtomicF32;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::plugin::{DEFAULT_GAIN, PARAM_BYPASS_ID, PARAM_GAIN_ID, clamp_gain};

/// DAW project に保存するが、audio thread からは読まない editor state。
///
/// window size のような host 管理の state ではなく、plugin UI 内の表示ページを例にしている。
/// 製品 plugin では IR path、track color、editor-only preference などもこの系統に入る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EditorPage {
    Controls,
    About,
}

impl Default for EditorPage {
    fn default() -> Self {
        Self::Controls
    }
}

impl EditorPage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Controls => "controls",
            Self::About => "about",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "controls" => Some(Self::Controls),
            "about" => Some(Self::About),
            _ => None,
        }
    }
}

/// DAW project に保存する non-realtime state。
///
/// audio thread はこの型も [`ProjectStateStore`] の lock も触らない。保存時は短く clone し、
/// 復元時は decode/validate 済みの snapshot を短く commit するだけにしておく。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ProjectState {
    pub(crate) editor_page: EditorPage,
}

/// ProjectState の SoT。
///
/// `RwLock` は non-realtime state の snapshot/commit だけに使う。lock 中で serialize、
/// host callback、GUI 同期 dispatch、file IO などを行わないことが重要。
pub(crate) struct ProjectStateStore {
    state: RwLock<ProjectState>,
}

impl ProjectStateStore {
    pub(crate) fn new() -> Self {
        Self {
            state: RwLock::new(ProjectState::default()),
        }
    }

    pub(crate) fn snapshot(&self) -> ProjectState {
        *self.state.read()
    }

    pub(crate) fn commit(&self, state: ProjectState) {
        *self.state.write() = state;
    }

    pub(crate) fn editor_page(&self) -> EditorPage {
        self.snapshot().editor_page
    }

    pub(crate) fn set_editor_page(&self, editor_page: EditorPage) {
        self.state.write().editor_page = editor_page;
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParameterStateSnapshot {
    pub(crate) gain: f32,
    pub(crate) bypass: bool,
}

/// audio processor / GUI / host からの問い合わせ が共有する thread-safe な state。
///
/// gain の値などは複数の thread から触られる:
/// - audio thread : [`wrac_clap_adapter::Processor::process`] の中で gain を読んで音に掛ける
/// - GUI thread   : ユーザーが slider を動かして gain を書き換える
/// - host thread  : [`wrac_clap_adapter::PluginParameters::parameter_value`] などで host が値を尋ねてくる
///
/// そのため realtime/current parameter values は [`std::sync::Arc`]<[`SharedState`]> として
/// 共有する。lock 不要な [`AtomicF32`] を使うことで audio thread を待たせない。
///
/// DAW project に保存する non-realtime state は [`ProjectStateStore`] に分ける。`save_state()`
/// は `SharedState` の parameter snapshot と `ProjectStateStore` の project snapshot を合成する。
pub(crate) struct SharedState {
    // gain の現在値 (線形 amplitude)。lock-free に読み書きする。
    gain: AtomicF32,
    // host の bypass parameter。audio thread から読むので lock-free に保持する。
    bypass: AtomicBool,
}

impl SharedState {
    pub(crate) fn new() -> Self {
        Self {
            gain: AtomicF32::new(DEFAULT_GAIN),
            bypass: AtomicBool::new(false),
        }
    }

    pub(crate) fn gain(&self) -> f32 {
        self.gain.load(Ordering::Acquire)
    }

    pub(crate) fn bypass(&self) -> bool {
        self.bypass.load(Ordering::Acquire)
    }

    pub(crate) fn snapshot_parameters(&self) -> ParameterStateSnapshot {
        // Gain では parameter 同士に強い transaction 境界がないため、単純な atomic load で十分。
        // 複数 field の完全一貫 snapshot が必要な製品では、この関数の中に seqlock 風の
        // generation check を閉じ込める。
        ParameterStateSnapshot {
            gain: self.gain(),
            bypass: self.bypass(),
        }
    }

    pub(crate) fn restore_parameters(&self, snapshot: ParameterStateSnapshot) {
        self.gain
            .store(clamp_gain(snapshot.gain), Ordering::Release);
        self.bypass.store(snapshot.bypass, Ordering::Release);
    }

    /// 指定された parameter の現在値を返す。
    ///
    /// 新しい parameter を追加するときは、この `match parameter_id` に読み出し処理を
    /// 追加する。GUI command は parameter id だけを見るので、command 名は増やさなくてよい。
    pub(crate) fn parameter_value(&self, parameter_id: u32) -> Option<f32> {
        match parameter_id {
            PARAM_GAIN_ID => Some(self.gain()),
            PARAM_BYPASS_ID => Some(f32::from(self.bypass())),
            _ => None,
        }
    }

    /// 外部から来た parameter 値を有効範囲に収めて SoT に保存する。
    ///
    /// 新しい parameter を追加するときは、この `match parameter_id` に保存処理を
    /// 追加する。各 parameter の clamp / normalization はここで完結させる。
    pub(crate) fn set_parameter_value(&self, parameter_id: u32, value: f64) -> Option<f32> {
        match parameter_id {
            PARAM_GAIN_ID => {
                // 範囲外の値が automation/UI から来ても問題ないように、必ず clamp してから保存する。
                let gain = clamp_gain(value as f32);
                self.gain.store(gain, Ordering::Release);
                Some(gain)
            }
            PARAM_BYPASS_ID => {
                let bypass = value >= 0.5;
                self.bypass.store(bypass, Ordering::Release);
                Some(f32::from(bypass))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProjectStateStore, SharedState};

    const fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn shared_state_is_send_sync() {
        assert_send_sync::<SharedState>();
    }

    #[test]
    fn project_state_store_is_send_sync() {
        assert_send_sync::<ProjectStateStore>();
    }
}
