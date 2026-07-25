//! HTML serialization for terminal output: optional ANSI color, optional
//! pretty-printing (indentation).
//!
//! Walks the parsed tree directly (rather than re-tokenizing serialized
//! HTML) so highlighting can never be confused by odd attribute values.

use ego_tree::NodeRef;
use scraper::{ElementRef, Node};
use std::fmt::Write;

/// ANSI escape sequences for one style role; empty strings when color is off.
struct Palette {
    reset: &'static str,
    dim: &'static str,
    tag: &'static str,
    attr_key: &'static str,
    attr_val: &'static str,
    comment: &'static str,
}

const COLOR_ON: Palette = Palette {
    reset: "\x1b[0m",
    dim: "\x1b[2m",
    tag: "\x1b[1;34m",     // bold blue: tag names
    attr_key: "\x1b[36m",  // cyan: attribute names
    attr_val: "\x1b[33m",  // yellow: attribute values
    comment: "\x1b[2;32m", // dim green: comments & doctype
};

const COLOR_OFF: Palette = Palette {
    reset: "",
    dim: "",
    tag: "",
    attr_key: "",
    attr_val: "",
    comment: "",
};

fn palette(color: bool) -> &'static Palette {
    if color { &COLOR_ON } else { &COLOR_OFF }
}

/// Elements with no closing tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose text children are serialized raw (no entity escaping).
const RAW_TEXT: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];

/// Elements whose contents are whitespace-sensitive: never reformat inside.
const PRESERVE: &[&str] = &["pre", "textarea"];

/// Serialize the outer HTML of `el` with ANSI colors, unformatted.
pub fn element_to_colored_html(el: ElementRef) -> String {
    let mut out = String::new();
    write_node(*el, &mut out, false, &COLOR_ON);
    out
}

/// Serialize the outer HTML of `el` indented (2 spaces per level),
/// optionally colored. Contents of `pre`, `textarea`, `script`, and
/// `style` are left verbatim.
pub fn element_to_pretty_html(el: ElementRef, color: bool) -> String {
    let mut out = String::new();
    write_pretty(*el, &mut out, 0, palette(color));
    // Drop the trailing newline; the caller adds one per match.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn open_tag(e: &scraper::node::Element, out: &mut String, p: &Palette) {
    let name = e.name();
    let _ = write!(out, "{}<{}{}{}{}", p.dim, p.reset, p.tag, name, p.reset);
    for (key, value) in e.attrs() {
        let _ = write!(
            out,
            " {}{}{}{}=\"{}{}{}{}{}\"{}",
            p.attr_key,
            key,
            p.reset,
            p.dim,
            p.reset,
            p.attr_val,
            escape_attr(value),
            p.reset,
            p.dim,
            p.reset
        );
    }
    let _ = write!(out, "{}>{}", p.dim, p.reset);
}

fn close_tag(name: &str, out: &mut String, p: &Palette) {
    let _ = write!(
        out,
        "{}</{}{}{}{}{}>{}",
        p.dim, p.reset, p.tag, name, p.reset, p.dim, p.reset
    );
}

fn write_node(node: NodeRef<Node>, out: &mut String, raw_text: bool, p: &Palette) {
    match node.value() {
        Node::Element(e) => {
            let name = e.name();
            open_tag(e, out, p);
            if VOID.contains(&name) {
                return;
            }
            let raw = RAW_TEXT.contains(&name);
            for child in node.children() {
                write_node(child, out, raw, p);
            }
            close_tag(name, out, p);
        }
        Node::Text(t) => {
            if raw_text {
                out.push_str(t);
            } else {
                out.push_str(&escape_text(t));
            }
        }
        Node::Comment(c) => {
            let _ = write!(out, "{}<!--{}-->{}", p.comment, c.comment, p.reset);
        }
        Node::Doctype(d) => {
            let _ = write!(out, "{}<!DOCTYPE {}>{}", p.comment, d.name(), p.reset);
        }
        Node::Document | Node::Fragment => {
            for child in node.children() {
                write_node(child, out, raw_text, p);
            }
        }
        Node::ProcessingInstruction(_) => {}
    }
}

/// True if the node contributes nothing visible (whitespace-only text).
fn insignificant(node: &NodeRef<Node>) -> bool {
    match node.value() {
        Node::Text(t) => t.trim().is_empty(),
        Node::ProcessingInstruction(_) => true,
        _ => false,
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn write_pretty(node: NodeRef<Node>, out: &mut String, depth: usize, p: &Palette) {
    match node.value() {
        Node::Element(e) => {
            let name = e.name();
            // Whitespace-sensitive or raw-text subtrees: one indented line,
            // contents verbatim (adding newlines inside <script> could even
            // change JS semantics via ASI).
            if PRESERVE.contains(&name) || RAW_TEXT.contains(&name) {
                indent(out, depth);
                write_node(node, out, false, p);
                out.push('\n');
                return;
            }
            if VOID.contains(&name) {
                indent(out, depth);
                open_tag(e, out, p);
                out.push('\n');
                return;
            }
            let kids: Vec<_> = node.children().filter(|c| !insignificant(c)).collect();
            let text_only = kids.iter().all(|c| matches!(c.value(), Node::Text(_)));
            indent(out, depth);
            open_tag(e, out, p);
            if kids.is_empty() {
                close_tag(name, out, p);
                out.push('\n');
            } else if text_only {
                let text: String = kids
                    .iter()
                    .map(|c| match c.value() {
                        Node::Text(t) => collapse_ws(&escape_text(t)),
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(&text);
                close_tag(name, out, p);
                out.push('\n');
            } else {
                out.push('\n');
                for child in kids {
                    write_pretty(child, out, depth + 1, p);
                }
                indent(out, depth);
                close_tag(name, out, p);
                out.push('\n');
            }
        }
        Node::Text(t) => {
            let collapsed = collapse_ws(&escape_text(t));
            if !collapsed.is_empty() {
                indent(out, depth);
                out.push_str(&collapsed);
                out.push('\n');
            }
        }
        Node::Comment(c) => {
            indent(out, depth);
            let _ = write!(out, "{}<!--{}-->{}", p.comment, c.comment, p.reset);
            out.push('\n');
        }
        Node::Doctype(d) => {
            indent(out, depth);
            let _ = write!(out, "{}<!DOCTYPE {}>{}", p.comment, d.name(), p.reset);
            out.push('\n');
        }
        Node::Document | Node::Fragment => {
            for child in node.children() {
                write_pretty(child, out, depth, p);
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

    fn first(html: &str, sel: &str) -> (Html, Selector) {
        (Html::parse_document(html), Selector::parse(sel).unwrap())
    }

    fn colored(html: &str, sel: &str) -> String {
        let (doc, sel) = first(html, sel);
        let el = doc.select(&sel).next().unwrap();
        element_to_colored_html(el)
    }

    fn pretty(html: &str, sel: &str) -> String {
        let (doc, sel) = first(html, sel);
        let el = doc.select(&sel).next().unwrap();
        element_to_pretty_html(el, false)
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

    // ---- pretty ----

    #[test]
    fn pretty_indents_nested_elements() {
        let p = pretty(
            r#"<div id="x"><ul><li>one</li><li>two</li></ul></div>"#,
            "div",
        );
        assert_eq!(
            p,
            "<div id=\"x\">\n  <ul>\n    <li>one</li>\n    <li>two</li>\n  </ul>\n</div>"
        );
    }

    #[test]
    fn pretty_text_only_element_stays_on_one_line() {
        assert_eq!(pretty("<h1>  A   Title </h1>", "h1"), "<h1>A Title</h1>");
    }

    #[test]
    fn pretty_empty_element_one_line() {
        assert_eq!(
            pretty("<div><span></span></div>", "div"),
            "<div>\n  <span></span>\n</div>"
        );
    }

    #[test]
    fn pretty_void_element_no_close() {
        assert_eq!(
            pretty(r#"<div><img src="i.png"></div>"#, "div"),
            "<div>\n  <img src=\"i.png\">\n</div>"
        );
    }

    #[test]
    fn pretty_preserves_pre_verbatim() {
        let p = pretty("<div><pre>a\n  b\nc</pre></div>", "div");
        assert_eq!(p, "<div>\n  <pre>a\n  b\nc</pre>\n</div>");
    }

    #[test]
    fn pretty_preserves_script_verbatim() {
        let p = pretty(
            "<div><script>let a = 1\nlet b = a < 2 && a</script></div>",
            "div",
        );
        assert!(p.contains("<script>let a = 1\nlet b = a < 2 && a</script>"));
    }

    #[test]
    fn pretty_skips_whitespace_only_text() {
        let p = pretty("<div>\n   <p>hi</p>\n   </div>", "div");
        assert_eq!(p, "<div>\n  <p>hi</p>\n</div>");
    }

    #[test]
    fn pretty_mixed_content_each_on_own_line() {
        let p = pretty("<p>before <b>bold</b> after</p>", "p");
        assert_eq!(p, "<p>\n  before\n  <b>bold</b>\n  after\n</p>");
    }

    #[test]
    fn pretty_escapes_text_and_attrs() {
        let p = pretty(r#"<p title="a&quot;b">1 &lt; 2</p>"#, "p");
        assert_eq!(p, "<p title=\"a&quot;b\">1 &lt; 2</p>");
    }

    #[test]
    fn pretty_colored_strips_to_plain_pretty() {
        let html = r#"<div id="x"><p>hi <b>there</b></p><pre>a
 b</pre></div>"#;
        let doc = Html::parse_document(html);
        let sel = Selector::parse("div").unwrap();
        let el = doc.select(&sel).next().unwrap();
        let plain = element_to_pretty_html(el, false);
        let colored = element_to_pretty_html(el, true);
        assert_eq!(strip_ansi(&colored), plain);
        assert!(colored.contains("\x1b[1;34m"));
    }

    #[test]
    fn pretty_comment_and_doctype() {
        let p = pretty("<div><!-- note --><p>x</p></div>", "div");
        assert_eq!(p, "<div>\n  <!-- note -->\n  <p>x</p>\n</div>");
    }
}
