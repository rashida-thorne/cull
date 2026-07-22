//! `--table`: HTML tables → CSV or NDJSON.

use crate::text;
use scraper::{ElementRef, Selector};
use serde_json::{Map, Value};
use std::io::Write;

/// Extract tables from the matched elements.
///
/// If the user gave a selector, each match is either a table itself or a
/// container to search within; with no selector we search the whole document.
pub fn run(
    matches: &[ElementRef],
    _had_selector: bool,
    json_rows: bool,
    pretty: bool,
    out: &mut impl Write,
) -> Result<bool, String> {
    let table_sel = Selector::parse("table").unwrap();
    let mut tables: Vec<ElementRef> = Vec::new();
    for m in matches {
        if m.value().name() == "table" {
            tables.push(*m);
        } else {
            tables.extend(m.select(&table_sel));
        }
    }
    // Drop nested tables already covered by an ancestor in the list.
    let mut seen: Vec<ElementRef> = Vec::new();
    for t in tables {
        let nested = seen.iter().any(|s| t.ancestors().any(|a| a.id() == s.id()));
        if !nested {
            seen.push(t);
        }
    }

    let mut any = false;
    let mut first = true;
    for t in &seen {
        let grid = to_grid(*t);
        if grid.is_empty() {
            continue;
        }
        any = true;
        if json_rows {
            write_json(&grid, pretty, out)?;
        } else {
            if !first {
                writeln!(out).map_err(|e| e.to_string())?; // blank line between tables
            }
            write_csv(&grid, out)?;
        }
        first = false;
    }
    Ok(any)
}

/// Expand an HTML table to a rectangular grid, honouring colspan/rowspan.
fn to_grid(table: ElementRef) -> Vec<Vec<String>> {
    let row_sel = Selector::parse("tr").unwrap();
    let cell_sel = Selector::parse("th, td").unwrap();

    // Skip rows of nested tables.
    let rows: Vec<ElementRef> = table
        .select(&row_sel)
        .filter(|tr| {
            nearest_table(*tr)
                .map(|t| t.id() == table.id())
                .unwrap_or(false)
        })
        .collect();

    let mut grid: Vec<Vec<Option<String>>> = Vec::new();
    for (r, tr) in rows.iter().enumerate() {
        if grid.len() <= r {
            grid.push(Vec::new());
        }
        let mut c = 0usize;
        for cell in tr.select(&cell_sel) {
            // Skip cells that belong to a nested table.
            if nearest_table(cell).map(|o| o.id()) != Some(table.id()) {
                continue;
            }
            // Find next free column in this row.
            while grid[r].len() > c && grid[r][c].is_some() {
                c += 1;
            }
            let colspan = attr_num(cell, "colspan");
            let rowspan = attr_num(cell, "rowspan");
            let val = text::collapsed_text(cell);
            for dr in 0..rowspan {
                let rr = r + dr;
                while grid.len() <= rr {
                    grid.push(Vec::new());
                }
                for dc in 0..colspan {
                    let cc = c + dc;
                    while grid[rr].len() <= cc {
                        grid[rr].push(None);
                    }
                    // Only the top-left of a span carries the value; the rest
                    // are filled with copies so rows stay aligned.
                    grid[rr][cc] = Some(val.clone());
                }
            }
            c += colspan;
        }
    }

    grid.into_iter()
        .map(|row| row.into_iter().map(|c| c.unwrap_or_default()).collect())
        .filter(|row: &Vec<String>| !row.is_empty())
        .collect()
}

fn nearest_table(el: ElementRef) -> Option<ElementRef> {
    el.ancestors()
        .filter_map(ElementRef::wrap)
        .find(|a| a.value().name() == "table")
}

fn attr_num(el: ElementRef, name: &str) -> usize {
    el.value()
        .attr(name)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| (1..=1000).contains(&n))
        .unwrap_or(1)
}

fn write_csv(grid: &[Vec<String>], out: &mut impl Write) -> Result<(), String> {
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    for row in grid {
        let line = (0..width)
            .map(|i| csv_field(row.get(i).map(String::as_str).unwrap_or("")))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_json(grid: &[Vec<String>], pretty: bool, out: &mut impl Write) -> Result<(), String> {
    if grid.len() < 2 {
        return Ok(());
    }
    let headers = &grid[0];
    for row in &grid[1..] {
        let mut obj = Map::new();
        for (i, h) in headers.iter().enumerate() {
            let key = if h.is_empty() {
                format!("col{}", i + 1)
            } else {
                h.clone()
            };
            obj.insert(key, Value::String(row.get(i).cloned().unwrap_or_default()));
        }
        let v = Value::Object(obj);
        let s = if pretty {
            serde_json::to_string_pretty(&v).unwrap()
        } else {
            serde_json::to_string(&v).unwrap()
        };
        writeln!(out, "{s}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    #[test]
    fn spans() {
        let html = Html::parse_document(
            r#"<table>
                <tr><th>A</th><th colspan="2">B</th></tr>
                <tr><td rowspan="2">1</td><td>2</td><td>3</td></tr>
                <tr><td>4</td><td>5</td></tr>
               </table>"#,
        );
        let sel = Selector::parse("table").unwrap();
        let t = html.select(&sel).next().unwrap();
        let g = to_grid(t);
        assert_eq!(g[0], vec!["A", "B", "B"]);
        assert_eq!(g[1], vec!["1", "2", "3"]);
        assert_eq!(g[2], vec!["1", "4", "5"]);
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("plain"), "plain");
    }
}
