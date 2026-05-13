# xtask

Repository-local build commands for the WRAC plugin template.

```bash
cargo xtask build
cargo xtask build --release
cargo xtask build --validate
cargo xtask build --target=vst3
cargo xtask build --target=au,standalone
cargo xtask install
cargo xtask validate
```

`build` creates every target supported by the current OS: CLAP/VST3/AU/standalone
on macOS, CLAP/VST3/standalone on Windows, and CLAP on Linux. Use `--target`
with a comma-separated list of `clap`, `vst3`, `au`, and `standalone` for
explicit target selection.

Generated artifacts use stable paths under `target/wrac` so downstream tooling
does not need to search previous build outputs. On macOS, `validate` runs
Steinberg's VST3 validator and `auval -v aufx WtGn YrCo`.
