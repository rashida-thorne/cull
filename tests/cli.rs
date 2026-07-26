//! Integration tests: run the built binary against fixture HTML.

use std::io::Write;
use std::process::{Command, Stdio};

fn cull(args: &[&str], stdin: Option<&str>) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cull"));
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn cull");
    if let Some(input) = stdin {
        // Tolerate EPIPE: if the child exits before reading stdin (e.g. an
        // invalid-selector error), the write racing against that exit is fine.
        match child.stdin.as_mut().unwrap().write_all(input.as_bytes()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(e) => panic!("write to child stdin: {e}"),
        }
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("wait");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn text_mode() {
    let (out, _, code) = cull(&[".post h2", "-t", &fixture("blog.html")], None);
    assert_eq!(out, "Hello, world\nOn tables\n");
    assert_eq!(code, 0);
}

#[test]
fn stdin_input() {
    let (out, _, code) = cull(&["p", "-t"], Some("<p>hi</p><p>there</p>"));
    assert_eq!(out, "hi\nthere\n");
    assert_eq!(code, 0);
}

#[test]
fn attr_with_base() {
    let (out, _, _) = cull(
        &[
            ".post a",
            "-a",
            "href",
            "-b",
            "https://ex.com",
            &fixture("blog.html"),
        ],
        None,
    );
    assert_eq!(
        out,
        "https://ex.com/posts/hello\nhttps://ex.com/posts/tables\n"
    );
}

#[test]
fn json_template_ndjson() {
    let (out, _, _) = cull(
        &[
            ".post",
            "-j",
            "{title: h2, tags: [.tag]}",
            &fixture("blog.html"),
        ],
        None,
    );
    let lines: Vec<&str> = out.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["title"], "Hello, world");
    assert_eq!(v["tags"], serde_json::json!(["rust", "intro"]));
}

#[test]
fn json_nested_objects_and_filters() {
    let html = r#"<div class="post"><h2> Post A </h2>
        <span class="c"><b class="u">Joe</b><i class="t">hi</i></span>
        <span class="c"><b class="u">Amy</b><i class="t">yo</i></span>
        <a class="more" href="  /next  ">1,234 comments</a></div>"#;
    let (out, _, _) = cull(
        &[
            ".post",
            "-j",
            "{title: h2 | upper, n: .more | num, href: .more @href | trim, \
             comments: [.c {user: .u | lower, text: .t}], first: .c {user: .u}}",
        ],
        Some(html),
    );
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["title"], "POST A");
    assert_eq!(v["n"], 1234);
    assert_eq!(v["href"], "/next");
    assert_eq!(
        v["comments"],
        serde_json::json!([
            {"user": "joe", "text": "hi"},
            {"user": "amy", "text": "yo"}
        ])
    );
    assert_eq!(v["first"]["user"], "Joe");
}

#[test]
fn json_array_flag() {
    let (out, _, _) = cull(
        &[".post", "-j", "{t: h2}", "--array", &fixture("blog.html")],
        None,
    );
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn json_nodes_ndjson() {
    let (out, _, code) = cull(&[".post h2 a", "--json-nodes", &fixture("blog.html")], None);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "tag": "a",
            "attrs": {"href": "/posts/hello"},
            "text": "Hello, world",
            "children": ["Hello, world"]
        })
    );
}

#[test]
fn json_nodes_array_pretty_and_base() {
    let (out, _, _) = cull(
        &[
            ".post h2 a",
            "--json-nodes",
            "--array",
            "--pretty",
            "-b",
            "https://blog.example",
            &fixture("blog.html"),
        ],
        None,
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["attrs"]["href"], "https://blog.example/posts/hello");
    assert!(out.contains("\n  "), "--pretty should indent");
}

#[test]
fn json_nodes_no_match_exit_1() {
    let (out, _, code) = cull(&["nope", "--json-nodes", &fixture("blog.html")], None);
    assert_eq!(out, "");
    assert_eq!(code, 1);
}

#[test]
fn table_csv() {
    let (out, _, _) = cull(&["--table", &fixture("blog.html")], None);
    assert_eq!(out, "Item,Price\n\"Apple, red\",$1\nPear,$2\n");
}

#[test]
fn table_json_rows() {
    let (out, _, _) = cull(&["--table", "--json-rows", &fixture("blog.html")], None);
    let first: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(first["Item"], "Apple, red");
    assert_eq!(first["Price"], "$1");
}

#[test]
fn markdown_mode() {
    let (out, _, _) = cull(&["--md", &fixture("blog.html")], None);
    assert!(out.contains("## [Hello, world](/posts/hello)"));
    assert!(out.contains("| Item | Price |"));
    assert!(out.contains("*Exciting*"));
}

#[test]
fn first_flag() {
    let (out, _, _) = cull(&[".post h2", "-t", "-1", &fixture("blog.html")], None);
    assert_eq!(out, "Hello, world\n");
}

#[test]
fn default_output_is_html() {
    let (out, _, _) = cull(&[".post h2", "-1", &fixture("blog.html")], None);
    assert!(out.starts_with("<h2>"));
}

#[test]
fn piped_output_has_no_ansi_by_default() {
    // stdout is a pipe in tests, so `auto` must not colorize.
    let (out, _, _) = cull(&[".post h2", "-1", &fixture("blog.html")], None);
    assert!(!out.contains('\x1b'));
}

#[test]
fn color_always_emits_ansi() {
    let (out, _, code) = cull(
        &[".post h2", "-1", "--color", "always", &fixture("blog.html")],
        None,
    );
    assert_eq!(code, 0);
    assert!(out.contains("\x1b[1;34mh2"), "tag name colored: {out:?}");
    assert!(out.contains("\x1b[0m"));
}

#[test]
fn color_never_suppresses_ansi() {
    let (out, _, _) = cull(
        &[".post h2", "-1", "--color", "never", &fixture("blog.html")],
        None,
    );
    assert!(!out.contains('\x1b'));
}

#[test]
fn no_match_exits_1() {
    let (out, _, code) = cull(&[".does-not-exist", "-t", &fixture("blog.html")], None);
    assert_eq!(out, "");
    assert_eq!(code, 1);
}

#[test]
fn bad_selector_exits_2() {
    let (_, err, code) = cull(&["p[", "-t", &fixture("blog.html")], None);
    assert_eq!(code, 2);
    assert!(err.contains("invalid selector"));
}

#[test]
fn bad_template_exits_2() {
    let (_, err, code) = cull(&["p", "-j", "{a:", &fixture("blog.html")], None);
    assert_eq!(code, 2);
    assert!(err.contains("template"));
}

#[test]
fn missing_file_exits_2() {
    let (_, _, code) = cull(&["p", "-t", "/no/such/file.html"], None);
    assert_eq!(code, 2);
}

#[test]
fn completions_generate() {
    let (out, _, code) = cull(&["--completions", "bash"], None);
    assert_eq!(code, 0);
    assert!(out.contains("_cull"));
}

#[test]
fn man_page_generates() {
    let (out, _, code) = cull(&["--man"], None);
    assert_eq!(code, 0);
    assert!(out.starts_with(".ie") || out.starts_with(".TH"));
    assert!(out.contains(".SH NAME"));
    assert!(out.contains("cull"));
}

#[test]
fn decodes_windows_1251_file_via_meta_charset() {
    let (out, _, code) = cull(&["p.msg", "-t", "tests/fixtures/cp1251.html"], None);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "привет мир");
}

#[test]
fn remove_strips_nodes_before_md() {
    let html = "<body><nav>MENU</nav><article><h1>T</h1><script>x()</script><p>Body</p></article><footer>F</footer></body>";
    let (out, _, code) = cull(
        &["--md", "--remove", "nav, footer, script", "-"],
        Some(html),
    );
    assert_eq!(out, "# T\n\nBody\n");
    assert_eq!(code, 0);
}

#[test]
fn remove_applies_before_selection() {
    let html = "<div class=a><span class=x>1</span><span>2</span></div>";
    let (out, _, code) = cull(&[".a", "-t", "-r", ".x"], Some(html));
    assert_eq!(out, "2\n");
    assert_eq!(code, 0);
}

#[test]
fn remove_is_repeatable() {
    let html = "<p class=a>1</p><p class=b>2</p><p>3</p>";
    let (out, _, _) = cull(&["p", "-t", "-r", ".a", "-r", ".b"], Some(html));
    assert_eq!(out, "3\n");
}

#[test]
fn remove_can_empty_the_match_set() {
    let (out, _, code) = cull(&[".a", "-t", "-r", ".a"], Some("<div class=a>x</div>"));
    assert_eq!(out, "");
    assert_eq!(code, 1); // grep-like: no matches
}

#[test]
fn remove_invalid_selector_errors() {
    let (_, err, code) = cull(&["p", "-t", "-r", "[[["], Some("<p>x</p>"));
    assert!(err.contains("--remove"));
    assert_eq!(code, 2);
}

#[test]
fn pretty_html_is_indented() {
    let html = "<div id=x><ul><li>one</li><li>two</li></ul></div>";
    let (out, _, code) = cull(&["div", "--pretty", "--color", "never"], Some(html));
    assert_eq!(
        out,
        "<div id=\"x\">\n  <ul>\n    <li>one</li>\n    <li>two</li>\n  </ul>\n</div>\n"
    );
    assert_eq!(code, 0);
}

#[test]
fn pretty_html_preserves_pre() {
    let html = "<div><pre>a\n  b</pre></div>";
    let (out, _, _) = cull(&["div", "-p", "--color", "never"], Some(html));
    assert_eq!(out, "<div>\n  <pre>a\n  b</pre>\n</div>\n");
}

#[test]
fn pretty_still_means_pretty_json_with_json() {
    let (out, _, _) = cull(&["div", "-j", "{t: p}", "-p"], Some("<div><p>x</p></div>"));
    assert!(out.contains("{\n"));
    assert!(out.contains("\"t\": \"x\""));
}

#[test]
fn count_prints_number_of_matches() {
    let (out, _, code) = cull(&["li", "--count"], Some("<ul><li>a<li>b<li>c</ul>"));
    assert_eq!(out, "3\n");
    assert_eq!(code, 0);
}

#[test]
fn count_zero_exits_one() {
    let (out, _, code) = cull(&["p", "-c"], Some("<div>x</div>"));
    assert_eq!(out, "0\n");
    assert_eq!(code, 1);
}

#[test]
fn count_respects_remove() {
    let html = "<p class=a>1</p><p>2</p>";
    let (out, _, _) = cull(&["p", "-c", "-r", ".a"], Some(html));
    assert_eq!(out, "1\n");
}

#[test]
fn multiple_inputs_concatenate_output() {
    let (out, _, code) = cull(
        &[
            "a",
            "-t",
            &fixture("multi_a.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(out, "A\nB\nC\n");
    assert_eq!(code, 0);
}

#[test]
fn multiple_inputs_count_per_file() {
    let (out, _, code) = cull(
        &[
            "a",
            "-c",
            &fixture("multi_a.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(
        out,
        format!(
            "{}:1\n{}:2\n",
            fixture("multi_a.html"),
            fixture("multi_b.html")
        )
    );
    assert_eq!(code, 0);
}

#[test]
fn single_input_count_stays_bare() {
    let (out, _, code) = cull(&["a", "-c", &fixture("multi_b.html")], None);
    assert_eq!(out, "2\n");
    assert_eq!(code, 0);
}

#[test]
fn files_with_matches_lists_only_matching_inputs() {
    let (out, _, code) = cull(
        &[
            "div a",
            "-l",
            &fixture("multi_a.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(out, format!("{}\n", fixture("multi_b.html")));
    assert_eq!(code, 0);
}

#[test]
fn files_with_matches_no_match_exits_one() {
    let (out, _, code) = cull(
        &[
            "video",
            "-l",
            &fixture("multi_a.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(out, "");
    assert_eq!(code, 1);
}

#[test]
fn first_applies_per_input() {
    let (out, _, code) = cull(
        &[
            "a",
            "-t",
            "-1",
            &fixture("multi_a.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(out, "A\nB\n");
    assert_eq!(code, 0);
}

#[test]
fn json_array_merges_across_inputs() {
    let (out, _, code) = cull(
        &[
            "a",
            "-j",
            "{u: @href}",
            "--array",
            &fixture("multi_a.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(out, "[{\"u\":\"/a\"},{\"u\":\"/b\"},{\"u\":\"/c\"}]\n");
    assert_eq!(code, 0);
}

#[test]
fn unreadable_input_reports_continues_and_exits_two() {
    let (out, err, code) = cull(
        &[
            "a",
            "-t",
            &fixture("multi_a.html"),
            &fixture("no_such_file.html"),
            &fixture("multi_b.html"),
        ],
        None,
    );
    assert_eq!(out, "A\nB\nC\n");
    assert!(err.contains("no_such_file.html"));
    assert_eq!(code, 2);
}

#[test]
fn md_mode_accepts_multiple_bare_inputs() {
    // Both positionals look like files; --md must treat them all as inputs.
    let (out, _, code) = cull(
        &["--md", &fixture("multi_a.html"), &fixture("multi_b.html")],
        None,
    );
    assert_eq!(out, "- [A](/a)\n[B](/b)[C](/c)\n");
    assert_eq!(code, 0);
}

#[test]
fn stdin_dash_mixes_with_files() {
    let (out, _, code) = cull(
        &["a", "-t", "-", &fixture("multi_b.html")],
        Some("<a href=x>S</a>"),
    );
    assert_eq!(out, "S\nB\nC\n");
    assert_eq!(code, 0);
}

/// Spawn a one-shot local HTTP server that returns `body` as text/html and
/// hands back (url, join-handle yielding the raw request head it received).
fn one_shot_server(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 4096];
        let mut req = String::new();
        loop {
            let n = stream.read(&mut buf).expect("read");
            req.push_str(&String::from_utf8_lossy(&buf[..n]));
            if n == 0 || req.contains("\r\n\r\n") {
                break;
            }
        }
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(resp.as_bytes()).expect("write");
        req
    });
    (url, handle)
}

#[test]
fn fetch_sends_custom_headers_and_ua_override() {
    let (url, server) = one_shot_server("<p>fetched</p>");
    let (out, _, code) = cull(
        &[
            "p",
            "-t",
            "-H",
            "User-Agent: test-agent/9.9",
            "-H",
            "X-Cull-Test: yes",
            &url,
        ],
        None,
    );
    assert_eq!(out, "fetched\n");
    assert_eq!(code, 0);
    let req = server.join().unwrap().to_ascii_lowercase();
    assert!(req.contains("user-agent: test-agent/9.9"), "req was: {req}");
    assert!(req.contains("x-cull-test: yes"), "req was: {req}");
    // Default UA must not also be sent when overridden.
    assert!(!req.contains("cull/"), "req was: {req}");
}

#[test]
fn fetch_sends_default_ua_when_not_overridden() {
    let (url, server) = one_shot_server("<p>hi</p>");
    let (out, _, code) = cull(&["p", "-t", &url], None);
    assert_eq!(out, "hi\n");
    assert_eq!(code, 0);
    let req = server.join().unwrap().to_ascii_lowercase();
    assert!(req.contains("user-agent: cull/"), "req was: {req}");
}

#[test]
fn invalid_header_is_fatal() {
    let (_, err, code) = cull(&["p", "-t", "-H", "NoColon", "http://127.0.0.1:1/"], None);
    assert_eq!(code, 2);
    assert!(err.contains("invalid --header"), "stderr was: {err}");
}

// --- v0.7.0: --inner, block-aware -t, --has-text ---

#[test]
fn inner_html_mode() {
    let html = r#"<div id="box"><p>one</p><p>two</p></div>"#;
    let (out, _, code) = cull(&["#box", "-i"], Some(html));
    assert_eq!(out, "<p>one</p><p>two</p>\n");
    assert_eq!(code, 0);
}

#[test]
fn inner_html_pretty() {
    let html = r#"<div id="box"><p>one</p><p>two</p></div>"#;
    let (out, _, code) = cull(&["#box", "-i", "-p", "--color", "never"], Some(html));
    assert_eq!(out, "<p>one</p>\n<p>two</p>\n");
    assert_eq!(code, 0);
}

#[test]
fn inner_html_text_only_child() {
    let (out, _, code) = cull(&["b", "-i"], Some("<p><b>just &amp; text</b></p>"));
    assert_eq!(out, "just &amp; text\n");
    assert_eq!(code, 0);
}

#[test]
fn text_mode_br_and_blocks() {
    let html = "<div id=a>line one<br>line two<p>para</p></div>";
    let (out, _, code) = cull(&["#a", "-t"], Some(html));
    assert_eq!(out, "line one\nline two\npara\n");
    assert_eq!(code, 0);
}

#[test]
fn text_mode_pre_verbatim() {
    let html = "<div id=a><p>intro</p><pre>  indented\n  code</pre></div>";
    let (out, _, code) = cull(&["#a", "-t"], Some(html));
    assert_eq!(out, "intro\n  indented\n  code\n");
    assert_eq!(code, 0);
}

#[test]
fn has_text_filters_matches() {
    let html = "<ul><li>rust tool</li><li>go tool</li><li>rust lib</li></ul>";
    let (out, _, code) = cull(&["li", "--has-text", "rust", "-t"], Some(html));
    assert_eq!(out, "rust tool\nrust lib\n");
    assert_eq!(code, 0);
}

#[test]
fn has_text_multiple_are_anded() {
    let html = "<ul><li>rust tool</li><li>go tool</li><li>rust lib</li></ul>";
    let (out, _, code) = cull(
        &["li", "--has-text", "rust", "--has-text", "lib", "-t"],
        Some(html),
    );
    assert_eq!(out, "rust lib\n");
    assert_eq!(code, 0);
}

#[test]
fn has_text_no_match_exits_1() {
    let (out, _, code) = cull(&["li", "--has-text", "zig", "-t"], Some("<li>rust</li>"));
    assert_eq!(out, "");
    assert_eq!(code, 1);
}

#[test]
fn has_text_crosses_inline_tags() {
    // The needle spans an inline tag boundary; collapsed text still matches.
    let html = "<p>price: <b>42</b> dollars</p>";
    let (out, _, code) = cull(&["p", "--has-text", "price: 42", "-t"], Some(html));
    assert_eq!(out, "price: 42 dollars\n");
    assert_eq!(code, 0);
}

#[test]
fn has_text_with_count() {
    let html = "<ul><li>rust tool</li><li>go tool</li><li>rust lib</li></ul>";
    let (out, _, code) = cull(&["li", "--has-text", "rust", "-c"], Some(html));
    assert_eq!(out, "2\n");
    assert_eq!(code, 0);
}

// --- modern selector support (htmlq panics on these; pup rejects them) ---

#[test]
fn selector_has() {
    let html = r#"<div class="a"><p>x</p></div><div class="b">y</div>"#;
    let (out, _, code) = cull(&["div:has(p)"], Some(html));
    assert_eq!(out, "<div class=\"a\"><p>x</p></div>\n");
    assert_eq!(code, 0);
}

#[test]
fn selector_not_has() {
    let html = r#"<div class="a"><p>x</p></div><div class="b">y</div>"#;
    let (out, _, code) = cull(&["div:not(:has(p))"], Some(html));
    assert_eq!(out, "<div class=\"b\">y</div>\n");
    assert_eq!(code, 0);
}

#[test]
fn selector_is_and_where() {
    let html = r#"<div class="a"><p>x</p></div><section><p>y</p></section><aside><p>z</p></aside>"#;
    let (out, _, code) = cull(&["p:is(.a p, section p)", "-t"], Some(html));
    assert_eq!(out, "x\ny\n");
    assert_eq!(code, 0);
    let (out, _, code) = cull(&[":where(section, aside) p", "-t"], Some(html));
    assert_eq!(out, "y\nz\n");
    assert_eq!(code, 0);
}

#[test]
fn selector_list_comma() {
    let html = r#"<span>s</span><div class="b">y</div><em>e</em>"#;
    let (out, _, code) = cull(&["span, .b", "-t"], Some(html));
    assert_eq!(out, "s\ny\n");
    assert_eq!(code, 0);
}

// ---- XML mode ----

#[test]
fn xml_autodetect_rss_links() {
    // The headline fix: html5ever parses <link> as void and loses the URL;
    // XML mode (auto-detected from the <?xml decl) keeps it.
    let (out, err, code) = cull(&["item > link", "-t", &fixture("feed.xml")], None);
    assert_eq!(
        out,
        "https://example.com/posts/1\nhttps://example.com/posts/2\n"
    );
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn xml_case_sensitive_selectors() {
    let (out, _, code) = cull(&["pubDate", "-c", &fixture("feed.xml")], None);
    assert_eq!(out, "2\n");
    assert_eq!(code, 0);
    // XML selectors are case-sensitive: lowercase matches nothing (exit 1).
    let (out, _, code) = cull(&["pubdate", "-c", &fixture("feed.xml")], None);
    assert_eq!(out, "0\n");
    assert_eq!(code, 1);
}

#[test]
fn xml_flag_forces_parsing_from_stdin() {
    // No <?xml decl and an unknown root: auto-detection stays off, --xml forces.
    let src = "<things><name>a</name><name>b</name></things>";
    let (out, _, code) = cull(&["name", "-t", "--xml"], Some(src));
    assert_eq!(out, "a\nb\n");
    assert_eq!(code, 0);
}

#[test]
fn html_flag_forces_html_parsing() {
    let src = "<?xml version=\"1.0\"?><rss><item><link>https://x.com/1</link></item></rss>";
    let (out, _, code) = cull(&["item > link", "-t", "--html"], Some(src));
    // HTML parsing mangles <link> into a void element: no text.
    assert_eq!(out, "");
    assert_eq!(code, 1);
}

#[test]
fn xml_template_over_feed() {
    let (out, _, code) = cull(
        &[
            "item",
            "-j",
            "{title: title, url: link, date: pubDate}",
            &fixture("feed.xml"),
        ],
        None,
    );
    let first: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(first["title"], "First & Foremost");
    assert_eq!(first["url"], "https://example.com/posts/1");
    assert_eq!(first["date"], "Mon, 01 Jan 2026 00:00:00 GMT");
    assert_eq!(code, 0);
}

#[test]
fn xml_explicit_flag_malformed_is_error() {
    let (out, err, code) = cull(&["a", "--xml"], Some("<a><b></a>"));
    assert_eq!(out, "");
    assert!(err.contains("XML parse error"), "stderr was: {err}");
    assert_eq!(code, 2);
}

#[test]
fn xml_autodetect_falls_back_to_html_on_broken_xml() {
    let src = "<?xml version=\"1.0\"?><rss><item>unclosed";
    let (out, err, code) = cull(&["item", "-t"], Some(src));
    assert!(err.contains("parsing as HTML"), "stderr was: {err}");
    assert_eq!(out, "unclosed\n");
    assert_eq!(code, 0);
}

#[test]
fn xml_pretty_serializes_link_with_children() {
    let (out, _, code) = cull(&["item", "-p", "-1", &fixture("feed.xml")], None);
    assert!(
        out.contains("<link>https://example.com/posts/1</link>"),
        "output was: {out}"
    );
    assert_eq!(code, 0);
}

#[test]
fn xml_and_html_inputs_mix() {
    // Per-input sniffing: one HTML file, one XML feed in the same run.
    let (out, _, code) = cull(
        &[
            "h2, item > title",
            "-t",
            &fixture("blog.html"),
            &fixture("feed.xml"),
        ],
        None,
    );
    assert_eq!(out, "Hello, world\nOn tables\nFirst & Foremost\nSecond\n");
    assert_eq!(code, 0);
}

#[test]
fn xml_sitemap_locs() {
    let sitemap = "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
        <url><loc>https://example.com/a</loc></url>\
        <url><loc>https://example.com/b</loc></url></urlset>";
    let (out, _, code) = cull(&["url > loc", "-t"], Some(sitemap));
    assert_eq!(out, "https://example.com/a\nhttps://example.com/b\n");
    assert_eq!(code, 0);
}
