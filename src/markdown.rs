//! `--md`: HTML → Markdown, tuned for readability and LLM pipelines.

use crate::text::{collapse_ws, resolve_url};
use ego_tree::NodeRef;
use scraper::{ElementRef, Node};

pub fn element_to_markdown(el: ElementRef, base: Option<&str>) -> String {
    let mut r = Renderer {
        out: String::new(),
        base,
        list_stack: Vec::new(),
    };
    r.walk_element(el);
    tidy(&r.out)
}

/// Collapse runs of 3+ newlines to exactly 2 and trim the edges.
fn tidy(s: &str) -> String {
    let mut cleaned = String::with_capacity(s.len());
    let mut nl = 0;
    for c in s.chars() {
        if c == '\n' {
            nl += 1;
            if nl <= 2 {
                cleaned.push(c);
            }
        } else {
            nl = 0;
            cleaned.push(c);
        }
    }
    cleaned.trim().to_string()
}

enum ListKind {
    Bullet,
    Numbered(usize),
}

struct Renderer<'a> {
    out: String,
    base: Option<&'a str>,
    list_stack: Vec<ListKind>,
}

impl<'a> Renderer<'a> {
    fn walk_children(&mut self, node: NodeRef<Node>) {
        for child in node.children() {
            match child.value() {
                Node::Text(t) => self.push_inline_text(t.as_ref()),
                Node::Element(_) => {
                    if let Some(el) = ElementRef::wrap(child) {
                        self.walk_element(el);
                    }
                }
                _ => {}
            }
        }
    }

    fn walk_element(&mut self, el: ElementRef) {
        let name = el.value().name();
        match name {
            "script" | "style" | "noscript" | "template" | "head" | "svg" | "iframe" => {}
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<usize>().unwrap_or(1);
                self.block_start();
                self.out.push_str(&"#".repeat(level));
                self.out.push(' ');
                self.walk_children(*el);
                self.block_end();
            }
            "p" => {
                self.block_start();
                self.walk_children(*el);
                self.block_end();
            }
            "br" => self.out.push_str("\\\n"),
            "hr" => {
                self.block_start();
                self.out.push_str("---");
                self.block_end();
            }
            "strong" | "b" => self.wrap_inline(el, "**"),
            "em" | "i" => self.wrap_inline(el, "*"),
            "del" | "s" | "strike" => self.wrap_inline(el, "~~"),
            "code" => {
                // Inline code, unless we're inside <pre> (handled there).
                let t: String = el.text().collect();
                let fence = if t.contains('`') { "``" } else { "`" };
                self.out.push_str(fence);
                self.out.push_str(t.trim_matches('\n'));
                self.out.push_str(fence);
            }
            "pre" => {
                let code: String = el.text().collect();
                let lang = el
                    .select(&sel("code"))
                    .next()
                    .and_then(|c| c.value().attr("class").map(str::to_string))
                    .and_then(|cls| {
                        cls.split_whitespace().find_map(|c| {
                            c.strip_prefix("language-")
                                .or_else(|| c.strip_prefix("lang-"))
                                .map(str::to_string)
                        })
                    })
                    .unwrap_or_default();
                self.block_start();
                self.out.push_str("```");
                self.out.push_str(&lang);
                self.out.push('\n');
                self.out.push_str(code.trim_matches('\n'));
                self.out.push_str("\n```");
                self.block_end();
            }
            "a" => {
                let label = inline_text(el);
                match el.value().attr("href") {
                    Some(href) if !label.is_empty() => {
                        let href = match self.base {
                            Some(b) => resolve_url(b, href),
                            None => href.to_string(),
                        };
                        self.out.push_str(&format!("[{label}]({href})"));
                    }
                    _ => self.out.push_str(&label),
                }
            }
            "img" => {
                let alt = el.value().attr("alt").unwrap_or("");
                if let Some(src) = el.value().attr("src") {
                    let src = match self.base {
                        Some(b) => resolve_url(b, src),
                        None => src.to_string(),
                    };
                    self.out
                        .push_str(&format!("![{}]({src})", collapse_ws(alt)));
                }
            }
            "ul" => {
                self.list_boundary();
                self.list_stack.push(ListKind::Bullet);
                self.walk_children(*el);
                self.list_stack.pop();
                self.list_boundary();
            }
            "ol" => {
                let start = el
                    .value()
                    .attr("start")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                self.list_boundary();
                self.list_stack.push(ListKind::Numbered(start));
                self.walk_children(*el);
                self.list_stack.pop();
                self.list_boundary();
            }
            "li" => {
                self.ensure_newline();
                let depth = self.list_stack.len().saturating_sub(1);
                self.out.push_str(&"  ".repeat(depth));
                let marker = match self.list_stack.last_mut() {
                    Some(ListKind::Numbered(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "- ".to_string(),
                };
                self.out.push_str(&marker);
                self.walk_children(*el);
            }
            "blockquote" => {
                let mut sub = Renderer {
                    out: String::new(),
                    base: self.base,
                    list_stack: Vec::new(),
                };
                sub.walk_children(*el);
                let inner = tidy(&sub.out);
                self.block_start();
                for (i, line) in inner.lines().enumerate() {
                    if i > 0 {
                        self.out.push('\n');
                    }
                    self.out.push_str("> ");
                    self.out.push_str(line);
                }
                self.block_end();
            }
            "table" => {
                self.render_table(el);
            }
            "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" => {
                // Reached only when selecting fragments directly; fall through.
                self.walk_children(*el);
            }
            _ => self.walk_children(*el),
        }
    }

    fn render_table(&mut self, el: ElementRef) {
        let rows: Vec<Vec<String>> = el
            .select(&sel("tr"))
            .map(|tr| {
                tr.select(&sel("th, td"))
                    .map(|c| inline_text(c).replace('|', "\\|"))
                    .collect()
            })
            .filter(|r: &Vec<String>| !r.is_empty())
            .collect();
        if rows.is_empty() {
            return;
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        self.block_start();
        for (i, row) in rows.iter().enumerate() {
            let cells: Vec<&str> = (0..width)
                .map(|j| row.get(j).map(String::as_str).unwrap_or(""))
                .collect();
            self.out.push_str("| ");
            self.out.push_str(&cells.join(" | "));
            self.out.push_str(" |\n");
            if i == 0 {
                self.out.push('|');
                for _ in 0..width {
                    self.out.push_str(" --- |");
                }
                self.out.push('\n');
            }
        }
        self.block_end();
    }

    fn wrap_inline(&mut self, el: ElementRef, mark: &str) {
        let t = inline_text(el);
        if t.is_empty() {
            return;
        }
        self.out.push_str(mark);
        self.out.push_str(&t);
        self.out.push_str(mark);
    }

    fn push_inline_text(&mut self, t: &str) {
        let collapsed = collapse_ws(t);
        if collapsed.is_empty() {
            // Preserve a single separating space between inline runs.
            if t.contains(char::is_whitespace)
                && !self.out.is_empty()
                && !self.out.ends_with([' ', '\n'])
            {
                self.out.push(' ');
            }
            return;
        }
        if t.starts_with(char::is_whitespace)
            && !self.out.is_empty()
            && !self.out.ends_with([' ', '\n'])
        {
            self.out.push(' ');
        }
        self.out.push_str(&collapsed);
        if t.ends_with(char::is_whitespace) {
            self.out.push(' ');
        }
    }

    fn block_start(&mut self) {
        while !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn block_end(&mut self) {
        self.block_start();
    }

    /// Blank line around top-level lists; just a newline when nested.
    fn list_boundary(&mut self) {
        if self.list_stack.is_empty() {
            self.block_start();
        } else {
            self.ensure_newline();
        }
    }

    fn ensure_newline(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }
}

fn inline_text(el: ElementRef) -> String {
    let mut s = String::new();
    for t in el.text() {
        s.push_str(t);
    }
    collapse_ws(&s)
}

fn sel(s: &str) -> scraper::Selector {
    scraper::Selector::parse(s).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn md(html: &str) -> String {
        let doc = Html::parse_document(html);
        element_to_markdown(doc.root_element(), Some("https://ex.com/dir/"))
    }

    #[test]
    fn headings_and_paragraphs() {
        let m = md("<h1>Title</h1><p>Hello <strong>world</strong>.</p>");
        assert_eq!(m, "# Title\n\nHello **world**.");
    }

    #[test]
    fn links_resolve_base() {
        let m = md(r#"<p>See <a href="/x">this</a>.</p>"#);
        assert_eq!(m, "See [this](https://ex.com/x).");
    }

    #[test]
    fn nested_lists() {
        let m = md("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>");
        assert_eq!(m, "- a\n  - b\n- c");
    }

    #[test]
    fn ordered_list_start() {
        let m = md(r#"<ol start="3"><li>x</li><li>y</li></ol>"#);
        assert_eq!(m, "3. x\n4. y");
    }

    #[test]
    fn code_blocks() {
        let m = md(r#"<pre><code class="language-rust">fn main() {}</code></pre>"#);
        assert_eq!(m, "```rust\nfn main() {}\n```");
    }

    #[test]
    fn inline_code() {
        let m = md("<p>run <code>ls -la</code> now</p>");
        assert_eq!(m, "run `ls -la` now");
    }

    #[test]
    fn blockquote() {
        let m = md("<blockquote><p>wise words</p></blockquote>");
        assert_eq!(m, "> wise words");
    }

    #[test]
    fn tables() {
        let m = md("<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>");
        assert_eq!(m, "| A | B |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn skips_script_and_style() {
        let m = md("<p>keep</p><script>var x;</script><style>p{}</style>");
        assert_eq!(m, "keep");
    }

    #[test]
    fn heading_keeps_link() {
        let m = md(r#"<h2><a href="/p/1">Hello</a></h2>"#);
        assert_eq!(m, "## [Hello](https://ex.com/p/1)");
    }

    #[test]
    fn images() {
        let m = md(r#"<p><img src="pic.png" alt="a pic"></p>"#);
        assert_eq!(m, "![a pic](https://ex.com/dir/pic.png)");
    }
}
