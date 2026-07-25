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
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
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
fn json_array_flag() {
    let (out, _, _) = cull(
        &[".post", "-j", "{t: h2}", "--array", &fixture("blog.html")],
        None,
    );
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
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
