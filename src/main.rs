mod decode;
mod extract;
mod markdown;
mod table;
mod text;

use clap::Parser;
use scraper::{ElementRef, Html, Selector};
use std::io::{Read, Write};
use std::process::ExitCode;

/// cull — like jq, but for HTML.
///
/// Select with CSS selectors; shape into JSON, CSV, Markdown, or text.
///
/// Examples:
///   curl -s https://example.com | cull h1 -t
///   cull '.post' -j '{title: h2, url: a @href}' page.html
///   cull --table page.html
///   cull article --md https://example.com/post
#[derive(Parser, Debug)]
#[command(name = "cull", version, about, verbatim_doc_comment)]
struct Args {
    /// CSS selector (omit with --table/--md to use the whole document)
    selector: Option<String>,

    /// Input file or URL (default: stdin)
    input: Option<String>,

    /// Print collapsed text content of matches
    #[arg(short = 't', long)]
    text: bool,

    /// Print an attribute of each match
    #[arg(short = 'a', long, value_name = "NAME")]
    attr: Option<String>,

    /// Shape matches into JSON via a template, e.g. '{title: h2, url: a @href, tags: [.tag]}'
    #[arg(short = 'j', long, value_name = "TEMPLATE")]
    json: Option<String>,

    /// Extract tables as CSV (or NDJSON with --json-rows)
    #[arg(long)]
    table: bool,

    /// With --table: emit one JSON object per row, keyed by header
    #[arg(long)]
    json_rows: bool,

    /// Convert matches (or the document) to Markdown
    #[arg(long)]
    md: bool,

    /// Only output the first match
    #[arg(short = '1', long)]
    first: bool,

    /// Wrap --json output in a single JSON array instead of NDJSON
    #[arg(long)]
    array: bool,

    /// Pretty-print JSON output
    #[arg(short = 'p', long)]
    pretty: bool,

    /// Resolve relative URLs in href/src against this base
    #[arg(short = 'b', long, value_name = "URL")]
    base: Option<String>,

    /// Generate shell completions (bash, zsh, fish, elvish, powershell) and exit
    #[arg(long, value_name = "SHELL", value_enum, exclusive = true)]
    completions: Option<clap_complete::Shell>,

    /// Print a roff man page to stdout and exit (e.g. `cull --man > cull.1`)
    #[arg(long, exclusive = true)]
    man: bool,
}

fn main() -> ExitCode {
    // Die silently on SIGPIPE (e.g. `cull ... | head`), like grep and cat.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let args = Args::parse();
    if let Some(shell) = args.completions {
        let mut cmd = <Args as clap::CommandFactory>::command();
        clap_complete::generate(shell, &mut cmd, "cull", &mut std::io::stdout());
        return ExitCode::SUCCESS;
    }
    if args.man {
        let cmd = <Args as clap::CommandFactory>::command();
        let man = clap_mangen::Man::new(cmd);
        if let Err(e) = man.render(&mut std::io::stdout()) {
            eprintln!("cull: {e}");
            return ExitCode::from(2);
        }
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(found) => {
            if found {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1) // like grep: no matches
            }
        }
        Err(e) => {
            eprintln!("cull: {e}");
            ExitCode::from(2)
        }
    }
}

fn read_input(input: &Option<String>) -> Result<String, String> {
    match input.as_deref() {
        None | Some("-") => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| format!("reading stdin: {e}"))?;
            Ok(decode::decode_html(&buf, None))
        }
        Some(path) if path.starts_with("http://") || path.starts_with("https://") => fetch(path),
        Some(path) => {
            let buf = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            Ok(decode::decode_html(&buf, None))
        }
    }
}

fn fetch(url: &str) -> Result<String, String> {
    let mut resp = ureq::get(url)
        .header("User-Agent", concat!("cull/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("fetching {url}: {e}"))?;
    let header_charset = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(decode::charset_from_content_type);
    let bytes = resp
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("reading {url}: {e}"))?;
    Ok(decode::decode_html(&bytes, header_charset.as_deref()))
}

fn run(args: &Args) -> Result<bool, String> {
    // If the second positional is missing but the first looks like a file/URL
    // and a doc-level mode is on, treat the selector as the input.
    let (selector_src, input) = disambiguate(args);

    let html_src = read_input(&input)?;
    let doc = Html::parse_document(&html_src);

    // Auto-base: if input was a URL and --base not given, use it.
    let base = args.base.clone().or_else(|| {
        input
            .as_deref()
            .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
            .map(|s| s.to_string())
    });

    let matches: Vec<ElementRef> = match &selector_src {
        Some(sel_src) => {
            let sel = Selector::parse(sel_src)
                .map_err(|e| format!("invalid selector {sel_src:?}: {e}"))?;
            doc.select(&sel).collect()
        }
        None => vec![doc.root_element()],
    };

    let matches: Vec<ElementRef> = if args.first {
        matches.into_iter().take(1).collect()
    } else {
        matches
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let found = if args.table {
        table::run(
            &matches,
            selector_src.is_some(),
            args.json_rows,
            args.pretty,
            &mut out,
        )?
    } else if args.md {
        let mut any = false;
        for m in &matches {
            let md = markdown::element_to_markdown(*m, base.as_deref());
            if !md.trim().is_empty() {
                any = true;
                writeln!(out, "{}", md.trim_end()).map_err(io_err)?;
            }
        }
        any
    } else if let Some(template) = &args.json {
        let tmpl = extract::parse_template(template)?;
        let values: Vec<serde_json::Value> = matches
            .iter()
            .map(|m| extract::eval(&tmpl, *m, base.as_deref()))
            .collect();
        let any = !values.is_empty();
        if args.array {
            let v = serde_json::Value::Array(values);
            writeln!(out, "{}", fmt_json(&v, args.pretty)).map_err(io_err)?;
        } else {
            for v in values {
                writeln!(out, "{}", fmt_json(&v, args.pretty)).map_err(io_err)?;
            }
        }
        any
    } else if let Some(attr) = &args.attr {
        let mut any = false;
        for m in &matches {
            if let Some(v) = m.value().attr(attr) {
                any = true;
                let v = text::maybe_resolve(attr, v, base.as_deref());
                writeln!(out, "{v}").map_err(io_err)?;
            }
        }
        any
    } else if args.text {
        let mut any = false;
        for m in &matches {
            let t = text::collapsed_text(*m);
            if !t.is_empty() {
                any = true;
                writeln!(out, "{t}").map_err(io_err)?;
            }
        }
        any
    } else {
        let mut any = false;
        for m in &matches {
            any = true;
            writeln!(out, "{}", m.html()).map_err(io_err)?;
        }
        any
    };

    Ok(found)
}

/// `cull --md page.html` puts the input in the selector slot; fix that up.
fn disambiguate(args: &Args) -> (Option<String>, Option<String>) {
    match (&args.selector, &args.input) {
        (Some(s), None) if args.table || args.md => {
            let looks_like_input = s.starts_with("http://")
                || s.starts_with("https://")
                || s == "-"
                || std::path::Path::new(s).exists();
            if looks_like_input {
                (None, Some(s.clone()))
            } else {
                (Some(s.clone()), None)
            }
        }
        (s, i) => (s.clone(), i.clone()),
    }
}

fn fmt_json(v: &serde_json::Value, pretty: bool) -> String {
    if pretty {
        serde_json::to_string_pretty(v).unwrap()
    } else {
        serde_json::to_string(v).unwrap()
    }
}

fn io_err(e: std::io::Error) -> String {
    format!("write: {e}")
}
