//! WXP Example Gain 固有の WebView GUI runtime。
//!
//! GUI 本体は HTML/CSS/TypeScript で書かれており (`src-gui/` 以下)、
//! これを embed した WebView を host window に貼り付けるのがこの module の
//! 役目。WebView との通信は `wxp` crate の command/channel 機構を使い、
//! frontend から `set_gain` などの command を invoke できる。
//!
//! 役割分担:
//! - `wrac_wxp_gui`: host UI thread の所有、callback dispatch、parent window
//!   の raw handle 変換などの厄介な部分を引き受ける
//! - この module    : WebView の内容 (URL / 埋め込み zip)、register する
//!   command、resize/scale の挙動など、製品ごとに変わる部分だけを書く

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use run_loop_timer::Timer;
use wrac_clap_adapter::{
    GuiConfiguration, GuiSize, HostParameterEditNotifier, PluginError, PluginResult,
};
use wrac_wxp_gui::{DpiConverter, ParentWindowHandle, WxpGuiRuntime, gui_size_to_logical};
use wxp::{WebContext, WebViewRef, WxpCommandHandler, WxpWebViewBuilder, dpi::LogicalSize};

use crate::plugin::{SharedState, register_commands};

// resize 時にクランプする論理ピクセルの上下限。
const MIN_GUI_SIZE: LogicalSize<f64> = LogicalSize::new(280.0, 280.0);
const MAX_GUI_SIZE: LogicalSize<f64> = LogicalSize::new(720.0, 720.0);

// release build 時のみ、`build.rs` が作った frontend zip を埋め込む。
// debug build では Vite dev server (`http://127.0.0.1:5173/`) を見るので不要。
#[cfg(not(debug_assertions))]
const FRONTEND_ZIP: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/wxp_example_gain_plugin_gui.zip"));

/// GUI window 1 つに対応する runtime。host が GUI を開くたびに 1 つ作られ、
/// 閉じるときに drop される。
pub(crate) struct WxpExampleGainGuiRuntime {
    // 共有 state。値の更新通知や WebView channel の登録に使う。
    shared: Arc<SharedState>,
    // 表示中の WebView。Option にしてあるのは Drop の順序を制御するため。
    web_view: Option<WebViewRef>,
    // wxp の WebContext。WebView より長く生かしておく必要があるので保持する。
    wxp_context: Option<WebContext>,
    // frontend からの command を受け取って Rust 側関数を呼ぶ dispatcher。
    command_handler: Rc<WxpCommandHandler>,
    // automation 等で立った dirty flag を定期的に GUI に反映するための timer。
    gui_update_timer: Timer,
    // 現在の論理サイズ。
    gui_size: LogicalSize<f64>,
    // DPI スケール (1.0, 1.5, 2.0 など) を考慮した bounds 変換に使う。
    dpi_converter: DpiConverter,
}

impl WxpExampleGainGuiRuntime {
    /// host が「GUI を開いて」と要求してきたタイミングで `plugin.rs` の closure
    /// から呼ばれる factory。parent window に貼り付ける WebView を作って返す。
    pub(crate) fn create(
        shared: Arc<SharedState>,
        host_parameter_edit_notifier: Arc<dyn HostParameterEditNotifier>,
        configuration: GuiConfiguration,
        initial_size: GuiSize,
        parent: ParentWindowHandle,
    ) -> PluginResult<Self> {
        // このサンプルは embedded (parent に貼り付けるタイプ) しか対応していない。
        // floating window が必要な場合は別途実装する。
        if configuration.is_floating {
            return Err(PluginError::Message("unsupported GUI configuration"));
        }

        // WebView から呼べる command (set_gain など) を登録する。
        let command_handler = Rc::new(WxpCommandHandler::new());
        register_commands(
            command_handler.clone(),
            shared.clone(),
            host_parameter_edit_notifier,
        );

        // WebView の cache/cookie などを置く data directory。
        let data_dir = std::env::temp_dir().join("wxp-example-gain-plugin");
        std::fs::create_dir_all(&data_dir)
            .map_err(|_| PluginError::Message("failed to create GUI data directory"))?;

        let mut wxp_context = WebContext::new(data_dir);
        // 初期 scale は 1.0 とし、後で host から `set_scale` で書き換えられる。
        let dpi_converter = DpiConverter::new(1.0);
        let gui_size = gui_size_to_logical(initial_size);
        let bounds = dpi_converter.create_webview_bounds(gui_size);

        // debug build では Vite dev server を見るので、frontend を変更しても
        // native plugin の再 build が不要になり開発体験が良くなる。
        // release build では DAW 環境で外部 dev server に依存できないので、
        // `build.rs` で固めた zip を WebView に直接 serve させる。
        #[cfg(debug_assertions)]
        let builder = {
            let url = "http://127.0.0.1:5173/";
            WxpWebViewBuilder::new(&mut wxp_context)
                .with_command_handler(command_handler.clone())
                .with_devtools(cfg!(debug_assertions))
                .with_visible(true)
                .with_bounds(bounds)
                .with_url(url)
        };

        #[cfg(not(debug_assertions))]
        let builder = {
            let url = "wxp-plugin://localhost/";
            WxpWebViewBuilder::new(&mut wxp_context)
                .with_command_handler(command_handler.clone())
                .with_devtools(cfg!(debug_assertions))
                .with_visible(true)
                .with_bounds(bounds)
                // 埋め込み zip を `wxp-plugin://` scheme で配信する。
                .with_serve_zip("wxp-plugin", FRONTEND_ZIP)
                .map_err(|_| PluginError::Message("failed to serve GUI assets"))?
                .with_url(url)
        };

        // parent window 上に子として WebView を作る。これで host UI に埋め込まれる。
        let web_view = builder
            .build_as_child(&parent)
            .map_err(|_| PluginError::Message("failed to build webview"))?;

        // 33ms ≒ 30Hz で「automation で値が動いたか?」を確認し、必要なら GUI に反映する。
        //
        // 補足: CLAP には `request_callback()` で main thread に処理を戻す API も
        // あるが、clap-wrapper 経由で VST3/AU/AAX に流すと host ごとの dispatch
        // 実装に依存してしまう。host の癖で GUI だけ古い値を出し続ける問題を防ぐ
        // ため、GUI runtime 自身の run loop 上で timer を回して定期的に回収する。
        let gui_update_timer = Timer::new(Duration::from_millis(33), {
            let shared = shared.clone();
            move || {
                shared.flush_pending_gui_notification();
            }
        });
        gui_update_timer.start();

        Ok(Self {
            shared: shared.clone(),
            web_view: Some(web_view),
            wxp_context: Some(wxp_context),
            command_handler,
            gui_update_timer,
            gui_size,
            dpi_converter,
        })
    }
}

// host から呼ばれる resize / scale / size 取得などの操作を実装する trait。
impl WxpGuiRuntime for WxpExampleGainGuiRuntime {
    /// host が表示倍率 (HiDPI 等) を伝えてきたときに呼ばれる。
    fn set_scale(&mut self, scale: f64) -> PluginResult<()> {
        self.dpi_converter.set_scale(scale);
        Ok(())
    }

    /// host が window サイズを変えたときに呼ばれる。範囲を clamp してから WebView に反映する。
    fn set_size(&mut self, size: GuiSize) -> PluginResult<()> {
        let requested = LogicalSize::new(size.width as f64, size.height as f64);
        self.gui_size = LogicalSize::new(
            requested
                .width
                .clamp(MIN_GUI_SIZE.width, MAX_GUI_SIZE.width),
            requested
                .height
                .clamp(MIN_GUI_SIZE.height, MAX_GUI_SIZE.height),
        );

        if let Some(web_view) = &self.web_view {
            web_view
                .set_bounds(self.dpi_converter.create_webview_bounds(self.gui_size))
                .map_err(|_| PluginError::Message("failed to resize webview"))?;
        }
        Ok(())
    }
}

// host が GUI を閉じると runtime が drop される。
// drop 順を field 宣言順に任せず、明示的に切断 → WebView 破棄 → context 破棄の
// 順で進めることで、callback が解放後の object を触る事故を防ぐ。
impl Drop for WxpExampleGainGuiRuntime {
    fn drop(&mut self) {
        // GUI が消えるので、shared state からも channel を外しておく。
        self.shared.clear_gui_channel();
        // WebView → WebContext の順で drop。逆だと wry が context 不在で panic することがある。
        self.web_view = None;
        self.wxp_context = None;
        // `command_handler` と `gui_update_timer` は field drop に任せる。
        // 下記 2 行は「ここまで生かしたい」ことを明示するためのダミー read。
        let _ = Rc::strong_count(&self.command_handler);
        let _ = self.gui_update_timer.is_running();
    }
}
