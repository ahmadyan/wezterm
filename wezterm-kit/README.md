# WezTermKit

`wezterm-kit` is an embeddable bridge layer for hosting WezTerm session logic inside another app.

This crate sits one layer above the existing `libwezterm` terminal-engine FFI in this fork:

- `libwezterm`: terminal state machine and screen model
- `wezterm-kit`: host-facing session lifecycle, PTY ownership, and callbacks

It is intentionally narrower than `wezterm-gui`:

- no window management
- no tab or pane chrome
- no hard dependency on WezTerm's frontend event loop
- a C ABI suitable for Swift/ObjC, C++, or other hosts

## Current scope

This first cut focuses on:

- local PTY session creation
- raw byte delivery to the host
- write and resize APIs
- callbacks for title, working directory, bell, and exit

## Extraction goal

The crate is structured so it can later move to a standalone repository with minimal churn:

- stable C-facing types live in `include/wezterm_kit.h`
- Rust internals stay behind an opaque `wezterm_kit_session_t`
- the host-facing API avoids exposing WezTerm-specific Rust types

## Non-goals for v1

- direct embedding of `wezterm-gui`
- GPU rendering APIs
- mux/tabs/splits
- Cocoa/AppKit types in the ABI

Those can be layered on later without changing the core session ABI.
