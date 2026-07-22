use scraper::ElementRef;

/// Text content with whitespace collapsed, like what a browser would render inline.
pub fn collapsed_text(el: ElementRef) -> String {
    let mut s = String::new();
    for chunk in el.text() {
        s.push_str(chunk);
    }
    collapse_ws(&s)
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
