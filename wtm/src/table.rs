//! Plain text tables.

use std::fmt::Write;

/// Format rows as left-aligned columns separated by two spaces. The last
/// column is not padded.
pub fn format(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|i| {
            rows.iter()
                .filter_map(|r| r.get(i))
                .map(|c| c.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if i + 1 == row.len() {
                line.push_str(cell);
            } else {
                let _ = write!(line, "{cell:<width$}  ", width = widths[i]);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
