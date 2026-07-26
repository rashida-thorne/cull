//! cull — like `jq`, but for HTML.
//!
//! This library crate exposes cull's core building blocks (selection,
//! shaped-JSON extraction, table extraction, Markdown conversion,
//! serialization, and charset decoding) so they can be reused outside the
//! CLI — e.g. by the WASM playground on the website. The CLI in `main.rs`
//! is the primary, supported interface; these APIs may change between
//! minor versions.

pub mod decode;
pub mod extract;
pub mod markdown;
pub mod nodes;
pub mod serialize;
pub mod table;
pub mod text;
pub mod xml;
