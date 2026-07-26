//! The `-j/--json` extraction template: a tiny, jq-flavoured shape language.
//!
//! Template grammar (whitespace-insensitive):
//!   template := object | array | expr
//!   object   := '{' key ':' template (',' key ':' template)* [','] '}'
//!   array    := '[' template ']'                     -- collects *all* matches
//!   expr     := selector [ '@' field ] [ '|' filter ] -- first match only
//!   key      := bareword | "quoted"
//!   field    := text | html | an attribute name (e.g. href)
//!   filter   := num    -- pull the first number out of the value ("1,234
//!                         points" → 1234, "$3.50" → 3.5); null if none
//!
//! A selector is any CSS selector; quote it ("a.x, a.y") if it contains
//! `,`, `}`, `]`, `@`, or `|`. The selector `.` (or an empty one) means the
//! context element itself.
//!
//! Example:
//!   {title: h2, url: a @href, score: .points | num, tags: [.tag]}

use crate::text;
use scraper::{ElementRef, Selector};
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum Template {
    Object(Vec<(String, Template)>),
    Array(Box<Template>),
    /// `sel { ... }` — evaluate `inner` with the (first) match of `selector`
    /// as the new context element. Inside an Array, iterates every match.
    Scoped {
        selector: Option<Selector>,
        inner: Box<Template>,
    },
    Expr {
        selector: Option<Selector>,
        field: Field,
        filters: Vec<Filter>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Text,
    Html,
    InnerHtml,
    Attr(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Filter {
    /// Extract the first number in the value ("1,234 points" → 1234).
    Num,
    /// Trim leading/trailing whitespace (useful for attribute values).
    Trim,
    /// ASCII-aware Unicode lowercase.
    Lower,
    /// ASCII-aware Unicode uppercase.
    Upper,
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

    /// selector [ '{' object '}' | '@' field ] [ '|' filter ]* —
    /// selector may be quoted, or runs until , { } ] @ |
    fn parse_expr(&mut self) -> Result<Template, String> {
        self.skip_ws();
        let selector_src = if matches!(self.peek(), Some('"') | Some('\'')) {
            self.parse_quoted()?
        } else {
            let mut s = String::new();
            while let Some(c) = self.peek() {
                if matches!(c, ',' | '{' | '}' | ']' | '@' | '|') {
                    break;
                }
                s.push(c);
                self.bump();
            }
            s.trim().to_string()
        };

        self.skip_ws();
        // `sel { ... }` — scope the object to the selector's match(es).
        if self.peek() == Some('{') && !selector_src.is_empty() {
            let selector = if selector_src == "." {
                None
            } else {
                Some(
                    Selector::parse(&selector_src)
                        .map_err(|e| format!("template: invalid selector {selector_src:?}: {e}"))?,
                )
            };
            let inner = self.parse_object()?;
            return Ok(Template::Scoped {
                selector,
                inner: Box::new(inner),
            });
        }

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

        let mut filters = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() != Some('|') {
                break;
            }
            self.bump();
            self.skip_ws();
            let mut f = String::new();
            while let Some(c) = self.peek() {
                if c.is_alphanumeric() || c == '_' {
                    f.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
            filters.push(match f.as_str() {
                "num" => Filter::Num,
                "trim" => Filter::Trim,
                "lower" => Filter::Lower,
                "upper" => Filter::Upper,
                "" => return Err("template: expected a filter name after '|'".into()),
                other => {
                    return Err(format!(
                        "template: unknown filter {other:?} (available: num, trim, lower, upper)"
                    ));
                }
            });
        }

        let selector = if selector_src.is_empty() || selector_src == "." {
            None
        } else {
            Some(
                Selector::parse(&selector_src)
                    .map_err(|e| format!("template: invalid selector {selector_src:?}: {e}"))?,
            )
        };

        let _ = self.src; // keep lifetime used
        Ok(Template::Expr {
            selector,
            field,
            filters,
        })
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
            Template::Expr {
                selector,
                field,
                filters,
            } => {
                let els: Vec<ElementRef> = match selector {
                    Some(sel) => ctx.select(sel).collect(),
                    None => vec![ctx],
                };
                Value::Array(
                    els.into_iter()
                        .filter_map(|el| field_value(el, field, base))
                        .filter_map(|v| apply_filters(v, filters))
                        .collect(),
                )
            }
            // `[sel {…}]` — one object per matching element.
            Template::Scoped { selector, inner } => {
                let els: Vec<ElementRef> = match selector {
                    Some(sel) => ctx.select(sel).collect(),
                    None => vec![ctx],
                };
                Value::Array(els.into_iter().map(|el| eval(inner, el, base)).collect())
            }
            other => Value::Array(vec![eval(other, ctx, base)]),
        },
        // `sel {…}` outside an array — scope to the first match (null if none).
        Template::Scoped { selector, inner } => {
            let el = match selector {
                Some(sel) => ctx.select(sel).next(),
                None => Some(ctx),
            };
            el.map(|el| eval(inner, el, base)).unwrap_or(Value::Null)
        }
        Template::Expr {
            selector,
            field,
            filters,
        } => {
            let el = match selector {
                Some(sel) => ctx.select(sel).next(),
                None => Some(ctx),
            };
            el.and_then(|el| field_value(el, field, base))
                .and_then(|v| apply_filters(v, filters))
                .unwrap_or(Value::Null)
        }
    }
}

/// Apply a chain of post-filters to an extracted value, in order.
fn apply_filters(v: Value, filters: &[Filter]) -> Option<Value> {
    let mut v = v;
    for f in filters {
        v = match (f, v) {
            (Filter::Num, Value::String(s)) => first_number(&s)?,
            (Filter::Trim, Value::String(s)) => Value::String(s.trim().to_string()),
            (Filter::Lower, Value::String(s)) => Value::String(s.to_lowercase()),
            (Filter::Upper, Value::String(s)) => Value::String(s.to_uppercase()),
            (_, other) => other, // non-strings pass through unchanged
        };
    }
    Some(v)
}

/// Find the first number in a string. Commas between digits are treated as
/// thousands separators ("1,234 points" → 1234); "$3.50" → 3.5; "-5" → -5.
fn first_number(s: &str) -> Option<Value> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let neg = chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
        if neg || chars[i].is_ascii_digit() {
            let mut buf = String::new();
            if neg {
                buf.push('-');
                i += 1;
            }
            let mut seen_dot = false;
            while i < chars.len() {
                let c = chars[i];
                if c.is_ascii_digit() {
                    buf.push(c);
                    i += 1;
                } else if c == ','
                    && i + 1 < chars.len()
                    && chars[i + 1].is_ascii_digit()
                    && !seen_dot
                {
                    i += 1; // thousands separator: skip
                } else if c == '.'
                    && !seen_dot
                    && i + 1 < chars.len()
                    && chars[i + 1].is_ascii_digit()
                {
                    seen_dot = true;
                    buf.push('.');
                    i += 1;
                } else {
                    break;
                }
            }
            return if seen_dot {
                buf.parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
            } else {
                buf.parse::<i64>().ok().map(|n| Value::Number(n.into()))
            };
        }
        i += 1;
    }
    None
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
        assert!(parse_template("{a: b | }").is_err());
        assert!(parse_template("{a: b | nope}").is_err());
    }

    #[test]
    fn num_filter() {
        let html = Html::parse_document(
            r#"<div class="post">
                 <span class="pts">1,234 points</span>
                 <span class="price">$3.50</span>
                 <span class="delta">-5 today</span>
                 <span class="none">no digits here</span>
               </div>"#,
        );
        let root = html
            .select(&Selector::parse(".post").unwrap())
            .next()
            .unwrap();
        let t =
            parse_template("{pts: .pts | num, price: .price|num, d: .delta | num, n: .none | num}")
                .unwrap();
        let v = eval(&t, root, None);
        assert_eq!(v["pts"], 1234);
        assert_eq!(v["price"], 3.5);
        assert_eq!(v["d"], -5);
        assert_eq!(v["n"], Value::Null);
    }

    #[test]
    fn scoped_object_array() {
        let html = Html::parse_document(
            r#"<div class="post"><h2> A </h2>
                 <span class="c"><b class="u">Joe</b><i class="t">hi</i></span>
                 <span class="c"><b class="u">Amy</b><i class="t">yo</i></span>
               </div>"#,
        );
        let root = html
            .select(&Selector::parse(".post").unwrap())
            .next()
            .unwrap();
        // [sel {…}] — one object per match
        let t = parse_template("{title: h2, comments: [.c {user: .u, text: .t}]}").unwrap();
        let v = eval(&t, root, None);
        assert_eq!(v["title"], "A");
        assert_eq!(v["comments"][0]["user"], "Joe");
        assert_eq!(v["comments"][1]["text"], "yo");
        assert_eq!(v["comments"].as_array().unwrap().len(), 2);
        // sel {…} — scoped to first match
        let t = parse_template("{first: .c {user: .u}}").unwrap();
        let v = eval(&t, root, None);
        assert_eq!(v["first"]["user"], "Joe");
        // sel {…} with no match — null, not an error
        let t = parse_template("{missing: .zzz {x: b}}").unwrap();
        let v = eval(&t, root, None);
        assert_eq!(v["missing"], Value::Null);
        // nested arrays inside scoped objects
        let t = parse_template("{cs: [.c {parts: [i]}]}").unwrap();
        let v = eval(&t, root, None);
        assert_eq!(v["cs"][0]["parts"][0], "hi");
    }

    #[test]
    fn filter_chains() {
        let v = eval_on_doc("{a: h2 | lower, b: .tag | upper, c: . @data-id | num}");
        assert_eq!(v["a"], "hello world");
        assert_eq!(v["b"], "RUST");
        assert_eq!(v["c"], 42);
        // chaining: trim then lower on an attribute
        let html = Html::parse_document(r#"<a href="  /X  " class="Big Link">t</a>"#);
        let root = html.select(&Selector::parse("a").unwrap()).next().unwrap();
        let t = parse_template("{h: . @href | trim | lower, c: . @class | lower}").unwrap();
        let v = eval(&t, root, None);
        assert_eq!(v["h"], "/x");
        assert_eq!(v["c"], "big link");
        // filters apply per-element inside arrays
        let v = eval_on_doc("{tags: [.tag | upper]}");
        assert_eq!(v["tags"][0], "RUST");
        assert_eq!(v["tags"][1], "CLI");
        // unknown filter is a parse error
        assert!(parse_template("{x: h2 | nope}").is_err());
    }

    #[test]
    fn first_number_edges() {
        assert_eq!(first_number("28"), Some(Value::Number(28.into())));
        assert_eq!(
            first_number("v2.0"),
            serde_json::Number::from_f64(2.0).map(Value::Number)
        );
        assert_eq!(
            first_number("a 10,000,000 b"),
            Some(Value::Number(10_000_000.into()))
        );
        assert_eq!(first_number("1,23"), Some(Value::Number(123.into())));
        assert_eq!(first_number("-"), None);
        assert_eq!(first_number(""), None);
    }
}
