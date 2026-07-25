mod decode;
mod extract;
mod markdown;
mod serialize;
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

    /// Input files or URLs (default: stdin; use - for stdin explicitly)
    inputs: Vec<String>,

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

    /// Pretty-print output: indented HTML, or indented JSON with --json/--table
    #[arg(short = 'p', long)]
    pretty: bool,

    /// Print only the number of matches (like grep -c; per input with multiple inputs)
    #[arg(short = 'c', long)]
    count: bool,

    /// Print only the names of inputs with at least one match (like grep -l)
    #[arg(short = 'l', long)]
    files_with_matches: bool,

    /// Remove nodes matching this CSS selector before selecting/output
    /// (repeatable; comma lists work: --remove 'nav, footer, script')
    #[arg(short = 'r', long = "remove", value_name = "SELECTOR")]
    remove: Vec<String>,

    /// Resolve relative URLs in href/src against this base
    #[arg(short = 'b', long, value_name = "URL")]
    base: Option<String>,

    /// Colorize HTML output: auto (default; TTY only), always, never
    #[arg(long, value_name = "WHEN", value_enum, default_value_t = ColorWhen::Auto)]
    color: ColorWhen,

    /// Generate shell completions (bash, zsh, fish, elvish, powershell) and exit
    #[arg(long, value_name = "SHELL", value_enum, exclusive = true)]
    completions: Option<clap_complete::Shell>,

    /// Print a roff man page to stdout and exit (e.g. `cull --man > cull.1`)
    #[arg(long, exclusive = true)]
    man: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

impl ColorWhen {
    fn enabled(self) -> bool {
        use std::io::IsTerminal;
        match self {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => {
                // Honor the NO_COLOR convention (any non-empty value disables).
                let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
                !no_color && std::io::stdout().is_terminal()
            }
        }
    }
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
        Ok((_, true)) => ExitCode::from(2), // some input failed to read
        Ok((true, false)) => ExitCode::SUCCESS,
        Ok((false, false)) => ExitCode::from(1), // like grep: no matches
        Err(e) => {
            eprintln!("cull: {e}");
            ExitCode::from(2)
        }
    }
}

fn read_input(input: &str) -> Result<String, String> {
    match input {
        "-" => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| format!("reading stdin: {e}"))?;
            Ok(decode::decode_html(&buf, None))
        }
        path if path.starts_with("http://") || path.starts_with("https://") => fetch(path),
        path => {
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

/// Returns (any_match_found, any_input_error).
fn run(args: &Args) -> Result<(bool, bool), String> {
    // If the first positional looks like a file/URL and a doc-level mode is
    // on, treat it as an input rather than a selector.
    let (selector_src, inputs) = disambiguate(args);

    // Parse selectors and templates once, up front (errors here are fatal).
    let selector = match &selector_src {
        Some(s) => Some(Selector::parse(s).map_err(|e| format!("invalid selector {s:?}: {e}"))?),
        None => None,
    };
    let remove_sels: Vec<Selector> = args
        .remove
        .iter()
        .map(|s| Selector::parse(s).map_err(|e| format!("invalid --remove selector {s:?}: {e}")))
        .collect::<Result<_, _>>()?;
    let json_tmpl = match &args.json {
        Some(t) => Some(extract::parse_template(t)?),
        None => None,
    };

    let inputs: Vec<String> = if inputs.is_empty() {
        vec!["-".to_string()]
    } else {
        inputs
    };
    let multi = inputs.len() > 1;
    let colorize = args.color.enabled();

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut found_any = false;
    let mut had_error = false;
    // With --json --array, matches from all inputs merge into one array.
    let mut array_acc: Vec<serde_json::Value> = Vec::new();

    for input in &inputs {
        let html_src = match read_input(input) {
            Ok(s) => s,
            Err(e) => {
                // Like grep: report, keep going, exit 2 at the end.
                eprintln!("cull: {e}");
                had_error = true;
                continue;
            }
        };
        let mut doc = Html::parse_document(&html_src);

        // --remove: detach matching nodes before any selection or conversion.
        for sel in &remove_sels {
            let ids: Vec<_> = doc.select(sel).map(|el| el.id()).collect();
            for id in ids {
                if let Some(mut node) = doc.tree.get_mut(id) {
                    node.detach();
                }
            }
        }
        let doc = doc;

        // Auto-base: if this input is a URL and --base not given, use it.
        let base = args.base.clone().or_else(|| {
            (input.starts_with("http://") || input.starts_with("https://")).then(|| input.clone())
        });

        let matches: Vec<ElementRef> = match &selector {
            Some(sel) => doc.select(sel).collect(),
            None => vec![doc.root_element()],
        };

        // --first applies per input, like grep -m1.
        let matches: Vec<ElementRef> = if args.first {
            matches.into_iter().take(1).collect()
        } else {
            matches
        };

        if args.files_with_matches {
            if !matches.is_empty() {
                found_any = true;
                writeln!(out, "{input}").map_err(io_err)?;
            }
            continue;
        }

        if args.count {
            if multi {
                writeln!(out, "{input}:{}", matches.len()).map_err(io_err)?;
            } else {
                writeln!(out, "{}", matches.len()).map_err(io_err)?;
            }
            found_any |= !matches.is_empty();
            continue;
        }

        let found = if args.table {
            table::run(
                &matches,
                selector.is_some(),
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
        } else if let Some(tmpl) = &json_tmpl {
            let values: Vec<serde_json::Value> = matches
                .iter()
                .map(|m| extract::eval(tmpl, *m, base.as_deref()))
                .collect();
            let any = !values.is_empty();
            if args.array {
                array_acc.extend(values);
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
                if args.pretty {
                    writeln!(out, "{}", serialize::element_to_pretty_html(*m, colorize))
                        .map_err(io_err)?;
                } else if colorize {
                    writeln!(out, "{}", serialize::element_to_colored_html(*m)).map_err(io_err)?;
                } else {
                    writeln!(out, "{}", m.html()).map_err(io_err)?;
                }
            }
            any
        };
        found_any |= found;
    }

    if json_tmpl.is_some() && args.array && !args.count && !args.files_with_matches {
        let v = serde_json::Value::Array(array_acc);
        writeln!(out, "{}", fmt_json(&v, args.pretty)).map_err(io_err)?;
    }

    Ok((found_any, had_error))
}

/// `cull --md page.html [more.html ...]` puts an input in the selector slot;
/// fix that up.
fn disambiguate(args: &Args) -> (Option<String>, Vec<String>) {
    match &args.selector {
        Some(s) if args.table || args.md => {
            let looks_like_input = s.starts_with("http://")
                || s.starts_with("https://")
                || s == "-"
                || std::path::Path::new(s).exists();
            if looks_like_input {
                let mut inputs = vec![s.clone()];
                inputs.extend(args.inputs.iter().cloned());
                (None, inputs)
            } else {
                (Some(s.clone()), args.inputs.clone())
            }
        }
        s => (s.clone(), args.inputs.clone()),
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
