//! ANSI-colored HTML serialization for terminal output.
//!
//! Walks the parsed tree directly (rather than re-tokenizing serialized
//! HTML) so highlighting can never be confused by odd attribute values.

use ego_tree::NodeRef;
use scraper::{ElementRef, Node};
use std::fmt::Write;

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const TAG: &str = "\x1b[1;34m"; // bold blue: tag names
const ATTR_KEY: &str = "\x1b[36m"; // cyan: attribute names
const ATTR_VAL: &str = "\x1b[33m"; // yellow: attribute values
const COMMENT: &str = "\x1b[2;32m"; // dim green: comments & doctype

/// Elements with no closing tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose text children are serialized raw (no entity escaping).
const RAW_TEXT: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];

/// Serialize the outer HTML of `el` with ANSI colors.
pub fn element_to_colored_html(el: ElementRef) -> String {
    let mut out = String::new();
    write_node(*el, &mut out, false);
    out
}

fn write_node(node: NodeRef<Node>, out: &mut String, raw_text: bool) {
    match node.value() {
        Node::Element(e) => {
            let name = e.name();
            let _ = write!(out, "{DIM}<{RESET}{TAG}{name}{RESET}");
            for (key, value) in e.attrs() {
                let _ = write!(
                    out,
                    " {ATTR_KEY}{key}{RESET}{DIM}=\"{RESET}{ATTR_VAL}{}{RESET}{DIM}\"{RESET}",
                    escape_attr(value)
                );
            }
            let _ = write!(out, "{DIM}>{RESET}");
            if VOID.contains(&name) {
                return;
            }
            let raw = RAW_TEXT.contains(&name);
            for child in node.children() {
                write_node(child, out, raw);
            }
            let _ = write!(out, "{DIM}</{RESET}{TAG}{name}{RESET}{DIM}>{RESET}");
        }
        Node::Text(t) => {
            if raw_text {
                out.push_str(t);
            } else {
                out.push_str(&escape_text(t));
            }
        }
        Node::Comment(c) => {
            let _ = write!(out, "{COMMENT}<!--{}-->{RESET}", c.comment);
        }
        Node::Doctype(d) => {
            let _ = write!(out, "{COMMENT}<!DOCTYPE {}>{RESET}", d.name());
        }
        Node::Document | Node::Fragment => {
            for child in node.children() {
                write_node(child, out, raw_text);
            }
        }
        Node::ProcessingInstruction(_) => {}
    }
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{Html, Selector};

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c2 in chars.by_ref() {
                    if c2 == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn colored(html: &str, sel: &str) -> String {
        let doc = Html::parse_document(html);
        let sel = Selector::parse(sel).unwrap();
        let el = doc.select(&sel).next().unwrap();
        element_to_colored_html(el)
    }

    #[test]
    fn stripped_output_matches_plain_serialization() {
        let html =
            r#"<div id="x" class="a b"><p>hi &amp; <b>bye</b></p><br><img src="i.png"></div>"#;
        let doc = Html::parse_document(html);
        let sel = Selector::parse("div").unwrap();
        let el = doc.select(&sel).next().unwrap();
        assert_eq!(strip_ansi(&element_to_colored_html(el)), el.html());
    }

    #[test]
    fn contains_ansi_codes() {
        let c = colored("<p class=\"x\">hi</p>", "p");
        assert!(c.contains("\x1b[1;34mp"));
        assert!(c.contains("\x1b[36mclass"));
        assert!(c.contains("\x1b[33mx"));
    }

    #[test]
    fn script_text_not_escaped() {
        let c = colored("<script>if (a && b < 3) {}</script>", "script");
        assert!(strip_ansi(&c).contains("a && b < 3"));
    }

    #[test]
    fn void_elements_no_close_tag() {
        let c = strip_ansi(&colored("<p>a<br>b</p>", "p"));
        assert_eq!(c, "<p>a<br>b</p>");
    }

    #[test]
    fn comments_colored() {
        let c = colored("<div><!-- note --></div>", "div");
        assert!(c.contains("<!-- note -->"));
        assert!(c.contains("\x1b[2;32m"));
    }
}
