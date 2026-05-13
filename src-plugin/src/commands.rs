//! WebView frontend から呼べる command の登録。
//!
//! Rust 側から見ると、ここが TypeScript UI との約束事になる。command 名や payload
//! の形を変えるときは `src-gui` 側の `invoke(...)` / subscription と一緒に変更する。

use std::rc::Rc;
use std::sync::Arc;

use serde_json::json;
use wrac_clap_adapter::HostParameterEditNotifier;
use wxp::{Channel, WxpCommandHandler};

use crate::gui::{GuiStateNotifier, parameter_payload};
use crate::state::SharedState;

/// WebView (フロントエンド) から呼べる command を [`WxpCommandHandler`] に登録する。
///
/// フロントエンド側 (`src-gui` の TypeScript) は `invoke("set_parameter_value",
/// { parameterId, value })` のような形でこれらの command を呼び出す。
pub(crate) fn register_commands(
    command_handler: Rc<WxpCommandHandler>,
    shared: Arc<SharedState>,
    gui_notifier: Arc<GuiStateNotifier>,
    host_parameter_edit_notifier: Arc<dyn HostParameterEditNotifier>,
) {
    // 現在の parameter 値を取得する。GUI 起動直後の初期表示などに使う。
    {
        let shared = shared.clone();
        command_handler.register_sync("get_parameter_state", move |ctx| {
            let parameter_id = ctx.arg::<u32>("parameterId").map_err(|e| e.to_string())?;
            let value = shared
                .parameter_value(parameter_id)
                .ok_or_else(|| "invalid parameter id".to_string())?;
            Ok::<_, String>(parameter_payload(parameter_id, value))
        });
    }

    // control に触れ始めたタイミング。host に「これから undo 単位」と伝える。
    {
        let host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        command_handler.register_sync("begin_parameter_gesture", move |ctx| {
            let parameter_id = ctx.arg::<u32>("parameterId").map_err(|e| e.to_string())?;
            host_parameter_edit_notifier.begin_edit(parameter_id);
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    // control が動いたタイミング。値を反映して host にも通知する。
    {
        let shared = shared.clone();
        let gui_notifier = gui_notifier.clone();
        let host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        command_handler.register_sync("set_parameter_value", move |ctx| {
            let parameter_id = ctx.arg::<u32>("parameterId").map_err(|e| e.to_string())?;
            let value = ctx.arg::<f64>("value").map_err(|e| e.to_string())?;
            let applied = shared
                .set_parameter_value(parameter_id, value)
                .ok_or_else(|| "invalid parameter id".to_string())?;
            gui_notifier.notify_parameter(parameter_id, applied);
            host_parameter_edit_notifier.update_edit(parameter_id, applied as f64);
            Ok::<_, String>(parameter_payload(parameter_id, applied))
        });
    }

    // control から指を離したタイミング。undo 単位の終了を host に伝える。
    {
        let host_parameter_edit_notifier = host_parameter_edit_notifier.clone();
        command_handler.register_sync("end_parameter_gesture", move |ctx| {
            let parameter_id = ctx.arg::<u32>("parameterId").map_err(|e| e.to_string())?;
            host_parameter_edit_notifier.end_edit(parameter_id);
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    // parameter の変化を継続的に受け取るための subscription を開始する。
    // 引数の `channel` は JS 側で作った callback channel で、これに対して plugin が
    // 値の変化を push してくる仕組み。
    {
        let gui_notifier = gui_notifier.clone();
        command_handler.register_sync("subscribe_parameters", move |ctx| {
            let channel = ctx.arg::<Channel>("channel").map_err(|e| e.to_string())?;
            gui_notifier.set_channel(channel);
            Ok::<_, String>(json!({ "ok": true }))
        });
    }

    // subscription を解除する。
    {
        let gui_notifier = gui_notifier.clone();
        command_handler.register_sync("unsubscribe_parameters", move |_ctx| {
            gui_notifier.clear_channel();
            Ok::<_, String>(json!({ "ok": true }))
        });
    }
}
