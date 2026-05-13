# wrac_clap_adapter

このテンプレートで使用する CLAP 向けの、防御的な adapter crate です。

CLAP の C ABI は `clap-sys` 経由で受け、製品 crate には Rust の trait と値型のみを公開します。VST3 / AU / AAX への変換は含みません。これらは `clap-wrapper` の責務で、本 crate は CLAP plugin および CLAP extension を Rust 側で実装するための境界に専念します。

## clack との違い

CLAP のヘッダには、各関数を呼び出してよい thread が `[main-thread]` `[audio-thread]` `[thread-safe]` といったコメントで示されています。例えば `init` は `[main-thread]`、`process` は `[audio-thread]`、`get_extension` は `[thread-safe]` です。

`clack` は host がこのコメント通りに関数を呼び出す前提で型を設計しており、Native CLAP host 向けには素直に動作します。

一方、本 crate は `clap-wrapper` 経由の VST3 / AU / AAX host も対象とします。これらの host を経由すると、`[main-thread]` 指定の query が別 thread から呼ばれるなど、コメント通りの呼び出し順・呼び出し thread にならない場合があります。本 crate はこれを adapter 側の lock と panic 捕捉で受け、製品コードに `unsafe` を露出させずに動作させることを目的としています。

## 謝辞

`wrac_clap_adapter` は、`clack` の safe で low-level な CLAP wrapper 設計、特に CLAP extension 境界と audio buffer access の考え方を参考にさせていただきました。本 crate は `clack` のコード派生ではなく、`clap-wrapper` 経由の配布で defensive に動作することを目的にした独立の `clap-sys` adapter です。

## Public API

- `PluginCore`: instance lifecycle と、サポートする extension の宣言
- `PluginAudioPorts`: CLAP `audio-ports`
- `PluginNotePorts`: CLAP `note-ports`
- `PluginParameters`: CLAP `params`
- `PluginStateSupport`: CLAP `state`
- `PluginGui`: CLAP `gui`
- `export_clap_plugin!`: CLAP entry point の export

各 trait は CLAP C ABI に対応する薄い Rust 表現です。独自の plugin framework としては設計しません。

## Thread Model

- FFI callback で発生した panic は C ABI の外へ伝播させない
- `PluginCore` は `RwLock` で保護する。ただし軽量な query / flush / state / configurable ports は lock を待たず、競合時は失敗値を返す
- `get_extension()` は instance 作成時に確定させた extension の集合のみを参照する
- 処理中であるかを示すフラグを adapter 独自に持たず、`Processor` の存在の有無を処理可能状態の唯一の根拠とする
- `start_processing()` / `stop_processing()` は audio 可否の根拠としない
- audio callback は `Processor` を直接呼び出し、`PluginCore` の lock を取得しない
- query 系 trait は `&self` 受けとし、任意の thread から並行に読める実装を要求する
- GUI callback は native UI lifecycle に触れ得るため、main thread への marshal はせず、adapter 側の `try_lock` guard で再入・並行呼び出しを失敗させる
- active 中に state restore が発生し得る前提で設計する

## Current Limits

Gain example のための最小実装です。

- `audio-ports` extension は製品実装が返す `count` / `info` をそのまま公開する
- MIDI typed event、instrument extension は未実装
- parameter rescan は未実装
- event batching helper は最小限
- audio ports activation extension は未実装
- 複数 plugin を 1 binary から出す構成は未対応

公開 framework ではないため、API の後方互換性は保証しません。
