# AAX Build and Validation

> English version: [aax.md](aax.md)

AAX は macOS / Windows で、`package.metadata.wrac.supported_formats` に
`aax` が含まれている場合に対応します。既定の build / install / validate は
この metadata list を使います。`--target=aax` は AAX だけを要求する指定です。
`supported_formats` に `aax` が含まれていない場合、明示的な AAX target 指定は失敗します。

## 前提条件

- 展開済みの AAX SDK
- AAX Validator/DSH archive、または展開済みの Validator/DSH directory
- CMake と clap-wrapper が対応する platform C++ toolchain

展開した AAX SDK root directory を `AAX_SDK_ROOT` に指定してください。

```sh
export AAX_SDK_ROOT=/path/to/aax-sdk-2-9-0
```

validation には、以下のどちらかを指定してください。

```sh
export AAX_VALIDATOR_DSH_ROOT=/path/to/aax-validator-dsh
# または
export AAX_VALIDATOR_DSH_ARCHIVE=/path/to/aax-validator-dsh-2024-6-0-138bab0d-mac-arm64.tar.gz
```

どちらの Validator/DSH 変数も未設定の場合、`xtask` は `~/Downloads` 以下の標準的な
download file name も確認します。

## Metadata

AAX の identity は、plugin package manifest の `[package.metadata.wrac]` から生成されます。
出荷前に、少なくとも以下を製品固有の値に置き換えてください。

- `aax_manufacturer_id`
- `aax_product_id`
- `aax_categories`
- `aax_stem_configs`
- `supported_formats = ["clap", "vst3", "au", "aax"]`

各 AAX stem config には一意な `plugin_id` が必要です。manufacturer、product、stem
plugin ID は host の plugin identity と project recall に使われるため、release 後は変更しないでください。

## Build

```sh
cargo xtask build --target=aax
# supported_formats に書かれた全 format を build する場合:
cargo xtask build
```

`cargo xtask build --target=aax --dry-run` を使うと、SDK の確認や artifact 生成を行わずに
task graph と依存順を確認できます。

## Install

macOS / Windows の AAX plugin は system-wide の Avid plugin folder にインストールします。
`--scope=default` では、AAX は system-wide、CLAP/VST3/AU は user-local にインストールします。

```sh
cargo xtask install --target=aax
```

`--scope=user` は AAX では無効です。system-wide を明示したい場合は `--scope=system` を使ってください。

`cargo xtask uninstall` の default は `--scope=all` です。AAX には user-local
install scope がないため、AAX uninstall の `all` は system-wide の Avid plugin
folder に解決されます。

## Validate

```sh
cargo xtask validate --target=aax
# supported_formats に書かれた全 format を validate する場合:
cargo xtask validate
```

validator は test ID ごとに選択した AAX Validator test を実行し、公式 JSON result を
`target/wrac-plugins/<package>/wrac/validation/aax/` に保存します。`xtask` は macOS /
Windows とも Avid package に同梱されている DTT runner を使います。DTT は DigiShell の
documented automation layer であり、local shell と hosted CI の両方で挙動をそろえやすいためです。
選択された test の `result_status` が pass 以外なら `xtask` は失敗します。
`--continue-on-error` を指定した場合、AAX validation が失敗しても最終 exit status は non-zero
のままですが、独立した package / format task は続行します。失敗 task に依存する AAX downstream task
は skip されます。

ローカル validation target では、以下を意図的に skip します。

- `test.cycle_counts`: DSP/HDX cycle-count validation であり、この native local build
  target の範囲外です。
- `test.page_table.load`: このテンプレートは page-table XML を生成しません。

署名、notarization、Pace wrapping、installer 作成、製品配布はこのテンプレートの build scope 外です。
