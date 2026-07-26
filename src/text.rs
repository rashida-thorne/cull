use ego_tree::NodeRef;
use scraper::{ElementRef, Node};

/// Text content with whitespace collapsed to single spaces, one line.
/// Layout-aware: `<br>` and block-element boundaries become a single
/// space (so `% of<br>world` reads "% of world", not "% ofworld"), and
/// invisible elements (`script`, `style`, ...) contribute nothing.
/// Used wherever a single-line value is wanted (JSON templates, CSV
/// cells, `--has-text` matching).
pub fn collapsed_text(el: ElementRef) -> String {
    collapse_ws(&block_text(el))
}

/// Elements that never contribute visible text.
const INVISIBLE: &[&str] = &["script", "style", "template", "head", "noscript"];

/// Elements rendered on their own line(s): a boundary before and after.
const BLOCK: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "option",
    "p",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Elements whose text is whitespace-sensitive: kept verbatim.
const VERBATIM: &[&str] = &["pre", "textarea"];

enum Seg {
    /// Inline text; whitespace will be collapsed.
    Text(String),
    /// Whitespace-sensitive text (<pre>), kept as-is.
    Verbatim(String),
    /// A line boundary (block edge or <br>).
    Break,
}

/// Text content the way a browser would lay it out, roughly like
/// `innerText`: `<br>` and block-element boundaries become newlines,
/// `<pre>`/`<textarea>` contents are preserved verbatim, invisible
/// elements (`script`, `style`, ...) are skipped, and everything else
/// has its whitespace collapsed.
pub fn block_text(el: ElementRef) -> String {
    let mut segs = Vec::new();
    collect(*el, &mut segs);
    render(&segs)
}

fn collect(node: NodeRef<Node>, segs: &mut Vec<Seg>) {
    match node.value() {
        Node::Text(t) => {
            if let Some(Seg::Text(prev)) = segs.last_mut() {
                prev.push_str(t);
            } else {
                segs.push(Seg::Text(t.to_string()));
            }
        }
        Node::Element(e) => {
            let name = e.name();
            // XML elements (non-HTML namespace, e.g. from --xml) have no
            // inline/block distinction; treat each as a block so sibling
            // fields land on separate lines instead of gluing together.
            if !crate::serialize::is_html(e) {
                segs.push(Seg::Break);
                for child in node.children() {
                    collect(child, segs);
                }
                segs.push(Seg::Break);
                return;
            }
            if INVISIBLE.contains(&name) {
                return;
            }
            if name == "br" {
                segs.push(Seg::Break);
                return;
            }
            if VERBATIM.contains(&name) {
                let mut raw = String::new();
                for chunk in ElementRef::wrap(node)
                    .map(|el| el.text())
                    .into_iter()
                    .flatten()
                {
                    raw.push_str(chunk);
                }
                // Outer newlines are layout, not content.
                let raw = raw.trim_matches('\n');
                if !raw.is_empty() {
                    segs.push(Seg::Break);
                    segs.push(Seg::Verbatim(raw.to_string()));
                    segs.push(Seg::Break);
                }
                return;
            }
            let block = BLOCK.contains(&name);
            if block {
                segs.push(Seg::Break);
            }
            for child in node.children() {
                collect(child, segs);
            }
            if block {
                segs.push(Seg::Break);
            }
        }
        Node::Document | Node::Fragment => {
            for child in node.children() {
                collect(child, segs);
            }
        }
        _ => {}
    }
}

fn render(segs: &[Seg]) -> String {
    let mut out = String::new();
    for seg in segs {
        match seg {
            Seg::Break => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Seg::Text(t) => {
                let collapsed = collapse_ws(t);
                if !collapsed.is_empty() {
                    // Mid-line continuation keeps the word boundary the
                    // source whitespace implied.
                    if !out.is_empty()
                        && !out.ends_with('\n')
                        && t.starts_with(|c: char| c.is_whitespace())
                    {
                        out.push(' ');
                    }
                    out.push_str(&collapsed);
                }
            }
            Seg::Verbatim(v) => {
                out.push_str(v);
            }
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_ws = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_ws {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.push(c);
            last_ws = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Resolve URL-ish attributes against a base, if one is set.
pub fn maybe_resolve(attr: &str, value: &str, base: Option<&str>) -> String {
    let Some(base) = base else {
        return value.to_string();
    };
    if !matches!(
        attr,
        "href" | "src" | "srcset" | "action" | "poster" | "data-src"
    ) {
        return value.to_string();
    }
    resolve_url(base, value)
}

/// Minimal RFC-3986-ish reference resolution — enough for scraping.
pub fn resolve_url(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.is_empty() {
        return base.to_string();
    }
    // Already absolute.
    if rel.contains("://") || rel.starts_with("data:") || rel.starts_with("mailto:") {
        return rel.to_string();
    }
    // Protocol-relative.
    if let Some(rest) = rel.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}://{rest}");
    }
    // Split base into origin + path.
    let (origin, path) = match base.find("://") {
        Some(i) => {
            let after = &base[i + 3..];
            match after.find('/') {
                Some(j) => (&base[..i + 3 + j], &base[i + 3 + j..]),
                None => (base, "/"),
            }
        }
        None => return rel.to_string(),
    };
    if let Some(frag) = rel.strip_prefix('#') {
        let p = path.split('#').next().unwrap_or(path);
        return format!("{origin}{p}#{frag}");
    }
    if rel.starts_with('/') {
        return format!("{origin}{}", normalize_path(rel));
    }
    if rel.starts_with('?') {
        let p = path.split('?').next().unwrap_or(path);
        return format!("{origin}{p}{rel}");
    }
    // Relative path: strip filename from base path.
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    format!("{origin}{}", normalize_path(&format!("{dir}{rel}")))
}

fn normalize_path(p: &str) -> String {
    // Keep query/fragment intact.
    let (path, suffix) = match p.find(['?', '#']) {
        Some(i) => (&p[..i], &p[i..]),
        None => (p, ""),
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut joined = parts.join("/");
    if !joined.starts_with('/') {
        joined.insert(0, '/');
    }
    format!("{joined}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_collapse() {
        assert_eq!(collapse_ws("  a\n\t b  c "), "a b c");
    }

    fn bt(html: &str) -> String {
        let doc = scraper::Html::parse_fragment(html);
        block_text(doc.root_element())
    }

    #[test]
    fn block_text_br_is_newline() {
        assert_eq!(bt("a<br>b<br><br>c"), "a\nb\nc");
    }

    #[test]
    fn block_text_block_boundaries() {
        assert_eq!(bt("<div>x<p>para</p>y</div>"), "x\npara\ny");
        assert_eq!(bt("<ul><li>one</li><li>two</li></ul>"), "one\ntwo");
    }

    #[test]
    fn block_text_inline_stays_inline() {
        assert_eq!(bt("x <b>bold</b> and <i>italic</i>."), "x bold and italic.");
        assert_eq!(bt("gl<b>ue</b>d"), "glued");
    }

    #[test]
    fn block_text_pre_verbatim() {
        assert_eq!(bt("<p>a</p><pre>  two\n  lines</pre>"), "a\n  two\n  lines");
    }

    #[test]
    fn block_text_skips_invisible() {
        assert_eq!(
            bt("<div>seen<script>var x=1;</script><style>p{}</style></div>"),
            "seen"
        );
    }

    #[test]
    fn block_text_collapses_inline_ws() {
        assert_eq!(bt("<p>a\n   b</p>"), "a b");
    }

    fn ct(html: &str) -> String {
        let doc = scraper::Html::parse_fragment(html);
        collapsed_text(doc.root_element())
    }

    #[test]
    fn collapsed_text_br_becomes_space() {
        // Wikipedia header cells like `% of<br>world` must not glue.
        assert_eq!(ct("% of<br>world"), "% of world");
        assert_eq!(ct("a<br><br>b"), "a b");
    }

    #[test]
    fn collapsed_text_block_boundary_becomes_space() {
        assert_eq!(ct("<div>one</div><div>two</div>"), "one two");
    }

    #[test]
    fn collapsed_text_skips_invisible() {
        assert_eq!(ct("seen<style>td{color:red}</style>"), "seen");
    }

    #[test]
    fn collapsed_text_inline_still_tight() {
        assert_eq!(ct("gl<b>ue</b>d"), "glued");
    }

    #[test]
    fn url_resolution() {
        let b = "https://ex.com/a/b/page.html?q=1";
        assert_eq!(resolve_url(b, "img.png"), "https://ex.com/a/b/img.png");
        assert_eq!(resolve_url(b, "../up.html"), "https://ex.com/a/up.html");
        assert_eq!(resolve_url(b, "/root.css"), "https://ex.com/root.css");
        assert_eq!(resolve_url(b, "//cdn.ex.com/x"), "https://cdn.ex.com/x");
        assert_eq!(resolve_url(b, "https://other.com/"), "https://other.com/");
        assert_eq!(resolve_url(b, "?p=2"), "https://ex.com/a/b/page.html?p=2");
    }
}
