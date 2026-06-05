# Production-Readiness Checks

> English version: [production-readiness-checks.md](production-readiness-checks.md)

`cargo xtask validate` は、指定された plugin format をビルドし、WRAC production-readiness checks を実行したあと、clap-validator、Steinberg VST3 validator、auval などの外部 format validator を実行します。WRAC check に違反がある場合は error として扱い、コマンドは non-zero exit code を返します。

WRAC production-readiness checks は、商用 plugin のための NovoNotes 独自の release policy check です。format specification そのものを検証する validator ではありません。この check set は小さく保ってください。実際に起きていない問題に対する check は追加しないでください。production-readiness check として妥当なのは、すでに観測された release、QA、host compatibility、support 上の実問題を防ぐものだけです。

コマンドは各 check を `pass`、`fail`、`disabled`、`skipped` としてログ出力します。CI log から、どの release policy check が評価されたかを確認できます。

## Check の disable

check は plugin crate の manifest で rule ID ごとに disable できます。disable する rule には、空ではない `reason` が必須です。

```toml
[package.metadata.wrac.validation.disabled_rules.fender-studio-pro-generic-editor-single-knob]
reason = "This product does not support Fender Studio Pro generic editor workflows."
```

未知の rule ID と空の reason は error です。

check を disable するのは、意図的な product decision がある場合だけにしてください。plugin がその check の release policy を満たすべき場合は、disable ではなく plugin を修正してください。

## Check の追加

新しい check の追加は、単なる code change ではなく release policy の変更です。PR を作る前に、author は以下を完了してください。

- **妥当性:** その check が、実際に起きた問題を扱っていることを確認する。仮説上の risk に対する check は追加しない。
- **重複回避:** 他の validator がすでに検出する check と重複させない。観測済みの問題が再現するにもかかわらず `cargo xtask validate` が通ってしまう場合だけ、新しい check を追加する。
- **Document:** この document の Check List に expectation、reason、error condition、fix を追加する。
- **Unit Test:** `pass`、`fail`、`disabled`、`skipped`、edge case を test する。
- **Manual Validate 必須:** unit test だけでは不十分です。必ず以下を実施する。
  - 実際の template plugin を意図的に壊し、`cargo xtask validate` が期待した rule ID と message で fail することを確認する。
  - plugin を元に戻し、コマンドがその check を `pass`、`disabled`、または `skipped` としてログ出力することを確認する。

## Check List

### `fender-studio-pro-generic-editor-single-knob`

**Expectation:** Fender Studio Pro generic editor workflow を support する production plugin は、visible な non-bypass parameter を 0 個、または 2 個以上 expose する。

**Reason:** Fender Studio Pro 8.0.3 の generic editor は、この shape では knob を表示できません。この rule では bypass parameter は knob 数に含めません。

**Error condition:** CLAP または VST3 validation が requested のとき、plugin が visible な non-bypass parameter をちょうど 1 個 expose している。

**Fix:** visible な non-bypass parameter を 0 個または 2 個以上にする。product が Fender Studio Pro generic editor workflow を意図的に support しない場合は、reason を書いて rule を disable する。

### `luna-vst3-param-id-must-match-index`

**Expectation:** VST3-compatible plugin は、public parameter ID を parameter list の index と一致させる。

**Reason:** LUNA 2.0.3.4381 では、VST3 parameter ID が parameter list index と異なる場合、VST3 automation write が失敗することがあります。

**Error condition:** VST3 validation が requested のとき、public parameter ID が parameter list index と異なる。

**Fix:** parameter を並べ替えるか public parameter ID を調整し、各 public parameter ID が index と一致するようにする。

### `bypass-param-shape`

**Expectation:** plugin は bypass parameter を最大 1 個 expose し、その parameter が boolean の host bypass control として振る舞う。

**Reason:** host bypass UI、bypass automation、generic editor、control surface は、bypass が boolean shape の parameter として 1 つ expose されているときに最も予測しやすく動作します。

**Error conditions:**

- bypass parameter が複数 expose されている。
- bypass parameter が stepped enum ではない。
- bypass parameter range が `0..1` ではない。
- bypass parameter default が `0` または `1` ではない。

**Fix:** bypass、stepped、enum flag を持ち、range `0..1`、default `0` または `1` の bypass parameter を 1 つ expose する。

### `plugin-requires-bypass`

**Expectation:** production plugin は valid な bypass parameter を 1 つ expose する。

**Reason:** host bypass UI、bypass automation、generic editor、control surface は、host-visible な bypass control が plugin から提供されることを期待する場合があります。valid な bypass parameter は実装コストが低く、plugin category を問わず host-specific compatibility risk を下げます。

**Error condition:** plugin が bypass parameter を expose していない。

**Fix:** bypass parameter を 1 つ追加する。product として host bypass を意図的に提供しない場合は、reason を書いて rule を disable する。

### `template-placeholders-renamed`

**Expectation:** template 由来の仮の名前、ID、URL を product 固有の値に置き換える。

**Reason:** これは、product metadata に template identity が残った setup failure が実際に観測されたための check です。仮の company name、plugin ID、plugin name、AU code、repository URL が残ると、host の scan cache、plugin menu、AU registration、log、support diagnostics に誤った product identity が出ます。この rule は template repository 自体では skipped されます。

**Error condition:** manifest metadata に `Your Company`、`com.your-company`、`WRAC Gain`、`wrac_gain_plugin`、`WtGn`、template repository URL などの template placeholder が残っている。

**Fix:** template 由来の metadata を product 固有の metadata に置き換える。template/example repository として意図的に残す場合は、reason を書いて rule を disable する。
