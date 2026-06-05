# WRAC Template Code Review Checklist

> English version: [product-code-review-checklist.md](product-code-review-checklist.md)

この checklist は、この template から作られた product の code review で使います。compiler、CI、`cargo xtask validate` では確実に証明できず、reviewer が見落としやすい template-specific な risk だけを載せています。

## Realtime Store Boundaries

- **Review:** audio processor から到達可能な code が、project/editor state store、GUI notifier、host GUI/state handle、logging setup、その他の non-realtime service に誤って到達できないか。
  **Why:** この template は、realtime parameter state と project/editor state を意図的に分離しています。allocation guard が検出できる realtime risk は一部だけです。audio thread からの blocking lock、host callback、non-realtime service access までは検出できません。

## Saved State Compatibility

- **Review:** release 済みの `SavedState` schema を変更する場合に、古い DAW project や preset に対する migration test または compatibility test が書かれているか。
  **Why:** serialized state compatibility は、人間の review だけでは信頼性が足りません。現在の save/load test は最新 schema の round-trip を証明できますが、schema 変更後に古い serialized state が意図通り recall されることまでは自動的に証明しません。
