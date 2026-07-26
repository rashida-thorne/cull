//! XML parsing: build a `scraper::Html` tree from real XML.
//!
//! HTML parsers silently *mangle* XML: html5ever treats `<link>` as a void
//! element (so every RSS `<link>URL</link>` loses its text and breaks the
//! sibling structure), lowercases tag and attribute names (`pubDate`,
//! `viewBox`), and rearranges content to fit the HTML document model.
//! This module parses XML for real (via `quick-xml`) and builds the same
//! in-memory tree cull uses for HTML, so every downstream feature — CSS
//! selection, `-j` templates, `-t` text, serialization — just works on
//! RSS / Atom feeds, sitemaps, SVG, OPML, and arbitrary XML.
//!
//! Elements are created in the *null* namespace (not the XHTML namespace),
//! which makes CSS selector matching **case-sensitive**, as XML requires:
//! `pubDate` matches `<pubDate>` and nothing else. Namespaced tags keep
//! their prefix and can be selected with an escaped colon
//! (`media\:thumbnail`).

use ego_tree::NodeId;
use html5ever::{Attribute, LocalName, QualName, ns};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use scraper::node::{Comment, Element, Text};
use scraper::{Html, Node, StrTendril};

/// True if `src` looks like an XML document rather than HTML.
///
/// Matches an `<?xml … ?>` declaration, or a first element that is a
/// well-known XML root: `rss`, `feed` (Atom), `urlset` / `sitemapindex`
/// (sitemaps), `opml`, or `svg`.
pub fn looks_like_xml(src: &str) -> bool {
    let s = src.trim_start_matches('\u{feff}').trim_start();
    if s.starts_with("<?xml") {
        return true;
    }
    // Find the first element tag, skipping comments, doctype, and PIs.
    let mut rest = s;
    loop {
        let Some(idx) = rest.find('<') else {
            return false;
        };
        let tag = &rest[idx..];
        if let Some(after) = tag.strip_prefix("<!--") {
            match after.find("-->") {
                Some(end) => {
                    rest = &after[end + 3..];
                    continue;
                }
                None => return false,
            }
        }
        if tag.starts_with("<!") || tag.starts_with("<?") {
            match tag.find('>') {
                Some(end) => {
                    rest = &tag[end + 1..];
                    continue;
                }
                None => return false,
            }
        }
        let name: String = tag[1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == ':' || *c == '-' || *c == '_')
            .collect();
        return matches!(
            name.as_str(),
            "rss" | "feed" | "urlset" | "sitemapindex" | "opml" | "svg"
        );
    }
}

/// Parse an XML document into a `scraper::Html` tree.
///
/// Returns `Err` with a human-readable message (including the byte offset)
/// if the document is not well-formed.
pub fn parse_xml(src: &str) -> Result<Html, String> {
    let mut reader = Reader::from_str(src);
    let config = reader.config_mut();
    config.check_end_names = true;

    let mut html = Html::new_document();
    let root = html.tree.root().id();
    let mut stack: Vec<NodeId> = vec![root];

    loop {
        let pos = reader.buffer_position();
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let id = append_element(&mut html, &stack, &start)?;
                stack.push(id);
            }
            Ok(Event::Empty(start)) => {
                append_element(&mut html, &stack, &start)?;
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            Ok(Event::Text(t)) => {
                let text = t
                    .xml_content(quick_xml::XmlVersion::Implicit1_0)
                    .map_err(|e| err_at(pos, &e.to_string()))?;
                append_text(&mut html, &stack, &text);
            }
            Ok(Event::CData(c)) => {
                let text = c.decode().map_err(|e| err_at(pos, &e.to_string()))?;
                append_text(&mut html, &stack, &text);
            }
            Ok(Event::GeneralRef(r)) => {
                // Character refs (`&#8212;`) and the five predefined XML
                // entities resolve to text; anything else (e.g. HTML's
                // `&nbsp;`, undeclared in XML) is kept literally.
                let decoded = r.decode().map_err(|e| err_at(pos, &e.to_string()))?;
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    _ => match decoded.as_ref() {
                        "lt" => "<".to_string(),
                        "gt" => ">".to_string(),
                        "amp" => "&".to_string(),
                        "apos" => "'".to_string(),
                        "quot" => "\"".to_string(),
                        other => format!("&{other};"),
                    },
                };
                append_text(&mut html, &stack, &resolved);
            }
            Ok(Event::Comment(c)) => {
                let text = c.decode().map_err(|e| err_at(pos, &e.to_string()))?;
                let node = Node::Comment(Comment {
                    comment: StrTendril::from(text.as_ref()),
                });
                append(&mut html, &stack, node);
            }
            Ok(Event::Decl(_) | Event::PI(_) | Event::DocType(_)) => {}
            Ok(Event::Eof) => break,
            Err(e) => return Err(err_at(pos, &e.to_string())),
        }
    }

    if stack.len() > 1 {
        return Err("XML parse error: unclosed element at end of input".to_string());
    }
    Ok(html)
}

fn err_at(pos: u64, msg: &str) -> String {
    format!("XML parse error at byte {pos}: {msg}")
}

fn append(html: &mut Html, stack: &[NodeId], node: Node) -> NodeId {
    let parent = *stack.last().expect("stack never empty");
    html.tree
        .get_mut(parent)
        .expect("parent exists")
        .append(node)
        .id()
}

fn append_text(html: &mut Html, stack: &[NodeId], text: &str) {
    // Merge with a preceding text node if any (refs split text into
    // multiple events; downstream code expects contiguous text).
    let parent = *stack.last().expect("stack never empty");
    let last = html
        .tree
        .get(parent)
        .and_then(|p| p.last_child())
        .map(|c| c.id());
    if let Some(last_id) = last
        && let Some(mut node) = html.tree.get_mut(last_id)
        && let Node::Text(t) = node.value()
    {
        t.text.push_slice(text);
        return;
    }
    append(
        html,
        stack,
        Node::Text(Text {
            text: StrTendril::from(text),
        }),
    );
}

fn append_element(html: &mut Html, stack: &[NodeId], start: &BytesStart) -> Result<NodeId, String> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs: Vec<Attribute> = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| format!("XML parse error: bad attribute in <{name}>: {e}"))?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| format!("XML parse error: bad attribute value in <{name}>: {e}"))?;
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), LocalName::from(key.as_str())),
            value: StrTendril::from(value.as_ref()),
        });
    }
    // Null namespace (not XHTML) => case-sensitive CSS matching, XML-style.
    let qual = QualName::new(None, ns!(), LocalName::from(name.as_str()));
    let node = Node::Element(Element::new(qual, attrs));
    Ok(append(html, stack, node))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Selector;

    fn sel(s: &str) -> Selector {
        Selector::parse(s).unwrap()
    }

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
  <channel>
    <title>Example Feed</title>
    <link>https://example.com/</link>
    <item>
      <title>First &amp; Foremost</title>
      <link>https://example.com/1</link>
      <pubDate>Mon, 01 Jan 2026 00:00:00 GMT</pubDate>
      <description><![CDATA[Some <b>bold</b> claims]]></description>
      <media:thumbnail url="https://example.com/1.jpg"/>
    </item>
    <item>
      <title>Second</title>
      <link>https://example.com/2</link>
      <pubDate>Tue, 02 Jan 2026 00:00:00 GMT</pubDate>
    </item>
  </channel>
</rss>"#;

    #[test]
    fn rss_links_keep_their_text() {
        // The whole reason this module exists: html5ever parses <link> as a
        // void element and drops the URL.
        let doc = parse_xml(RSS).unwrap();
        let links: Vec<String> = doc
            .select(&sel("item > link"))
            .map(|el| el.text().collect())
            .collect();
        assert_eq!(links, ["https://example.com/1", "https://example.com/2"]);
    }

    #[test]
    fn case_sensitive_selectors() {
        let doc = parse_xml(RSS).unwrap();
        assert_eq!(doc.select(&sel("pubDate")).count(), 2);
        // XML is case-sensitive: lowercase must NOT match.
        assert_eq!(doc.select(&sel("pubdate")).count(), 0);
    }

    #[test]
    fn cdata_is_text() {
        let doc = parse_xml(RSS).unwrap();
        let desc: String = doc
            .select(&sel("item description"))
            .next()
            .unwrap()
            .text()
            .collect();
        assert_eq!(desc, "Some <b>bold</b> claims");
    }

    #[test]
    fn entities_resolve_and_merge() {
        let doc = parse_xml(RSS).unwrap();
        let title: String = doc
            .select(&sel("item > title"))
            .next()
            .unwrap()
            .text()
            .collect();
        assert_eq!(title, "First & Foremost");
    }

    #[test]
    fn char_refs_resolve() {
        let doc = parse_xml("<r><t>A&#8212;B and &#x2014; too</t></r>").unwrap();
        let t: String = doc.select(&sel("t")).next().unwrap().text().collect();
        assert_eq!(t, "A\u{2014}B and \u{2014} too");
    }

    #[test]
    fn unknown_entities_kept_literally() {
        let doc = parse_xml("<r><t>a&nbsp;b</t></r>").unwrap();
        let t: String = doc.select(&sel("t")).next().unwrap().text().collect();
        assert_eq!(t, "a&nbsp;b");
    }

    #[test]
    fn namespaced_tags_selectable_with_escaped_colon() {
        let doc = parse_xml(RSS).unwrap();
        let url = doc
            .select(&sel(r"media\:thumbnail"))
            .next()
            .unwrap()
            .value()
            .attr("url")
            .unwrap()
            .to_string();
        assert_eq!(url, "https://example.com/1.jpg");
    }

    #[test]
    fn attributes_keep_case() {
        let doc = parse_xml(r#"<svg viewBox="0 0 10 10"><rect/></svg>"#).unwrap();
        let el = doc.select(&sel("svg")).next().unwrap();
        assert_eq!(el.value().attr("viewBox"), Some("0 0 10 10"));
    }

    #[test]
    fn malformed_is_an_error() {
        assert!(parse_xml("<a><b></a>").is_err());
        assert!(parse_xml("<a>").is_err());
    }

    #[test]
    fn detection() {
        assert!(looks_like_xml("<?xml version=\"1.0\"?><foo/>"));
        assert!(looks_like_xml("\u{feff}  <?xml version=\"1.0\"?><foo/>"));
        assert!(looks_like_xml("<rss version=\"2.0\"></rss>"));
        assert!(looks_like_xml("<!-- hi --><feed xmlns=\"x\"></feed>"));
        assert!(looks_like_xml(
            "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"></urlset>"
        ));
        assert!(!looks_like_xml("<!DOCTYPE html><html></html>"));
        assert!(!looks_like_xml("<div>plain html</div>"));
        assert!(!looks_like_xml("plain text, no tags"));
    }

    #[test]
    fn atom_links_via_attr() {
        let atom = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry><title>Post</title><link rel="alternate" href="https://example.com/p"/></entry>
</feed>"#;
        let doc = parse_xml(atom).unwrap();
        let href = doc
            .select(&sel("entry > link"))
            .next()
            .unwrap()
            .value()
            .attr("href")
            .unwrap()
            .to_string();
        assert_eq!(href, "https://example.com/p");
    }
}
