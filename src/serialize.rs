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

/// Elements that render inline: pretty-printing keeps them on the same
/// line as their surrounding text, because inserting a line break (which
/// is whitespace) where the source had none would change what a browser
/// renders — e.g. `<b>a</b><i>b</i>` is "ab" but `<b>a</b>\n<i>b</i>` is
/// "a b". `script`/`style` are included: they render nothing, so breaking
/// around them can silently add a space between the texts they separate.
/// (SVG/MathML elements are not in the HTML namespace and stay on their
/// own line; a rare space may appear next to them.)
const INLINE: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "br", "button", "cite", "code", "data", "del", "dfn", "em",
    "i", "img", "input", "ins", "kbd", "label", "mark", "meter", "noscript", "object", "output",
    "picture", "progress", "q", "rp", "rt", "ruby", "s", "samp", "script", "select", "slot",
    "small", "span", "strong", "style", "sub", "sup", "textarea", "time", "u", "var", "wbr",
];

/// True if this element is in the HTML namespace. Void/raw-text/preserve
/// rules are HTML-only: an XML `<link>` or `<title>` (e.g. in an RSS feed,
/// parsed with `--xml`) is an ordinary element with children.
pub(crate) fn is_html(e: &scraper::node::Element) -> bool {
    &*e.name.ns == "http://www.w3.org/1999/xhtml"
}

/// Serialize the outer HTML of `el` with ANSI colors, unformatted.
pub fn element_to_colored_html(el: ElementRef) -> String {
    let mut out = String::new();
    write_node(*el, &mut out, false, &COLOR_ON);
    out
}

/// Serialize the inner HTML of `el` (children only), optionally colored,
/// unformatted. Children of raw-text elements are emitted unescaped.
pub fn element_to_inner_html(el: ElementRef, color: bool) -> String {
    let mut out = String::new();
    let raw = is_html(el.value()) && RAW_TEXT.contains(&el.value().name());
    for child in el.children() {
        write_node(child, &mut out, raw, palette(color));
    }
    out
}

/// Serialize the inner HTML of `el` indented (2 spaces per level),
/// optionally colored.
pub fn element_to_pretty_inner_html(el: ElementRef, color: bool) -> String {
    let name = el.value().name();
    // Whitespace-sensitive contents are never reformatted.
    if is_html(el.value()) && (PRESERVE.contains(&name) || RAW_TEXT.contains(&name)) {
        return element_to_inner_html(el, color);
    }
    let p = palette(color);
    let mut out = String::new();
    for group in group_children(*el, p) {
        match group {
            Group::Run(s) => {
                out.push_str(&s);
                out.push('\n');
            }
            Group::Block(child) => write_pretty(child, &mut out, 0, p),
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Serialize the outer HTML of `el` indented (2 spaces per level),
/// optionally colored. Contents of `pre`, `textarea`, `script`, and
/// `style` are left verbatim, and inline elements stay on the same line
/// as their surrounding text so re-rendering the output is faithful.
pub fn element_to_pretty_html(el: ElementRef, color: bool) -> String {
    let mut out = String::new();
    write_pretty(*el, &mut out, 0, palette(color));
    // Drop the trailing newline; the caller adds one per match.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Serialize the whole document — DOCTYPE, top-level comments, and the
/// root element — unformatted, optionally colored. `el` is the root
/// element; its parent (the document node) is serialized when present.
pub fn document_to_html(el: ElementRef, color: bool) -> String {
    let mut out = String::new();
    match el.parent() {
        Some(doc) if matches!(doc.value(), Node::Document | Node::Fragment) => {
            write_node(doc, &mut out, false, palette(color));
        }
        _ => write_node(*el, &mut out, false, palette(color)),
    }
    out
}

/// Pretty-printed variant of [`document_to_html`]: DOCTYPE and top-level
/// comments each on their own line, then the indented root element.
pub fn document_to_pretty_html(el: ElementRef, color: bool) -> String {
    let mut out = String::new();
    match el.parent() {
        Some(doc) if matches!(doc.value(), Node::Document | Node::Fragment) => {
            write_pretty(doc, &mut out, 0, palette(color));
        }
        _ => write_pretty(*el, &mut out, 0, palette(color)),
    }
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
            if is_html(e) && VOID.contains(&name) {
                return;
            }
            let raw = is_html(e) && RAW_TEXT.contains(&name);
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

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Elements that render nothing at all (metadata & friends): they can
/// appear mid-sentence (e.g. Wikipedia emits `<link>` inside paragraphs),
/// so breaking a line around them would inject a visible space.
const RENDERLESS: &[&str] = &[
    "area", "base", "datalist", "link", "meta", "param", "source", "template", "track",
];

/// True if this node belongs to an inline "run" when pretty-printing:
/// text, comments (they render nothing, so a line break around them could
/// add a visible space), and HTML inline elements.
fn is_run_member(node: &NodeRef<Node>) -> bool {
    match node.value() {
        Node::Text(_) | Node::Comment(_) => true,
        Node::Element(e) => {
            is_html(e) && (INLINE.contains(&e.name()) || RENDERLESS.contains(&e.name()))
        }
        _ => false,
    }
}

/// Append text collapsed to single internal spaces, preserving whether
/// leading/trailing whitespace existed (a single space each). The leading
/// space is skipped if `out` already ends with one.
fn push_collapsed(t: &str, out: &mut String) {
    let lead = t.starts_with(|c: char| c.is_whitespace());
    let trail = t.ends_with(|c: char| c.is_whitespace());
    let core = collapse_ws(&escape_text(t));
    if core.is_empty() {
        if (lead || trail) && !out.ends_with(' ') {
            out.push(' ');
        }
        return;
    }
    if lead && !out.ends_with(' ') {
        out.push(' ');
    }
    out.push_str(&core);
    if trail {
        out.push(' ');
    }
}

/// Serialize one run member on the current line: text whitespace is
/// collapsed (presence-preserving), PRESERVE/RAW_TEXT subtrees verbatim.
fn write_inline(node: NodeRef<Node>, out: &mut String, p: &Palette) {
    match node.value() {
        Node::Element(e) => {
            let name = e.name();
            if is_html(e) && (PRESERVE.contains(&name) || RAW_TEXT.contains(&name)) {
                write_node(node, out, false, p);
                return;
            }
            open_tag(e, out, p);
            if is_html(e) && VOID.contains(&name) {
                return;
            }
            for child in node.children() {
                write_inline(child, out, p);
            }
            close_tag(name, out, p);
        }
        Node::Text(t) => push_collapsed(t, out),
        Node::Comment(c) => {
            let _ = write!(out, "{}<!--{}-->{}", p.comment, c.comment, p.reset);
        }
        _ => {}
    }
}

/// Render a run of inline siblings as one line; edge whitespace is
/// trimmed (it sits at a block boundary, where it never renders).
fn render_run(nodes: &[NodeRef<Node>], p: &Palette) -> String {
    let mut s = String::new();
    for n in nodes {
        write_inline(*n, &mut s, p);
    }
    s.trim_matches(' ').to_string()
}

/// A block element's children, partitioned: consecutive inline members
/// form runs (one output line each); everything else nests as a block.
enum Group<'a> {
    Run(String),
    Block(NodeRef<'a, Node>),
}

fn group_children<'a>(node: NodeRef<'a, Node>, p: &Palette) -> Vec<Group<'a>> {
    let mut groups = Vec::new();
    let mut run: Vec<NodeRef<Node>> = Vec::new();
    let flush = |run: &mut Vec<NodeRef<'a, Node>>, groups: &mut Vec<Group<'a>>| {
        if run.is_empty() {
            return;
        }
        // A run with visible content (text or inline elements) must stay on
        // one line. A run of only renderless elements/comments (e.g. the
        // <meta>/<link> stack in <head>) can safely take one line each —
        // no visible content is adjacent to those breaks.
        let visible = run.iter().any(|n| match n.value() {
            Node::Text(t) => !t.trim().is_empty(),
            Node::Element(e) => is_html(e) && INLINE.contains(&e.name()),
            _ => false,
        });
        if visible {
            let s = render_run(run, p);
            if !s.is_empty() {
                groups.push(Group::Run(s));
            }
        } else {
            for n in run.iter() {
                let s = render_run(std::slice::from_ref(n), p);
                if !s.is_empty() {
                    groups.push(Group::Run(s));
                }
            }
        }
        run.clear();
    };
    for child in node.children() {
        if is_run_member(&child) {
            run.push(child);
        } else if matches!(child.value(), Node::ProcessingInstruction(_)) {
            continue;
        } else {
            flush(&mut run, &mut groups);
            groups.push(Group::Block(child));
        }
    }
    flush(&mut run, &mut groups);
    groups
}

fn write_pretty(node: NodeRef<Node>, out: &mut String, depth: usize, p: &Palette) {
    match node.value() {
        Node::Element(e) => {
            let name = e.name();
            // Whitespace-sensitive or raw-text subtrees: one indented line,
            // contents verbatim (adding newlines inside <script> could even
            // change JS semantics via ASI).
            if is_html(e) && (PRESERVE.contains(&name) || RAW_TEXT.contains(&name)) {
                indent(out, depth);
                write_node(node, out, false, p);
                out.push('\n');
                return;
            }
            if is_html(e) && VOID.contains(&name) {
                indent(out, depth);
                open_tag(e, out, p);
                out.push('\n');
                return;
            }
            let groups = group_children(node, p);
            indent(out, depth);
            open_tag(e, out, p);
            match groups.as_slice() {
                [] => {
                    close_tag(name, out, p);
                    out.push('\n');
                }
                // All-inline content fits on the element's own line.
                [Group::Run(s)] => {
                    out.push_str(s);
                    close_tag(name, out, p);
                    out.push('\n');
                }
                _ => {
                    out.push('\n');
                    for group in &groups {
                        match group {
                            Group::Run(s) => {
                                indent(out, depth + 1);
                                out.push_str(s);
                                out.push('\n');
                            }
                            Group::Block(child) => write_pretty(*child, out, depth + 1, p),
                        }
                    }
                    indent(out, depth);
                    close_tag(name, out, p);
                    out.push('\n');
                }
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
        // <span> is inline: it stays on the parent's line.
        assert_eq!(
            pretty("<div><span></span></div>", "div"),
            "<div><span></span></div>"
        );
        assert_eq!(pretty("<div></div>", "div"), "<div></div>");
    }

    #[test]
    fn pretty_void_element_no_close() {
        // <img> is inline; <hr> is a block-level void and gets its own line.
        assert_eq!(
            pretty(r#"<div><img src="i.png"></div>"#, "div"),
            "<div><img src=\"i.png\"></div>"
        );
        assert_eq!(
            pretty("<div><p>a</p><hr><p>b</p></div>", "div"),
            "<div>\n  <p>a</p>\n  <hr>\n  <p>b</p>\n</div>"
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
    fn pretty_inline_content_stays_on_one_line() {
        // Splitting inline content across lines would insert whitespace
        // that changes rendering; runs stay intact.
        let p = pretty("<p>before <b>bold</b> after</p>", "p");
        assert_eq!(p, "<p>before <b>bold</b> after</p>");
    }

    #[test]
    fn pretty_never_adds_space_between_inline_elements() {
        // "bolditalx" must not become "bold ital x" (rendering fidelity;
        // the inverse of htmlq#58, which drops significant spaces).
        let p = pretty("<p><b>bold</b><i>ital</i>x</p>", "p");
        assert_eq!(p, "<p><b>bold</b><i>ital</i>x</p>");
    }

    #[test]
    fn pretty_keeps_significant_space_between_inline_elements() {
        let p = pretty("<p><b>a</b> <i>b</i></p>", "p");
        assert_eq!(p, "<p><b>a</b> <i>b</i></p>");
        // Newlines between inline elements collapse to one space.
        let p = pretty("<p><b>a</b>\n  <i>b</i></p>", "p");
        assert_eq!(p, "<p><b>a</b> <i>b</i></p>");
    }

    #[test]
    fn pretty_mixed_inline_and_block_children() {
        let p = pretty("<div>intro <b>x</b><p>para</p>tail</div>", "div");
        assert_eq!(p, "<div>\n  intro <b>x</b>\n  <p>para</p>\n  tail\n</div>");
    }

    #[test]
    fn pretty_inline_script_does_not_split_text() {
        // <script> renders nothing: breaking around it would separate "a"
        // and "b" with whitespace that isn't in the source.
        let p = pretty("<p>a<script>x&&y</script>b</p>", "p");
        assert_eq!(p, "<p>a<script>x&&y</script>b</p>");
    }

    #[test]
    fn pretty_renderless_mid_text_stays_inline() {
        // Wikipedia emits <link> elements mid-paragraph; breaking around
        // them would put a space before the following comma.
        let p = pretty("<p><sup>[update]</sup><link rel=\"x\">, more</p>", "p");
        assert_eq!(p, "<p><sup>[update]</sup><link rel=\"x\">, more</p>");
    }

    #[test]
    fn pretty_renderless_stack_gets_one_line_each() {
        let p = pretty("<head><meta charset=\"a\"><link rel=\"b\"></head>", "head");
        assert_eq!(
            p,
            "<head>\n  <meta charset=\"a\">\n  <link rel=\"b\">\n</head>"
        );
    }

    #[test]
    fn pretty_nested_inline_collapses_internal_whitespace() {
        let p = pretty("<p><b>one\n   two</b> three</p>", "p");
        assert_eq!(p, "<p><b>one two</b> three</p>");
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
