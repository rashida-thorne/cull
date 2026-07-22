//! The `-j/--json` extraction template: a tiny, jq-flavoured shape language.
//!
//! Template grammar (whitespace-insensitive):
//!   template := object | array | expr
//!   object   := '{' key ':' template (',' key ':' template)* [','] '}'
//!   array    := '[' template ']'          -- collects *all* matches
//!   expr     := selector [ '@' field ]    -- first match only
//!   key      := bareword | "quoted"
//!   field    := text | html | an attribute name (e.g. href)
//!
//! A selector is any CSS selector; quote it ("a.x, a.y") if it contains
//! `,`, `}`, `]`, or `@`. The selector `.` (or an empty one) means the
//! context element itself.
//!
//! Example:
//!   {title: h2, url: a @href, tags: [.tag], meta: {id: . @data-id}}

use crate::text;
use scraper::{ElementRef, Selector};
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum Template {
    Object(Vec<(String, Template)>),
    Array(Box<Template>),
    Expr {
        selector: Option<Selector>,
        field: Field,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Text,
    Html,
    InnerHtml,
    Attr(String),
}

pub fn parse_template(src: &str) -> Result<Template, String> {
    let mut p = Parser {
        chars: src.chars().collect(),
        pos: 0,
        src,
    };
    let t = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(format!(
            "template: unexpected trailing input at position {}: {:?}",
            p.pos,
            &src[p.byte_pos()..]
        ));
    }
    Ok(t)
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    src: &'a str,
}

impl<'a> Parser<'a> {
    fn byte_pos(&self) -> usize {
        self.chars[..self.pos].iter().map(|c| c.len_utf8()).sum()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Template, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some(_) => self.parse_expr(),
            None => Err("template: unexpected end of input".into()),
        }
    }

    fn parse_object(&mut self) -> Result<Template, String> {
        self.bump(); // '{'
        let mut pairs = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some('}') {
                self.bump();
                break;
            }
            let key = self.parse_key()?;
            self.skip_ws();
            if self.bump() != Some(':') {
                return Err(format!("template: expected ':' after key {key:?}"));
            }
            let val = self.parse_value()?;
            pairs.push((key, val));
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {}
                other => {
                    return Err(format!(
                        "template: expected ',' or '}}' in object, found {other:?}"
                    ));
                }
            }
        }
        if pairs.is_empty() {
            return Err("template: empty object".into());
        }
        Ok(Template::Object(pairs))
    }

    fn parse_array(&mut self) -> Result<Template, String> {
        self.bump(); // '['
        let inner = self.parse_value()?;
        self.skip_ws();
        if self.bump() != Some(']') {
            return Err("template: expected ']' to close array".into());
        }
        Ok(Template::Array(Box::new(inner)))
    }

    fn parse_key(&mut self) -> Result<String, String> {
        self.skip_ws();
        if matches!(self.peek(), Some('"') | Some('\'')) {
            return self.parse_quoted();
        }
        let mut key = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                key.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if key.is_empty() {
            return Err(format!("template: expected a key at position {}", self.pos));
        }
        Ok(key)
    }

    fn parse_quoted(&mut self) -> Result<String, String> {
        let quote = self.bump().unwrap();
        let mut s = String::new();
        loop {
            match self.bump() {
                Some('\\') => match self.bump() {
                    Some(c) => s.push(c),
                    None => return Err("template: unterminated escape".into()),
                },
                Some(c) if c == quote => break,
                Some(c) => s.push(c),
                None => return Err("template: unterminated string".into()),
            }
        }
        Ok(s)
    }

    /// selector [ '@' field ] — selector may be quoted, or runs until , } ] @
    fn parse_expr(&mut self) -> Result<Template, String> {
        self.skip_ws();
        let selector_src = if matches!(self.peek(), Some('"') | Some('\'')) {
            self.parse_quoted()?
        } else {
            let mut s = String::new();
            while let Some(c) = self.peek() {
                if matches!(c, ',' | '}' | ']' | '@') {
                    break;
                }
                s.push(c);
                self.bump();
            }
            s.trim().to_string()
        };

        if selector_src.is_empty() && self.peek() != Some('@') {
            return Err(format!(
                "template: expected a selector at position {} (use '.' for the current element)",
                self.pos
            ));
        }

        self.skip_ws();
        let field = if self.peek() == Some('@') {
            self.bump();
            let mut f = String::new();
            while let Some(c) = self.peek() {
                if c.is_alphanumeric() || c == '_' || c == '-' || c == ':' {
                    f.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
            match f.as_str() {
                "" => return Err("template: expected a field name after '@'".into()),
                "text" => Field::Text,
                "html" => Field::Html,
                "innerhtml" | "inner-html" => Field::InnerHtml,
                name => Field::Attr(name.to_string()),
            }
        } else {
            Field::Text
        };

        let selector = if selector_src.is_empty() || selector_src == "." {
            None
        } else {
            Some(
                Selector::parse(&selector_src)
                    .map_err(|e| format!("template: invalid selector {selector_src:?}: {e}"))?,
            )
        };

        let _ = self.src; // keep lifetime used
        Ok(Template::Expr { selector, field })
    }
}

pub fn eval(t: &Template, ctx: ElementRef, base: Option<&str>) -> Value {
    match t {
        Template::Object(pairs) => {
            let mut map = Map::new();
            for (k, v) in pairs {
                map.insert(k.clone(), eval(v, ctx, base));
            }
            Value::Object(map)
        }
        Template::Array(inner) => match inner.as_ref() {
            Template::Expr { selector, field } => {
                let els: Vec<ElementRef> = match selector {
                    Some(sel) => ctx.select(sel).collect(),
                    None => vec![ctx],
                };
                Value::Array(
                    els.into_iter()
                        .filter_map(|el| field_value(el, field, base))
                        .collect(),
                )
            }
            // Array of objects/arrays needs an element scope: use the object's
            // first Expr selector... simpler rule: [{...}] iterates ctx itself
            // is meaningless, so we require [selector {…}] form? v0.1: iterate
            // over ctx children matching nothing — instead treat as single eval.
            other => Value::Array(vec![eval(other, ctx, base)]),
        },
        Template::Expr { selector, field } => {
            let el = match selector {
                Some(sel) => ctx.select(sel).next(),
                None => Some(ctx),
            };
            el.and_then(|el| field_value(el, field, base))
                .unwrap_or(Value::Null)
        }
    }
}

fn field_value(el: ElementRef, field: &Field, base: Option<&str>) -> Option<Value> {
    match field {
        Field::Text => Some(Value::String(text::collapsed_text(el))),
        Field::Html => Some(Value::String(el.html())),
        Field::InnerHtml => Some(Value::String(el.inner_html())),
        Field::Attr(name) => el
            .value()
            .attr(name)
            .map(|v| Value::String(text::maybe_resolve(name, v, base))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn doc() -> Html {
        Html::parse_document(
            r#"<div class="post" data-id="42">
                 <h2>Hello <em>World</em></h2>
                 <a href="/p/1">read</a>
                 <span class="tag">rust</span><span class="tag">cli</span>
               </div>"#,
        )
    }

    fn eval_on_doc(tmpl: &str) -> Value {
        let d = doc();
        let sel = Selector::parse(".post").unwrap();
        let el = d.select(&sel).next().unwrap();
        eval(
            &parse_template(tmpl).unwrap(),
            el,
            Some("https://ex.com/x/"),
        )
    }

    #[test]
    fn object_shape() {
        let v = eval_on_doc("{title: h2, url: a @href, tags: [.tag], id: . @data-id}");
        assert_eq!(v["title"], "Hello World");
        assert_eq!(v["url"], "https://ex.com/p/1");
        assert_eq!(v["tags"], serde_json::json!(["rust", "cli"]));
        assert_eq!(v["id"], "42");
    }

    #[test]
    fn missing_is_null() {
        let v = eval_on_doc("{x: h9, y: [h9]}");
        assert_eq!(v["x"], Value::Null);
        assert_eq!(v["y"], serde_json::json!([]));
    }

    #[test]
    fn bare_expr() {
        let v = eval_on_doc("h2");
        assert_eq!(v, "Hello World");
    }

    #[test]
    fn quoted_selector_with_comma() {
        let v = eval_on_doc(r#"{both: ["h2, a"]}"#);
        assert_eq!(v["both"], serde_json::json!(["Hello World", "read"]));
    }

    #[test]
    fn parse_errors() {
        assert!(parse_template("{a: }").is_err());
        assert!(parse_template("{a: b").is_err());
        assert!(parse_template("").is_err());
    }
}
