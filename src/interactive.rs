//! `-I` — interactive selector mode.
//!
//! A small live-preview TUI: edit the CSS selector, watch the output update,
//! Tab cycles the output shape, Enter prints the current result to stdout
//! and exits. All drawing happens on stderr (and keys are read from the
//! terminal), so the printed result still pipes cleanly:
//!
//! ```text
//! curl -s https://example.com | cull -I | less
//! ```

use crate::{Args, emit, fmt_json};
use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal,
};
use cull::extract;
use scraper::{ElementRef, Html, Selector};
use std::io::Write;

const PROMPT: &str = "cull> ";

/// Output shapes reachable with Tab. `AsGiven` is whatever the command-line
/// flags asked for; the rest override them for a quick look.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    AsGiven,
    Html,
    Text,
    Markdown,
    JsonNodes,
}

impl Mode {
    fn label(self, args: &Args) -> &'static str {
        match self {
            Mode::AsGiven if !shaped(args) => "html",
            Mode::AsGiven => "as given",
            Mode::Html => "html",
            Mode::Text => "text",
            Mode::Markdown => "markdown",
            Mode::JsonNodes => "json nodes",
        }
    }
}

/// Do the command-line flags already pick an output shape?
fn shaped(args: &Args) -> bool {
    args.text
        || args.inner
        || args.attr.is_some()
        || args.json.is_some()
        || args.json_nodes
        || args.table
        || args.md
}

/// The Tab cycle. When the flags already shape the output, plain HTML is
/// added as an extra stop so it stays reachable.
fn mode_cycle(args: &Args) -> Vec<Mode> {
    if shaped(args) {
        vec![
            Mode::AsGiven,
            Mode::Html,
            Mode::Text,
            Mode::Markdown,
            Mode::JsonNodes,
        ]
    } else {
        vec![Mode::AsGiven, Mode::Text, Mode::Markdown, Mode::JsonNodes]
    }
}

/// Clone `args` with output flags overridden for the given mode.
fn mode_args(args: &Args, mode: Mode) -> Args {
    let mut a = args.clone();
    if mode == Mode::AsGiven {
        return a;
    }
    a.text = false;
    a.inner = false;
    a.attr = None;
    a.json = None;
    a.json_nodes = false;
    a.table = false;
    a.json_rows = false;
    a.md = false;
    match mode {
        Mode::Text => a.text = true,
        Mode::Markdown => a.md = true,
        Mode::JsonNodes => a.json_nodes = true,
        Mode::Html | Mode::AsGiven => {}
    }
    a
}

/// Truncate a line to `width` visible columns, treating ANSI SGR escape
/// sequences as zero-width and re-appending a reset if any were kept.
fn truncate_ansi(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut visible = 0usize;
    let mut saw_ansi = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            saw_ansi = true;
            out.push(c);
            for e in chars.by_ref() {
                out.push(e);
                if e == 'm' {
                    break;
                }
            }
            continue;
        }
        if visible >= width {
            // Keep consuming only to notice trailing ANSI resets; simpler to
            // just stop and reset below.
            break;
        }
        // Tabs render unpredictably in raw mode; make them spaces.
        if c == '\t' {
            out.push(' ');
        } else {
            out.push(c);
        }
        visible += 1;
    }
    if saw_ansi {
        out.push_str("\x1b[0m");
    }
    out
}

/// Compute matches for a selector string against the document, honoring
/// --has-text and --first. Empty selector = whole document.
fn compute_matches<'a>(
    doc: &'a Html,
    sel_src: &str,
    args: &Args,
) -> Result<Vec<ElementRef<'a>>, String> {
    let matches: Vec<ElementRef> = if sel_src.trim().is_empty() {
        vec![doc.root_element()]
    } else {
        let sel = Selector::parse(sel_src).map_err(|e| format!("{e}"))?;
        doc.select(&sel).collect()
    };
    let matches: Vec<ElementRef> = if args.has_text.is_empty() {
        matches
    } else {
        matches
            .into_iter()
            .filter(|m| {
                let t = cull::text::collapsed_text(*m);
                args.has_text.iter().all(|needle| t.contains(needle))
            })
            .collect()
    };
    Ok(if args.first {
        matches.into_iter().take(1).collect()
    } else {
        matches
    })
}

/// Render matches to a string with the given mode-args (handles --array).
fn render(
    args: &Args,
    matches: &[ElementRef],
    selector_present: bool,
    base: Option<&str>,
    json_tmpl: Option<&extract::Template>,
    colorize: bool,
) -> Result<(String, bool), String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut array_acc: Vec<serde_json::Value> = Vec::new();
    let tmpl = if args.json.is_some() { json_tmpl } else { None };
    let found = emit(
        args,
        matches,
        selector_present,
        base,
        tmpl,
        colorize,
        &mut array_acc,
        &mut buf,
    )?;
    if (tmpl.is_some() || args.json_nodes) && args.array {
        let v = serde_json::Value::Array(array_acc);
        let line = format!("{}\n", fmt_json(&v, args.pretty));
        buf.extend_from_slice(line.as_bytes());
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), found))
}

struct State {
    sel: Vec<char>,
    cursor: usize,
    scroll: usize,
    mode_ix: usize,
    preview: Vec<String>,
    found: bool,
    count: usize,
    error: Option<String>,
}

enum Outcome {
    Print,
    Quit,
}

pub fn run(
    args: &Args,
    doc: &Html,
    initial_selector: &str,
    input_name: &str,
    base: Option<&str>,
    json_tmpl: Option<&extract::Template>,
) -> Result<(bool, bool), String> {
    let modes = mode_cycle(args);
    // Preview color: honor --color=never and NO_COLOR, otherwise on (the
    // preview is always a terminal).
    let preview_color = match args.color {
        crate::ColorWhen::Never => false,
        _ => !std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
    };

    let mut st = State {
        sel: initial_selector.chars().collect(),
        cursor: initial_selector.chars().count(),
        scroll: 0,
        mode_ix: 0,
        preview: Vec::new(),
        found: false,
        count: 0,
        error: None,
    };

    let recompute = |st: &mut State| {
        let sel_src: String = st.sel.iter().collect();
        match compute_matches(doc, &sel_src, args) {
            Ok(matches) => {
                st.count = matches.len();
                let margs = mode_args(args, modes[st.mode_ix]);
                match render(
                    &margs,
                    &matches,
                    !sel_src.trim().is_empty(),
                    base,
                    json_tmpl,
                    preview_color,
                ) {
                    Ok((text, found)) => {
                        st.preview = text.lines().map(str::to_owned).collect();
                        st.found = found;
                        st.error = None;
                    }
                    Err(e) => st.error = Some(e),
                }
            }
            Err(e) => {
                st.error = Some(format!("invalid selector: {e}"));
            }
        }
    };
    recompute(&mut st);

    let mut tty = std::io::stderr();
    terminal::enable_raw_mode().map_err(|e| format!("--interactive needs a terminal: {e}"))?;
    let _ = execute!(tty, terminal::EnterAlternateScreen, cursor::Hide);

    let outcome = event_loop(&mut tty, &mut st, args, &modes, input_name, recompute);

    let _ = execute!(tty, terminal::LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();

    match outcome? {
        Outcome::Quit => Ok((false, false)),
        Outcome::Print => {
            // Re-render for stdout with real color rules.
            let sel_src: String = st.sel.iter().collect();
            let matches = compute_matches(doc, &sel_src, args)?;
            let margs = mode_args(args, modes[st.mode_ix]);
            let colorize = margs.color.enabled();
            let (text, found) = render(
                &margs,
                &matches,
                !sel_src.trim().is_empty(),
                base,
                json_tmpl,
                colorize,
            )?;
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            out.write_all(text.as_bytes())
                .map_err(|e| format!("write: {e}"))?;
            Ok((found, false))
        }
    }
}

fn event_loop(
    tty: &mut std::io::Stderr,
    st: &mut State,
    args: &Args,
    modes: &[Mode],
    input_name: &str,
    recompute: impl Fn(&mut State),
) -> Result<Outcome, String> {
    loop {
        draw(tty, st, args, modes, input_name).map_err(|e| format!("draw: {e}"))?;
        let ev = event::read().map_err(|e| format!("read key: {e}"))?;
        match ev {
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind: KeyEventKind::Press,
                ..
            }) => {
                let ctrl = modifiers.contains(KeyModifiers::CONTROL);
                match (code, ctrl) {
                    (KeyCode::Enter, _) => {
                        if st.error.is_none() {
                            return Ok(Outcome::Print);
                        }
                    }
                    (KeyCode::Esc, _) | (KeyCode::Char('c'), true) | (KeyCode::Char('d'), true) => {
                        return Ok(Outcome::Quit);
                    }
                    (KeyCode::Tab, _) => {
                        st.mode_ix = (st.mode_ix + 1) % modes.len();
                        st.scroll = 0;
                        recompute(st);
                    }
                    (KeyCode::BackTab, _) => {
                        st.mode_ix = (st.mode_ix + modes.len() - 1) % modes.len();
                        st.scroll = 0;
                        recompute(st);
                    }
                    (KeyCode::Char('u'), true) => {
                        st.sel.clear();
                        st.cursor = 0;
                        st.scroll = 0;
                        recompute(st);
                    }
                    (KeyCode::Char('w'), true) => {
                        let mut i = st.cursor;
                        while i > 0 && st.sel[i - 1] == ' ' {
                            i -= 1;
                        }
                        while i > 0 && st.sel[i - 1] != ' ' {
                            i -= 1;
                        }
                        st.sel.drain(i..st.cursor);
                        st.cursor = i;
                        recompute(st);
                    }
                    (KeyCode::Char('a'), true) | (KeyCode::Home, _) => st.cursor = 0,
                    (KeyCode::Char('e'), true) | (KeyCode::End, _) => st.cursor = st.sel.len(),
                    (KeyCode::Left, _) => st.cursor = st.cursor.saturating_sub(1),
                    (KeyCode::Right, _) => st.cursor = (st.cursor + 1).min(st.sel.len()),
                    (KeyCode::Backspace, _) => {
                        if st.cursor > 0 {
                            st.sel.remove(st.cursor - 1);
                            st.cursor -= 1;
                            recompute(st);
                        }
                    }
                    (KeyCode::Delete, _) => {
                        if st.cursor < st.sel.len() {
                            st.sel.remove(st.cursor);
                            recompute(st);
                        }
                    }
                    (KeyCode::Up, _) => st.scroll = st.scroll.saturating_sub(1),
                    (KeyCode::Down, _) => st.scroll += 1,
                    (KeyCode::PageUp, _) => {
                        let h = view_height();
                        st.scroll = st.scroll.saturating_sub(h);
                    }
                    (KeyCode::PageDown, _) => st.scroll += view_height(),
                    (KeyCode::Char(c), false) => {
                        st.sel.insert(st.cursor, c);
                        st.cursor += 1;
                        recompute(st);
                    }
                    _ => {}
                }
            }
            Event::Resize(..) => {} // next draw picks it up
            _ => {}
        }
        // Clamp scroll to content.
        let h = view_height();
        let max_scroll = st.preview.len().saturating_sub(h);
        st.scroll = st.scroll.min(max_scroll);
    }
}

fn term_size() -> (usize, usize) {
    terminal::size()
        .map(|(w, h)| (w as usize, h as usize))
        .unwrap_or((80, 24))
}

fn view_height() -> usize {
    term_size().1.saturating_sub(2).max(1)
}

fn draw(
    tty: &mut std::io::Stderr,
    st: &State,
    args: &Args,
    modes: &[Mode],
    input_name: &str,
) -> std::io::Result<()> {
    let (w, h) = term_size();
    let view_h = h.saturating_sub(2).max(1);

    queue!(
        tty,
        cursor::Hide,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    // Input line, horizontally scrolled so the cursor is always visible.
    let avail = w.saturating_sub(PROMPT.len() + 1).max(1);
    let start = st.cursor.saturating_sub(avail.saturating_sub(1));
    let visible: String = st.sel.iter().skip(start).take(avail).collect();
    queue!(
        tty,
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print(PROMPT),
        ResetColor,
        SetAttribute(Attribute::Reset),
        Print(&visible)
    )?;

    // Status line.
    queue!(tty, cursor::MoveTo(0, 1))?;
    let mode = modes[st.mode_ix].label(args);
    let hint = "Tab mode · ↑↓ scroll · Enter print · Esc quit";
    let status = match &st.error {
        Some(e) => format!(" ✗ {e}"),
        None => {
            let total = st.preview.len();
            let top = if total == 0 { 0 } else { st.scroll + 1 };
            let bottom = (st.scroll + view_h).min(total);
            format!(
                " {} match{} · {mode} · {input_name} · lines {top}-{bottom}/{total}",
                st.count,
                if st.count == 1 { "" } else { "es" },
            )
        }
    };
    let pad = w.saturating_sub(status.chars().count() + hint.chars().count() + 1);
    let line = if pad > 0 {
        format!("{status}{}{hint} ", " ".repeat(pad))
    } else {
        status.chars().take(w).collect()
    };
    if st.error.is_some() {
        queue!(
            tty,
            SetForegroundColor(Color::Red),
            SetAttribute(Attribute::Reverse),
            Print(line),
            SetAttribute(Attribute::Reset),
            ResetColor
        )?;
    } else {
        queue!(
            tty,
            SetAttribute(Attribute::Reverse),
            Print(line),
            SetAttribute(Attribute::Reset)
        )?;
    }

    // Preview pane.
    for (row, line) in st.preview.iter().skip(st.scroll).take(view_h).enumerate() {
        queue!(
            tty,
            cursor::MoveTo(0, (row + 2) as u16),
            Print(truncate_ansi(line, w))
        )?;
    }
    if st.preview.is_empty() && st.error.is_none() {
        queue!(
            tty,
            cursor::MoveTo(0, 2),
            SetForegroundColor(Color::DarkGrey),
            Print("(no output — refine the selector)"),
            ResetColor
        )?;
    }

    // Park the cursor in the input line.
    let cur_col = (PROMPT.len() + st.cursor.saturating_sub(start)).min(w.saturating_sub(1)) as u16;
    queue!(tty, cursor::MoveTo(cur_col, 0), cursor::Show)?;
    tty.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_args() -> Args {
        use clap::Parser;
        Args::parse_from(["cull", "div"])
    }

    #[test]
    fn truncate_plain() {
        assert_eq!(truncate_ansi("hello world", 5), "hello");
        assert_eq!(truncate_ansi("hi", 5), "hi");
    }

    #[test]
    fn truncate_keeps_ansi_and_resets() {
        let s = "\x1b[36m<div\x1b[0m class";
        let t = truncate_ansi(s, 4);
        assert_eq!(t, "\x1b[36m<div\x1b[0m\x1b[0m");
    }

    #[test]
    fn truncate_tabs_become_spaces() {
        assert_eq!(truncate_ansi("a\tb", 3), "a b");
    }

    #[test]
    fn cycle_without_flags_has_no_duplicate_html() {
        let args = plain_args();
        let modes = mode_cycle(&args);
        assert_eq!(modes.len(), 4);
        assert_eq!(modes[0].label(&args), "html");
    }

    #[test]
    fn cycle_with_flags_keeps_html_reachable() {
        use clap::Parser;
        let args = Args::parse_from(["cull", "div", "-t"]);
        let modes = mode_cycle(&args);
        assert_eq!(modes.len(), 5);
        assert_eq!(modes[0].label(&args), "as given");
        assert!(modes.iter().any(|m| m.label(&args) == "html"));
    }

    #[test]
    fn mode_args_overrides_shape_flags() {
        use clap::Parser;
        let args = Args::parse_from(["cull", "div", "-t", "--json-rows", "--table"]);
        let a = mode_args(&args, Mode::Markdown);
        assert!(a.md && !a.text && !a.table && !a.json_rows);
        let a = mode_args(&args, Mode::AsGiven);
        assert!(a.text && a.table);
    }

    #[test]
    fn empty_selector_matches_root() {
        let doc = Html::parse_document("<html><body><p>hi</p></body></html>");
        let args = plain_args();
        let m = compute_matches(&doc, "", &args).unwrap();
        assert_eq!(m.len(), 1);
        let m = compute_matches(&doc, "p", &args).unwrap();
        assert_eq!(m.len(), 1);
        assert!(compute_matches(&doc, "p[", &args).is_err());
    }

    #[test]
    fn render_respects_mode() {
        let doc = Html::parse_document("<html><body><p>hi <b>there</b></p></body></html>");
        let args = plain_args();
        let m = compute_matches(&doc, "p", &args).unwrap();
        let (text, found) =
            render(&mode_args(&args, Mode::Text), &m, true, None, None, false).unwrap();
        assert!(found);
        assert_eq!(text.trim(), "hi there");
        let (html, _) = render(
            &mode_args(&args, Mode::AsGiven),
            &m,
            true,
            None,
            None,
            false,
        )
        .unwrap();
        assert!(html.contains("<b>there</b>"));
    }
}
