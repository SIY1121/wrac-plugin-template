# clap_wrapper_builder

`clap_wrapper_builder` is a helper CMake build environment for wrapping a CLAP
plugin static library into VST3 / AUv2 / AAX / Standalone.

The dependency SDKs are normally checked out as git submodules under this
directory. Repository-local product builds are driven by `cargo xtask`; this
directory intentionally keeps the reusable CMake wrapper definition for other
products.

## Contents

- `CMakeLists.txt` - Static-library based wrapper build definition
- `clap-wrapper` / `clap` / `vst3sdk` / `AudioUnitSDK` - Dependency SDKs / toolchain

## Usage

The main template build uses:

```bash
cargo xtask build
cargo xtask build --target=standalone
```

Other products can call this CMake project directly and pass
`CLAP_WRAPPER_BUILDER_TARGET_LIB`, `CLAP_WRAPPER_BUILDER_OUTPUT_NAME`, and
`CLAP_WRAPPER_BUILDER_STAGE_DIR`.

## AAX

AAX support remains available in CMake for products that need it, but the WRAC
Gain `cargo xtask` build disables AAX. AAX requires the Avid SDK. Point
`AAX_SDK_ROOT` at a local SDK checkout, or enable dependency download when that
is acceptable for your environment.
