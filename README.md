# WRAC Plugin Template

A template for implementing audio plugins with the WRAC stack.
You can copy this repository as a starting point for new projects.

> 日本語版: [README_JA.md](README_JA.md)

# What is the WRAC Stack?

The WRAC stack is a technology stack for audio plugin development, built around three core components: **Webview, Rust Audio, and CLAP**.

**W** (WebView): User interface implementation using HTML/CSS/JS.

**RA** (Rust Audio): Audio signal processing implementation in Rust.

**C** (CLAP): Interface with host applications via the CLever Audio Plug-in standard.


## Contents

The code in this repository implements a simple plugin called WRAC Gain.
It is also structured so it can be used as a template.

- CLAP plugin implementation in Rust using [clap-sys](https://github.com/micahrj/clap-sys)
- WebView GUI implementation using [wxp](https://github.com/novonotes/wxp)
- VST3 / AU / Standalone builds via [clap-wrapper](https://github.com/free-audio/clap-wrapper)

## Build

Common commands:

```bash
# Debug build for all formats
cargo xtask build
# Release build for all formats
cargo xtask build --release
# Debug build for VST3 only
cargo xtask build --target=vst3
# Release build for AU and Standalone
cargo xtask build --target=au,standalone --release
# Validate built plugins
cargo xtask validate
# Install built plugins
cargo xtask install
```

Launch the standalone app after building it:

```bash
cargo xtask build --target=standalone
cargo xtask launch
```

Supported formats:

| OS | `cargo xtask build` targets | `cargo xtask validate` targets |
|----|-----------------------------|-------------------------------|
| macOS | CLAP / VST3 / AU / Standalone | CLAP / VST3 / AU |
| Windows | CLAP / VST3 / Standalone | CLAP / VST3 |
| Linux | CLAP / VST3 / Standalone | CLAP / VST3 |

The `--target` option accepts `clap`, `vst3`, `au`, and `standalone` as comma-separated values.

For detailed usage:

```bash
# Overall help
cargo xtask --help
# Subcommand help
cargo xtask build --help
```

## Setting Up a New Project

To create a new wxp plugin based on this repository, see [Setup](docs/setup.md).


## Give it a spin?

This template comes with a simple Gain plugin pre-implemented. Try loading it up and let us know how it works in your DAW!
Even a quick comment like **"Works on Logic Pro 10.7"** is incredibly helpful for the community.

Feel free to drop a quick note here:
👉 [DAW Compatibility Reports](https://github.com/novonotes/wrac-plugin-template/discussions/6)

## Notes

This repository is intended as an implementation example and starting point, not a general-purpose framework. Future changes will not provide API backwards compatibility or migration support.

## Reference

For known DAW compatibility status, see the [DAW Compatibility Matrix](https://github.com/novonotes/wrac-plugin-template/wiki/DAW-Compatibility-Matrix).

For usage of the wxp crate, see the [wxp README](https://github.com/novonotes/wxp/tree/main/crates/wxp).
