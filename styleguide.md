# WRAC Product Implementation Style Guide

This guide is for product implementers and reviewers using this template as the
base for a commercial plugin.

`cargo xtask validate` covers deterministic production-readiness checks for built
artifacts. It does not replace implementation review. When using an AI reviewer,
ask it to read this document first and report findings with file/line evidence,
rule names, and concrete fixes.

## Review Scope

Treat the plugin as a host-loaded commercial product, not as an ordinary desktop
app. Review decisions should optimize for host compatibility, project recall,
automation safety, realtime safety, and supportability.

Prefer mechanical checks when a rule can be proven from the manifest or built
artifact. Use source review for cross-file consistency and design contracts that
are too contextual for a low-noise validator.

## Metadata And Identity

- Keep `plugins/*/src-plugin/Cargo.toml` `[package.metadata.wrac]` as the source
  of truth for product identity.
- Do not hard-code plugin IDs, names, company names, versions, or AUv2 codes in
  Rust, TypeScript, CMake, or generated bundle metadata when they can come from
  WRAC metadata.
- CLAP descriptors, macOS `Info.plist`, wrapper metadata, frontend About text,
  log names, and standalone identity should agree.
- After a product is published, public plugin IDs, AUv2 manufacturer/type/subtype
  codes, and public parameter IDs are stable compatibility identifiers.

## Parameters

- Every public parameter must have one stable ID, one host-visible name, a finite
  range, a finite default value inside that range, and consistent text conversion.
- Keep initial public parameter IDs dense and index-aligned unless there is a
  documented compatibility reason not to.
- When adding a parameter, update all relevant places together: constants,
  `param_count`, `param_info`, `param_value`, `apply_param_value`,
  `value_to_text`, `text_to_value`, shared state, save/restore, GUI payloads,
  frontend constants, and UI routing.
- GUI-originated parameter edits must update the product source of truth and send
  host automation notifications in a begin/update/end gesture.
- Bypass should be a single host-visible boolean-shaped bypass parameter unless
  the product intentionally disables the production-readiness rule with a reason.

## State And Project Recall

- Implement state save/load for production products. DAW project recall is a
  product requirement, not a convenience feature.
- Saved state formats must be backward compatible. New fields should have
  defaults or explicit migration logic.
- Parameter state and project/editor-only state may share one serialized payload,
  but audio-thread state should remain separate from non-realtime project locks.
- Notify the host when non-parameter project state changes and should be saved.

## Realtime Audio

- `Processor::process` and `Processor::reset` are realtime callbacks. They must
  not allocate, block, perform file or network I/O, call host GUI/state APIs, or
  wait on contended locks.
- Pass immutable snapshots or lock-free/atomic shared state into the processor at
  `activate`. Do not make the audio thread depend on lifecycle, GUI, project
  state, or layout locks.
- Clamp host-supplied event times and parameter values before using them.
- Support both `f32` and `f64` audio buffers when using the WRAC process buffer
  abstraction.
- Use debug assertions and allocation checks for realtime contracts, but do not
  treat them as replacements for release artifact validation.

## Capabilities And Threading

- Keep `PluginCore` lifecycle state separate from capability implementations.
  Capability callbacks can be queried re-entrantly or from wrapper-controlled
  threads.
- `PluginAudioPortsExtension` and `PluginNotePortsExtension` implementations must
  be thread-safe and avoid blocking, allocation, and contended locks.
- `PluginParamsExtension` queries must not depend on GUI runtime state or
  lifecycle locks.
- `PluginStateExtension` must be callable while the plugin is active.
- Host state dirty notifications belong on the GUI/main/control path, not on the
  audio thread.
- Host GUI resize requests should originate from GUI events, not background work
  or realtime callbacks.

## GUI And Frontend Contract

- Keep Rust command names and TypeScript `invoke(...)` calls in sync.
- Keep JSON payload keys and types in sync across Rust and TypeScript.
- In release builds, frontend assets must be embedded in the plugin. Debug server
  URLs should not be required for release artifacts.
- Debug frontend URLs should use `127.0.0.1` unless there is a tested reason to
  use a different host.
- GUI open, close, reopen, resize, scale, and host-focus behavior should be tested
  in standalone and representative DAWs.

## Validation Strategy

- Production-readiness checks should fail deterministically when a manifest or
  built artifact violates a low-noise release policy.
- Unit tests should cover pure logic such as parameter conversion, state
  migration, layout resolution, and GUI payload builders.
- Debug assertions should catch impossible states and realtime violations during
  development.
- Runtime guards should reject bad host input without panicking across FFI or
  corrupting project/audio state.
- AI review should focus on cross-file consistency, source-level thread contract
  violations, backward compatibility, and frontend/native command contracts.
- Real-host smoke tests are still required for workflows that depend on host
  behavior, such as DAW scanning, generic editors, automation writing, GUI resize,
  and project reload.
