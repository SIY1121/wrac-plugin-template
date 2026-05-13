//! WXP Example Gain の `PluginCore` 実装と共有状態。
//!
//! このファイルが plugin の中心です。やっていることを大雑把に並べると:
//!
//! 1. plugin の自己紹介情報 (`PLUGIN_DESCRIPTOR`) を宣言する
//! 2. parameter 定義 (gain ひとつだけ) を host に教える
//! 3. parameter の現在値を audio thread / GUI / host から触れる
//!    `SharedState` を持つ
//! 4. host から `activate` されたら audio 処理用の `Processor` を作る
//! 5. host から GUI を開かれたら WebView runtime を作る
//! 6. state の save/restore (DAW の project に保存) を実装する
//!
//! CLAP / VST3 / AU といった plugin format の差分は `wrac_clap_adapter` が
//! 吸収するので、ここでは「gain plugin として何を持つか」だけに集中できます。

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use atomic_float::AtomicF32;
use novonotes_run_loop::{RunLoop, RunLoopSender};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wrac_clap_adapter::{
    ActivateContext, AudioPortConfigurationRequest, AudioPortFlags, AudioPortInfo, AudioPortType,
    Auv2Descriptor, GuiSize, HostParameterEditNotifier, ParameterFlags, ParameterInfo,
    ParameterValueEvent, PluginAudioPorts, PluginConfigurableAudioPorts, PluginCore,
    PluginCoreContext, PluginDescriptor, PluginError, PluginFeature, PluginGui, PluginParameters,
    PluginResult, PluginState, PluginStateSupport, Processor,
};
use wrac_wxp_gui::{GuiSizeLimits, WxpGuiController, WxpGuiRuntime};
use wxp::{Channel, WxpCommandHandler};

use crate::audio::WxpExampleGainAudioProcessor;
use crate::gui::WxpExampleGainGuiRuntime;

// plugin を識別する reverse-DNS 形式の ID。DAW が plugin を一意に判別するために
// 使うので、自分の plugin を作るときはここを必ず変更する。
pub(crate) const PLUGIN_ID: &str = "com.novo-notes.wxp-example-gain";

// 各 parameter にも host 内で一意の ID を割り当てる必要がある。
// gain がひとつだけなので 1 を割り振っている。
pub(crate) const PARAM_GAIN_ID: u32 = 1;

// gain の値域。1.0 が「そのまま (0 dB)」、0.0 が「無音 (-inf dB)」、
// 2.0 が「2 倍 (+6 dB)」を表す線形 amplitude。
pub(crate) const DEFAULT_GAIN: f32 = 1.0;
pub(crate) const MIN_GAIN: f32 = 0.0;
pub(crate) const MAX_GAIN: f32 = 2.0;

// GUI window のサイズ範囲 (pixel)。host は initial size でウインドウを開き、
// ユーザーがリサイズしたときは min..=max の範囲にクランプされる。
pub(crate) const DEFAULT_GUI_SIZE: GuiSize = GuiSize {
    width: 360,
    height: 360,
};
const MIN_GUI_SIZE: GuiSize = GuiSize {
    width: 280,
    height: 280,
};
const MAX_GUI_SIZE: GuiSize = GuiSize {
    width: 720,
    height: 720,
};

// host (DAW) に plugin を自己紹介するための静的データ。
// `wrac_clap_adapter` がこれを CLAP / AUv2 の descriptor 構造体へと変換する。
pub(crate) const PLUGIN_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: PLUGIN_ID,
    name: "WXP Example Gain",
    vendor: "NOVO NOTES",
    url: "",
    manual_url: "",
    support_url: "",
    version: "0.1.0",
    description: "Simple gain plugin",
    features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    // AUv2 (macOS の Audio Unit v2) 用の追加情報。
    // manufacturer_code と plugin_subtype は 4 文字 ASCII の固有 ID で、
    // 同じ会社内で重複しないように決める必要がある。
    auv2: Some(Auv2Descriptor {
        manufacturer_code: *b"NvNt",
        manufacturer_name: "NOVO NOTES",
        plugin_type: *b"aufx", // "aufx" = audio effect
        plugin_subtype: *b"WxGn",
    }),
};

/// plugin 1 instance を表す型。`PluginCore` trait の実装本体。
///
/// host (DAW) が plugin を読み込むごとにこの struct が 1 つずつ作られる。
/// audio 処理本体は `activate` で別途 `Processor` として切り出すので、
/// この struct 自身は lifecycle と factory の役目に徹する。
pub(crate) struct WxpExampleGainCore {
    // audio thread / GUI / host から共有して触る状態。
    // 詳細は `SharedState` の doc を参照。
    shared: Arc<SharedState>,
    // WebView による GUI を CLAP の GUI extension として扱うための helper。
    // `Arc` にしているのは host が `plugin_gui` を複数回問い合わせるため。
    gui: Arc<WxpGuiController>,
}

/// audio processor / GUI / host からの問い合わせ が共有する thread-safe な state。
///
/// gain の値などは複数の thread から触られる:
/// - audio thread : `process()` の中で gain を読んで音に掛ける
/// - GUI thread   : ユーザーが slider を動かして gain を書き換える
/// - host thread  : `parameter_base_value()` などで host が値を尋ねてくる
///
/// そのため値の "Single Source of Truth (SoT)" を `WxpExampleGainCore` の私有
/// field に置くのではなく、`Arc<SharedState>` として共有する。lock 不要な
/// `AtomicF32` を使うことで audio thread を待たせない実装になっている。
///
/// `WxpExampleGainCore` 自身は lifecycle (activate/deactivate) と factory の
/// 境界を持ち、real-time に読みたい値はすべてこの共有 state 経由に統一する。
pub(crate) struct SharedState {
    // gain の現在値 (線形 amplitude)。lock-free に読み書きする。
    gain: AtomicF32,
    // 現在の audio channel 数 (mono なら 1、stereo なら 2)。
    // host が port 構成を変えてきた場合に書き換えられる。
    audio_channel_count: AtomicU32,
    // automation 等で gain が更新されたが、まだ GUI に反映していないことを示す flag。
    // 詳細は `mark_gui_notification_pending` の解説を参照。
    pending_gui_notification: AtomicBool,
    // GUI が開いている間だけ Some。GUI 側 (WebView) に値の変化を push するための
    // channel と、UI thread の run loop に戻すための sender。
    gui_notifier: Mutex<Option<GuiNotifier>>,
}

#[derive(Clone)]
struct GuiNotifier {
    // UI thread (= GUI runtime の run loop) にクロージャを送るための sender。
    sender: RunLoopSender,
    // WebView 側 JS の subscriber に値を送るための channel。
    channel: Channel,
}

/// DAW project に保存される plugin state の serialize 形式。
///
/// `serde_json` で JSON にして bytes として host に渡し、復元時は逆に読み戻す。
/// gain の値だけを persist する単純な構造だが、新しい parameter を追加する際は
/// この struct を拡張して `restore_state` で欠落 field を default 値に埋めると
/// 古い project ファイルとの互換性を保ちやすい。
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SavedPluginState {
    pub(crate) gain: f32,
}

impl WxpExampleGainCore {
    pub(crate) fn new(context: PluginCoreContext) -> Self {
        let shared = Arc::new(SharedState::new());
        let host_parameter_edit_notifier = context.host_parameter_edit_notifier;

        // GUI は遅延生成 (host が `create_gui` を呼ぶまで作らない) する必要があるので、
        // ここでは「GUI が要求されたときに runtime を作る closure」だけ用意して
        // `WxpGuiController` に渡す。closure は shared を捕獲して runtime に渡す。
        let gui_shared = shared.clone();
        let gui_host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        let gui = Arc::new(
            WxpGuiController::new(
                move |configuration, initial_size, parent| {
                    WxpExampleGainGuiRuntime::create(
                        gui_shared.clone(),
                        gui_host_parameter_edit_notifier.clone(),
                        configuration,
                        initial_size,
                        parent,
                    )
                    .map(|runtime| Box::new(runtime) as Box<dyn WxpGuiRuntime>)
                },
                DEFAULT_GUI_SIZE,
            )
            .with_size_limits(GuiSizeLimits {
                min: MIN_GUI_SIZE,
                max: MAX_GUI_SIZE,
            }),
        );

        Self { shared, gui }
    }
}

/// `wrac_clap_adapter::export_clap_plugin!` から呼ばれる factory 関数。
///
/// host が新しい plugin instance を必要としたタイミングで adapter が呼び出し、
/// trait object として `PluginCore` を返す。実装の差し替えはここを変えるだけ。
pub(crate) fn create_plugin_core(context: PluginCoreContext) -> Box<dyn PluginCore> {
    Box::new(WxpExampleGainCore::new(context))
}

// ---------------------------------------------------------------------------
// PluginCore: plugin の lifecycle と、提供する extension の宣言
// ---------------------------------------------------------------------------
// `PluginCore` は plugin 一個分の lifecycle 全体を見る trait。各メソッドが
// 「この plugin は ○○ をサポートします」という宣言にもなっており、対応しない
// 機能では `None` を返せば OK。
impl PluginCore for WxpExampleGainCore {
    /// host が audio 処理を開始する直前に呼ばれる。
    /// ここで返した `Processor` が以降 audio thread 上で `process()` される。
    fn activate(&mut self, _context: ActivateContext) -> PluginResult<Box<dyn Processor>> {
        Ok(Box::new(WxpExampleGainAudioProcessor::new(
            self.shared.clone(),
        )))
    }

    /// host が audio 処理を停止したときに呼ばれる。
    /// `_processor` は `activate` で返した実体。drop すれば clean up される。
    fn deactivate(&mut self, _processor: Box<dyn Processor>) -> PluginResult<()> {
        Ok(())
    }

    // 以下は CLAP の各 extension の宣言。Some を返すと「この extension を実装している」、
    // None を返すと「未対応」になる。実装本体は別 impl ブロックに書く。

    fn audio_ports(&self) -> Option<&dyn PluginAudioPorts> {
        Some(self)
    }

    fn configurable_audio_ports(&mut self) -> Option<&mut dyn PluginConfigurableAudioPorts> {
        Some(self)
    }

    fn parameters(&self) -> Option<&dyn PluginParameters> {
        Some(self)
    }

    fn state(&mut self) -> Option<&mut dyn PluginStateSupport> {
        Some(self)
    }

    fn gui(&self) -> Option<Arc<dyn PluginGui>> {
        Some(self.gui.clone())
    }
}

// ---------------------------------------------------------------------------
// PluginAudioPorts: audio 入出力 port の宣言
// ---------------------------------------------------------------------------
// gain plugin なので「main in 1 つ」「main out 1 つ」のシンプルな構成。
// channel 数は `SharedState::audio_channel_count` から動的に取り出す。
impl PluginAudioPorts for WxpExampleGainCore {
    fn audio_port_count(&self, _is_input: bool) -> u32 {
        1
    }

    fn audio_port_info(&self, index: u32, is_input: bool) -> Option<AudioPortInfo> {
        let channel_count = self.shared.audio_channel_count();
        (index == 0).then_some(if is_input {
            AudioPortInfo {
                id: 1,
                name: "Main In",
                flags: AudioPortFlags {
                    is_main: true,
                    ..AudioPortFlags::default()
                },
                channel_count,
                port_type: audio_port_type(channel_count),
                in_place_pair: None,
            }
        } else {
            AudioPortInfo {
                id: 2,
                name: "Main Out",
                flags: AudioPortFlags {
                    is_main: true,
                    ..AudioPortFlags::default()
                },
                channel_count,
                port_type: audio_port_type(channel_count),
                in_place_pair: None,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// PluginConfigurableAudioPorts: host が port 構成を変えに来たときの応答
// ---------------------------------------------------------------------------
// 例えば host が「stereo から mono に切り替えたい」と提案してきたとき、
// plugin はそれを受理できるかを `can_apply_*` で答え、`apply_*` で実際に反映する。
impl PluginConfigurableAudioPorts for WxpExampleGainCore {
    fn can_apply_audio_port_configuration(
        &self,
        requests: &[AudioPortConfigurationRequest],
    ) -> bool {
        let accepted = resolve_audio_channel_count(self.shared.audio_channel_count(), requests);
        accepted.is_some()
    }

    fn apply_audio_port_configuration(
        &mut self,
        requests: &[AudioPortConfigurationRequest],
    ) -> PluginResult<()> {
        // gain DSP 自体は channel 数に依存しない実装になっているが、CLAP の port
        // metadata は clap-wrapper が後から渡してくる host buffer と一致している
        // 必要がある。negotiate した channel 数を shared state に保存し、後続の
        // port 問い合わせや新しい Processor からも同じ値が見えるようにする。
        let channel_count =
            resolve_audio_channel_count(self.shared.audio_channel_count(), requests)
                .ok_or(PluginError::InvalidState)?;
        self.shared.set_audio_channel_count(channel_count);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PluginParameters: parameter の宣言と現在値のやり取り
// ---------------------------------------------------------------------------
// host から見える parameter API。今回は gain ひとつだけなので、id が
// `PARAM_GAIN_ID` 以外の問い合わせはすべて `InvalidParameter` を返す。
impl PluginParameters for WxpExampleGainCore {
    fn parameter_count(&self) -> u32 {
        1
    }

    fn parameter_info(&self, index: u32) -> Option<ParameterInfo> {
        (index == 0).then_some(ParameterInfo {
            id: PARAM_GAIN_ID,
            name: "Gain",
            module: "",
            min_value: MIN_GAIN as f64,
            max_value: MAX_GAIN as f64,
            default_value: DEFAULT_GAIN as f64,
            flags: ParameterFlags {
                // automation 可能であることを host に伝える。これが false だと
                // DAW で parameter を自動化できなくなる。
                is_automatable: true,
                ..ParameterFlags::default()
            },
        })
    }

    /// host が「今この parameter の値はいくつ?」と尋ねてきたときに答える。
    fn parameter_base_value(&self, parameter_id: u32) -> PluginResult<f64> {
        if parameter_id != PARAM_GAIN_ID {
            return Err(PluginError::InvalidParameter);
        }
        Ok(self.shared.gain() as f64)
    }

    /// host から input event として parameter 値が届いたときの経路。
    fn apply_parameter_value(&self, event: ParameterValueEvent) -> PluginResult<f64> {
        if event.parameter_id != PARAM_GAIN_ID {
            return Err(PluginError::InvalidParameter);
        }
        let gain = self.shared.set_gain(event.value);
        self.shared.mark_gui_notification_pending();
        Ok(gain as f64)
    }

    /// 内部値 → 表示文字列。例: 1.0 → "0.0 dB"。
    fn parameter_value_to_text(&self, parameter_id: u32, value: f64) -> PluginResult<String> {
        if parameter_id != PARAM_GAIN_ID {
            return Err(PluginError::InvalidParameter);
        }
        Ok(gain_db_text(clamp_gain(value as f32) as f64))
    }

    /// 表示文字列 → 内部値。ユーザーが host UI に "3 dB" のように入力したとき呼ばれる。
    fn parameter_text_to_value(&self, parameter_id: u32, text: &str) -> PluginResult<f64> {
        if parameter_id != PARAM_GAIN_ID {
            return Err(PluginError::InvalidParameter);
        }

        let text = text.trim();
        let text = text.strip_suffix("dB").unwrap_or(text).trim();
        let db = text
            .parse::<f64>()
            .map_err(|_| PluginError::InvalidParameter)?;
        // dB → 線形 amplitude に変換してから clamp。
        Ok(clamp_gain(10.0_f64.powf(db / 20.0) as f32) as f64)
    }
}

// ---------------------------------------------------------------------------
// PluginStateSupport: state の保存と復元 (DAW project への persist)
// ---------------------------------------------------------------------------
// DAW がプロジェクトを保存するときに `save_state` が、開くときに `restore_state` が
// 呼ばれる。bytes フォーマットは plugin 側で自由に決められるので、ここでは
// JSON にしておく (人が読めるとデバッグが楽)。
impl PluginStateSupport for WxpExampleGainCore {
    fn save_state(&mut self) -> PluginResult<PluginState> {
        let bytes = serde_json::to_vec(&SavedPluginState {
            gain: self.shared.gain(),
        })
        .map_err(|_| PluginError::InvalidState)?;
        Ok(PluginState { bytes })
    }

    fn restore_state(&mut self, state: PluginState) -> PluginResult<()> {
        let state: SavedPluginState =
            serde_json::from_slice(&state.bytes).map_err(|_| PluginError::InvalidState)?;
        self.shared.set_gain(state.gain as f64);
        self.shared.notify_gui();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SharedState: 共有 state と state 整合性のための操作
// ---------------------------------------------------------------------------
// ここでは clamp 済みの値を SoT に保存することと、GUI へ安全に通知する手段だけを
// 提供する。host に edit 通知するか、GUI 通知を即時に行うか延期するかは呼び出し元の
// workflow 側で決める。
impl SharedState {
    fn new() -> Self {
        Self {
            gain: AtomicF32::new(DEFAULT_GAIN),
            // template の default は stereo。host が configure してくれば書き換わる。
            audio_channel_count: AtomicU32::new(2),
            pending_gui_notification: AtomicBool::new(false),
            gui_notifier: Mutex::new(None),
        }
    }

    pub(crate) fn gain(&self) -> f32 {
        self.gain.load(Ordering::Acquire)
    }

    pub(crate) fn audio_channel_count(&self) -> u32 {
        self.audio_channel_count.load(Ordering::Acquire)
    }

    fn set_audio_channel_count(&self, channel_count: u32) {
        self.audio_channel_count
            .store(channel_count, Ordering::Release);
    }

    /// 外部から来た gain を有効範囲に収めて SoT に保存する。
    pub(crate) fn set_gain(&self, gain: f64) -> f32 {
        // 範囲外の値が automation/UI から来ても問題ないように、必ず clamp してから保存する。
        let gain = clamp_gain(gain as f32);
        self.gain.store(gain, Ordering::Release);
        gain
    }

    /// audio/process 経路では GUI へ直接通知せず、GUI runtime の timer に反映を任せる。
    pub(crate) fn mark_gui_notification_pending(&self) {
        self.pending_gui_notification.store(true, Ordering::Release);
    }

    /// GUI が起動したときに WebView 向け channel を登録する。
    /// `RunLoop::sender()` を一緒に保持して、後から UI thread に処理を戻せるようにする。
    pub(crate) fn set_gui_channel(&self, channel: Channel) {
        *self.gui_notifier.lock() = Some(GuiNotifier {
            sender: RunLoop::sender(),
            channel,
        });
    }

    /// GUI が閉じられたときに呼ぶ。以降 `notify_gui` は no-op になる。
    pub(crate) fn clear_gui_channel(&self) {
        *self.gui_notifier.lock() = None;
    }

    /// GUI runtime の timer から定期的に呼ばれる。
    /// `mark_gui_notification_pending` で立てた dirty flag をここで回収して UI に流す。
    pub(crate) fn flush_pending_gui_notification(&self) {
        if self.pending_gui_notification.swap(false, Ordering::AcqRel) {
            self.notify_gui();
        }
    }

    pub(crate) fn notify_gui(&self) {
        let Some(notifier) = self.gui_notifier.lock().clone() else {
            // GUI が開いていなければ通知不要。
            return;
        };

        let payload = gain_payload(self.gain());
        // WebView channel は GUI runtime と同じ UI thread 上で扱う必要がある。
        // host / audio thread から直接 send すると native UI の thread affinity を
        // 破るので、いったん run loop に戻してから channel に渡す。
        notifier.sender.send(move || {
            let _ = notifier.channel.send(payload);
        });
    }
}

/// WebView へ送る JSON payload。GUI (TypeScript 側) はこの形を期待している。
pub(crate) fn gain_payload(gain: f32) -> serde_json::Value {
    json!({
        "type": "gain-state",
        "value": gain,
        "dbText": gain_db_text(gain as f64),
    })
}

/// gain を有効範囲に収める。外部から来た値はすべてこれを通してから使う。
pub(crate) fn clamp_gain(gain: f32) -> f32 {
    gain.clamp(MIN_GAIN, MAX_GAIN)
}

fn audio_port_type(channel_count: u32) -> AudioPortType {
    match channel_count {
        1 => AudioPortType::Mono,
        2 => AudioPortType::Stereo,
        _ => AudioPortType::Unspecified,
    }
}

/// host から渡された port 構成要求を解析し、受理可能なら新しい channel 数を返す。
///
/// このサンプルでは入出力が「対称な main port」のケースしか受け付けない。
/// sidechain のような非対称構成を受理してしまうと製品固有の routing 意味論が
/// 必要になり、汎用の gain サンプルでは正しく定義できないため。
fn resolve_audio_channel_count(
    current_channel_count: u32,
    requests: &[AudioPortConfigurationRequest],
) -> Option<u32> {
    let mut input_channel_count = current_channel_count;
    let mut output_channel_count = current_channel_count;
    for request in requests {
        if request.port_index != 0 {
            return None;
        }
        if !is_supported_audio_port_request(request) {
            return None;
        }
        if request.is_input {
            input_channel_count = request.channel_count;
        } else {
            output_channel_count = request.channel_count;
        }
    }

    // 入出力で channel 数が一致しているときだけ受理する。
    (input_channel_count == output_channel_count).then_some(input_channel_count)
}

fn is_supported_audio_port_request(request: &AudioPortConfigurationRequest) -> bool {
    matches!(
        (request.channel_count, request.port_type),
        (1, AudioPortType::Mono | AudioPortType::Unspecified)
            | (2, AudioPortType::Stereo | AudioPortType::Unspecified)
    )
}

/// 線形 amplitude を dB 表示の文字列に変換する。0 以下は "-inf dB"。
pub(crate) fn gain_db_text(gain: f64) -> String {
    if gain <= 0.0 {
        "-inf dB".to_string()
    } else {
        format!("{:.1} dB", 20.0 * gain.log10())
    }
}

/// WebView (フロントエンド) から呼べる command を登録する。
///
/// フロントエンド側 (`src-gui` の TypeScript) は `invoke("set_gain", { value })`
/// のような形でこれらの command を呼び出す。ここで登録した名前と引数は GUI 側
/// コードと約束事になっているので、変更するときは両側を揃える。
pub(crate) fn register_commands(
    command_handler: Rc<WxpCommandHandler>,
    shared: Arc<SharedState>,
    host_parameter_edit_notifier: Arc<dyn HostParameterEditNotifier>,
) {
    // 現在の gain 値を取得 (GUI 起動直後の初期表示などに使う)。
    {
        let shared = shared.clone();
        command_handler.register_sync("get_gain_state", move |_ctx| {
            Ok::<_, String>(gain_payload(shared.gain()))
        });
    }

    // slider に触れ始めたタイミング。host に「これから undo 単位」と伝える。
    {
        let host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        command_handler.register_sync("begin_parameter_gesture", move |_ctx| {
            host_parameter_edit_notifier.begin_edit(PARAM_GAIN_ID);
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    // slider が動いたタイミング。値を反映して host にも通知する。
    {
        let shared = shared.clone();
        let host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        command_handler.register_sync("set_gain", move |ctx| {
            let value = ctx.arg::<f64>("value").map_err(|e| e.to_string())?;
            let applied = shared.set_gain(value);
            shared.notify_gui();
            host_parameter_edit_notifier.update_edit(PARAM_GAIN_ID, applied as f64);
            Ok::<_, String>(gain_payload(applied))
        });
    }

    // slider から指を離したタイミング。undo 単位の終了を host に伝える。
    {
        let host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        command_handler.register_sync("end_parameter_gesture", move |_ctx| {
            host_parameter_edit_notifier.end_edit(PARAM_GAIN_ID);
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    // gain の変化を継続的に受け取るための subscription を開始する。
    // 引数の `channel` は JS 側で作った callback channel で、これに対して plugin が
    // 値の変化を push してくる仕組み。
    {
        let shared = shared.clone();
        command_handler.register_sync("subscribe_gain", move |ctx| {
            let channel = ctx.arg::<Channel>("channel").map_err(|e| e.to_string())?;
            // 登録直後に現在値を 1 度送って初期同期する。
            channel
                .send(gain_payload(shared.gain()))
                .map_err(|e| e.to_string())?;
            shared.set_gui_channel(channel);
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    // subscription を解除する。
    {
        let shared = shared.clone();
        command_handler.register_sync("unsubscribe_gain", move |_ctx| {
            shared.clear_gui_channel();
            Ok::<_, String>(json!({ "ok": true }))
        });
    }
}

#[cfg(test)]
mod tests {
    // 単体テスト例: `resolve_audio_channel_count` の対称性チェックなど、
    // host や CLAP runtime を立ち上げずに検証できるロジックをここに置く。

    use wrac_clap_adapter::{AudioPortConfigurationRequest, AudioPortType};

    use super::{SharedState, resolve_audio_channel_count};

    const fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn shared_state_is_send_sync() {
        assert_send_sync::<SharedState>();
    }

    #[test]
    fn accepts_matching_mono_configuration() {
        let requests = [
            AudioPortConfigurationRequest {
                is_input: true,
                port_index: 0,
                channel_count: 1,
                port_type: AudioPortType::Mono,
            },
            AudioPortConfigurationRequest {
                is_input: false,
                port_index: 0,
                channel_count: 1,
                port_type: AudioPortType::Mono,
            },
        ];

        assert_eq!(resolve_audio_channel_count(2, &requests), Some(1));
    }

    #[test]
    fn rejects_mismatched_input_output_configuration() {
        let requests = [
            AudioPortConfigurationRequest {
                is_input: true,
                port_index: 0,
                channel_count: 1,
                port_type: AudioPortType::Mono,
            },
            AudioPortConfigurationRequest {
                is_input: false,
                port_index: 0,
                channel_count: 2,
                port_type: AudioPortType::Stereo,
            },
        ];

        assert_eq!(resolve_audio_channel_count(2, &requests), None);
    }
}
