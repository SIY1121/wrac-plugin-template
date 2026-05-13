# wrac_wxp_gui

`wrac_wxp_gui` は `wrac_clap_adapter` の `PluginGui` と wxp WebView runtime を接続する helper crate です。

責務は 2 つです。1 つは CLAP の `clap_window_t` 由来の `ClapWindow` を `raw-window-handle` の型に変換して wxp に渡すこと、もう 1 つは特定の thread からしか操作できない WebView runtime を host UI thread の run loop 上に保持することです。CLAP C ABI、plugin descriptor、parameter / state / audio extension は扱いません。

## 境界

- `wrac_clap_adapter`: CLAP ABI と lock 管理の境界
- `wrac_wxp_gui`: WebView runtime の thread 管理と parent window 変換
- `src-plugin`: 製品固有の WebView 内容、command、parameter 更新

wrac_wxp_gui は `wrac_clap_adapter` と wxp を接続するための薄い実装です。公開 framework ではないため、API の後方互換性は保証しません。

## 前提

- `set_parent()` で UI thread を固定し、GUI runtime はその thread 上で `show()` 時に作る
- 1 process 内の host UI thread は単一とみなす
- 複数 UI thread を使う host は unsupported として失敗させる
- floating window はこの helper では扱わない
