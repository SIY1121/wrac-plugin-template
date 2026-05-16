//! WRAC Gain plugin — the entry-point crate for this template.
//!
//! A minimal gain (volume) plugin. `src-plugin` contains only product-specific logic
//! (parameters, state, DSP, GUI); the messy CLAP ABI and FFI invariants are encapsulated
//! in the separate `wrac_clap_adapter` crate. When building a plugin from this template,
//! you'll mostly be editing the files in this crate.
//!
//! File layout:
//! - `plugin.rs`   : the plugin contract as seen by the host; details live under `plugin/`.
//! - `state.rs`    : lock-free state shared by the audio thread, GUI, and host.
//! - `audio.rs`    : DSP running on the audio thread (just applies gain in this sample).
//! - `gui.rs`      : WebView-based GUI integration; runtime/notifier live under `gui/`.
//! - `commands.rs` : Rust commands callable from the WebView frontend; resize helpers under `commands/`.
//!
//! Logging goes through the `log` facade. `logging.rs` provides a simple logger for debug
//! builds; production plugins are expected to replace it with a custom logger.

// In debug builds, swap in a custom allocator to detect allocations on the audio
// thread immediately (see process() in audio.rs for usage).
#[cfg(debug_assertions)]
use assert_no_alloc::*;

#[cfg(debug_assertions)]
#[global_allocator]
static ALLOC_DISABLER: AllocDisabler = AllocDisabler;

mod audio;
mod commands;
mod gui;
mod logging;
mod plugin;
mod state;

// Export the CLAP entry point. The adapter generates the C ABI, factory, and lifecycle
// machinery; here we only supply "what this plugin is" (descriptor) and "how to create
// the core" (create).
wrac_clap_adapter::export_clap_plugin! {
    descriptor: crate::plugin::PLUGIN_DESCRIPTOR,
    create: crate::plugin::create_plugin_core,
}
