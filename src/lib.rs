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

/// Like [`scraper::Html::root_element`], but returns `None` instead of
/// panicking when the parsed tree contains no element at all (possible for
/// XML input that is only a `<?xml ...?>` declaration, comments, or PIs).
pub fn try_root_element(doc: &scraper::Html) -> Option<scraper::ElementRef<'_>> {
    doc.tree
        .root()
        .children()
        .find(|child| child.value().is_element())
        .and_then(scraper::ElementRef::wrap)
}
