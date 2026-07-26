//! WASM playground for cull — https://rashida-thorne.github.io/cull/playground.html
//!
//! Hand-rolled ABI (no wasm-bindgen): the page passes a UTF-8 JSON request
//! into linear memory and gets a UTF-8 JSON response back.
//!
//! Exports:
//! - `pg_alloc(len) -> ptr`          allocate a buffer the host can write into
//! - `pg_free(ptr, len)`             free a buffer previously returned/allocated
//! - `pg_run(ptr, len) -> u64`       run cull; returns (out_ptr << 32) | out_len
//!
//! Request JSON:
//! `{html, selector?, mode, arg?, remove?, base?, first?, pretty?, json_rows?, array?}`
//! where `mode` is one of `html | text | attr | json | table | md`
//! and `arg` carries the attribute name (attr) or template (json).
//!
//! Response JSON: `{ok: bool, output: string}` — on ok=false, `output` is the
//! error message (same wording as the CLI where possible).

use cull::{extract, markdown, serialize, table, text};
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use std::io::Write;

/// Allocate an exact-size buffer the host can write into.
/// Buffers are `Box<[u8]>` under the hood, so `pg_free(ptr, len)` with the
/// same `len` is always sound (no capacity bookkeeping to get wrong).
#[unsafe(no_mangle)]
pub extern "C" fn pg_alloc(len: usize) -> *mut u8 {
    let b = vec![0u8; len.max(1)].into_boxed_slice();
    Box::into_raw(b) as *mut u8
}

/// # Safety
/// `ptr` must come from `pg_alloc(len)` or a `pg_run` return with the same
/// `len`, and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pg_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        let slice = std::ptr::slice_from_raw_parts_mut(ptr, len.max(1));
        unsafe { drop(Box::from_raw(slice)) };
    }
}

/// # Safety
/// `ptr..ptr+len` must be a valid, initialized region written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pg_run(ptr: *const u8, len: usize) -> u64 {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let response = match std::str::from_utf8(input) {
        Ok(s) => run_json(s),
        Err(_) => err("request was not valid UTF-8"),
    };
    // Responses are JSON, so never empty; len.max(1) in pg_free is a no-op here.
    let b = response.into_bytes().into_boxed_slice();
    let out_len = b.len();
    let out_ptr = Box::into_raw(b) as *mut u8;
    ((out_ptr as u64) << 32) | (out_len as u64)
}

fn err(msg: impl std::fmt::Display) -> String {
    serde_json::to_string(&serde_json::json!({"ok": false, "output": msg.to_string()})).unwrap()
}

fn ok(output: String) -> String {
    serde_json::to_string(&serde_json::json!({"ok": true, "output": output})).unwrap()
}

fn run_json(request: &str) -> String {
    let req: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return err(format!("bad request JSON: {e}")),
    };
    let s = |k: &str| req.get(k).and_then(Value::as_str);
    let b = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    let Some(html) = s("html") else {
        return err("request missing `html`");
    };
    let mode = s("mode").unwrap_or("html");
    match run(
        html,
        s("selector").filter(|s| !s.trim().is_empty()),
        mode,
        s("arg"),
        s("remove").filter(|s| !s.trim().is_empty()),
        s("base").filter(|s| !s.trim().is_empty()),
        b("first"),
        b("pretty"),
        b("json_rows"),
        b("array"),
    ) {
        Ok(out) => ok(out),
        Err(e) => err(e),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    html: &str,
    selector_src: Option<&str>,
    mode: &str,
    arg: Option<&str>,
    remove: Option<&str>,
    base: Option<&str>,
    first: bool,
    pretty: bool,
    json_rows: bool,
    array: bool,
) -> Result<String, String> {
    let selector = match selector_src {
        Some(s) => Some(Selector::parse(s).map_err(|e| format!("invalid selector {s:?}: {e}"))?),
        None => None,
    };
    let remove_sel = match remove {
        Some(s) => {
            Some(Selector::parse(s).map_err(|e| format!("invalid --remove selector {s:?}: {e}"))?)
        }
        None => None,
    };

    let mut doc = Html::parse_document(html);
    if let Some(sel) = &remove_sel {
        let ids: Vec<_> = doc.select(sel).map(|el| el.id()).collect();
        for id in ids {
            if let Some(mut node) = doc.tree.get_mut(id) {
                node.detach();
            }
        }
    }
    let doc = doc;

    let matches: Vec<ElementRef> = match &selector {
        Some(sel) => doc.select(sel).collect(),
        None => vec![doc.root_element()],
    };
    let matches: Vec<ElementRef> = if first {
        matches.into_iter().take(1).collect()
    } else {
        matches
    };

    let mut out: Vec<u8> = Vec::new();
    match mode {
        "table" => {
            table::run(&matches, selector.is_some(), json_rows, pretty, &mut out)?;
        }
        "md" => {
            for m in &matches {
                let md = markdown::element_to_markdown(*m, base);
                if !md.trim().is_empty() {
                    writeln!(out, "{}", md.trim_end()).unwrap();
                }
            }
        }
        "json" => {
            let tmpl_src = arg.filter(|a| !a.trim().is_empty()).unwrap_or("{}");
            let tmpl = extract::parse_template(tmpl_src)?;
            let values: Vec<Value> = matches
                .iter()
                .map(|m| extract::eval(&tmpl, *m, base))
                .collect();
            if array {
                let v = Value::Array(values);
                writeln!(out, "{}", fmt_json(&v, pretty)).unwrap();
            } else {
                for v in values {
                    writeln!(out, "{}", fmt_json(&v, pretty)).unwrap();
                }
            }
        }
        "attr" => {
            let name = arg.map(str::trim).filter(|a| !a.is_empty());
            let Some(name) = name else {
                return Err("attr mode needs an attribute name".into());
            };
            for m in &matches {
                if let Some(v) = m.value().attr(name) {
                    writeln!(out, "{}", text::maybe_resolve(name, v, base)).unwrap();
                }
            }
        }
        "text" => {
            for m in &matches {
                let t = text::collapsed_text(*m);
                if !t.is_empty() {
                    writeln!(out, "{t}").unwrap();
                }
            }
        }
        "html" => {
            for m in &matches {
                if pretty {
                    writeln!(out, "{}", serialize::element_to_pretty_html(*m, false)).unwrap();
                } else {
                    writeln!(out, "{}", m.html()).unwrap();
                }
            }
        }
        other => return Err(format!("unknown mode {other:?}")),
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn fmt_json(v: &Value, pretty: bool) -> String {
    if pretty {
        serde_json::to_string_pretty(v).unwrap()
    } else {
        serde_json::to_string(v).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::run_json;

    fn out(req: serde_json::Value) -> (bool, String) {
        let resp: serde_json::Value = serde_json::from_str(&run_json(&req.to_string())).unwrap();
        (
            resp["ok"].as_bool().unwrap(),
            resp["output"].as_str().unwrap().to_string(),
        )
    }

    #[test]
    fn text_mode() {
        let (ok, o) = out(serde_json::json!({
            "html": "<ul><li>a</li><li>b</li></ul>", "selector": "li", "mode": "text"
        }));
        assert!(ok);
        assert_eq!(o, "a\nb\n");
    }

    #[test]
    fn json_template() {
        let (ok, o) = out(serde_json::json!({
            "html": "<div class=p><h2>T</h2><a href=/x>l</a></div>",
            "selector": ".p", "mode": "json", "arg": "{t: h2, u: a @href}",
            "base": "https://e.com"
        }));
        assert!(ok);
        assert_eq!(o, "{\"t\":\"T\",\"u\":\"https://e.com/x\"}\n");
    }

    #[test]
    fn bad_selector_reported() {
        let (ok, o) = out(serde_json::json!({
            "html": "<p>x</p>", "selector": "!!", "mode": "text"
        }));
        assert!(!ok);
        assert!(o.contains("invalid selector"));
    }

    #[test]
    fn remove_then_md() {
        let (ok, o) = out(serde_json::json!({
            "html": "<article><h1>Hi</h1><nav>skip</nav><p>body</p></article>",
            "mode": "md", "remove": "nav"
        }));
        assert!(ok);
        assert!(o.contains("# Hi") && o.contains("body") && !o.contains("skip"));
    }
}
