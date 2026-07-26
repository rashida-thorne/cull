//! `--json-nodes`: dump matched elements as JSON node trees.
//!
//! The shape is deliberately stable and jq-friendly:
//!
//! ```json
//! {"tag": "a",
//!  "attrs": {"href": "https://example.com/x"},
//!  "text": "collapsed subtree text",
//!  "children": ["text node", {"tag": "b", ...}]}
//! ```
//!
//! - `attrs` is always an object (possibly empty); `href`/`src`/`action`
//!   style URL attributes are resolved against `--base` when given.
//! - `text` is the whole subtree's text, collapsed the same way `-j`
//!   values and CSV cells are (layout-aware; `script`/`style` excluded).
//! - `children` interleaves nested element objects with text-node
//!   strings (whitespace-only text nodes are dropped); comments and
//!   other non-content nodes are skipped.

use crate::text;
use scraper::{ElementRef, Node};
use serde_json::{Map, Value};

/// Serialize one matched element (and its subtree) to a JSON value.
pub fn element_to_json(el: ElementRef, base: Option<&str>) -> Value {
    let mut obj = Map::new();
    obj.insert("tag".into(), Value::String(el.value().name().to_string()));

    let mut attrs = Map::new();
    for (name, value) in el.value().attrs() {
        attrs.insert(
            name.to_string(),
            Value::String(text::maybe_resolve(name, value, base)),
        );
    }
    obj.insert("attrs".into(), Value::Object(attrs));
    obj.insert("text".into(), Value::String(text::collapsed_text(el)));

    let mut children = Vec::new();
    for child in el.children() {
        match child.value() {
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    children.push(element_to_json(child_el, base));
                }
            }
            Node::Text(t) => {
                let collapsed = text::collapse_ws(t);
                if !collapsed.is_empty() {
                    children.push(Value::String(collapsed));
                }
            }
            _ => {} // comments, PIs, doctypes
        }
    }
    obj.insert("children".into(), Value::Array(children));

    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{Html, Selector};

    fn dump(html: &str, sel: &str, base: Option<&str>) -> Value {
        let doc = Html::parse_fragment(html);
        let sel = Selector::parse(sel).unwrap();
        let el = doc.select(&sel).next().expect("no match");
        element_to_json(el, base)
    }

    #[test]
    fn basic_shape() {
        let v = dump(r#"<a href="/x" class="b">hi <b>there</b></a>"#, "a", None);
        assert_eq!(
            v,
            serde_json::json!({
                "tag": "a",
                "attrs": {"href": "/x", "class": "b"},
                "text": "hi there",
                "children": [
                    "hi",
                    {"tag": "b", "attrs": {}, "text": "there", "children": ["there"]}
                ]
            })
        );
    }

    #[test]
    fn attrs_object_always_present_and_key_order_kept() {
        let v = dump("<p>x</p>", "p", None);
        assert_eq!(v["attrs"], serde_json::json!({}));
        // Stable top-level key order (serde_json preserve_order).
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["tag", "attrs", "text", "children"]);
    }

    #[test]
    fn base_resolves_urls() {
        let v = dump(
            r#"<a href="/x">l</a>"#,
            "a",
            Some("https://example.com/dir/"),
        );
        assert_eq!(v["attrs"]["href"], "https://example.com/x");
    }

    #[test]
    fn whitespace_only_text_nodes_dropped() {
        let v = dump("<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>", "ul", None);
        let children = v["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().all(|c| c.is_object()));
    }

    #[test]
    fn comments_and_script_text_excluded() {
        let v = dump(
            "<div><!-- no --><script>var x=1;</script>visible</div>",
            "div",
            None,
        );
        assert_eq!(v["text"], "visible");
        let children = v["children"].as_array().unwrap();
        // script element still appears in the tree, comment does not
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["tag"], "script");
        assert_eq!(children[1], Value::String("visible".into()));
    }

    #[test]
    fn text_is_layout_aware() {
        let v = dump("<div>% of<br>world</div>", "div", None);
        assert_eq!(v["text"], "% of world");
    }
}
