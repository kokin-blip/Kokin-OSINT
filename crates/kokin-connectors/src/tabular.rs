//! Bounded, local-only bulk preview. No identifier is sent by parsing a file.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
const MAX_ROWS: usize = 20_000;
const MAX_COLUMNS: usize = 64;
#[derive(Debug, Serialize, Deserialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub warnings: Vec<String>,
}
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
pub fn parse(text: &str, format: &str, header: bool) -> Result<Table> {
    if text.len() > 2_000_000 {
        return Err(invalid("bulk preview exceeds 2 MB"));
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut table = match format {
        "json" => json(text)?,
        "csv" | "tsv" => {
            let mut rows = delimited(text, if format == "csv" { ',' } else { '\t' })?;
            let width = rows.iter().map(Vec::len).max().unwrap_or(1);
            let mut columns = if header && !rows.is_empty() {
                rows.remove(0)
            } else {
                (1..=width).map(|i| format!("Column {i}")).collect()
            };
            while columns.len() < width {
                columns.push(format!("Column {}", columns.len() + 1));
            }
            Table {
                columns,
                rows,
                warnings: vec![],
            }
        }
        "text" | "md" | "log" => Table {
            columns: vec!["Text".into()],
            rows: text
                .lines()
                .filter(|v| !v.trim().is_empty())
                .map(|s| vec![s.to_owned()])
                .collect(),
            warnings: vec![
                "Text lines are preserved. Map or edit selectors before any external research."
                    .into(),
            ],
        },
        _ => return Err(invalid(
            "choose CSV, TSV, JSON, text, Markdown or logs; process PDF/OCR evidence locally first",
        )),
    };
    if table.rows.len() > MAX_ROWS
        || table.columns.len() > MAX_COLUMNS
        || table
            .rows
            .iter()
            .any(|r| r.len() > MAX_COLUMNS || r.iter().any(|v| v.len() > 8192))
    {
        return Err(invalid(
            "preview exceeds 20,000 rows, 64 columns or 8 KiB per cell",
        ));
    }
    let width = table.columns.len();
    let uneven = table.rows.iter().any(|r| r.len() != width);
    if uneven {
        table.warnings.push(
            "Rows have different column counts; inspect empty/missing cells before selecting."
                .into(),
        );
    }
    for row in &mut table.rows {
        row.resize(width.max(row.len()), String::new());
    }
    Ok(table)
}
/// Revalidate structured IPC inputs as well as file-parser output.
pub fn validate(table: &Table) -> Result<()> {
    let size = table.warnings.iter().map(String::len).sum::<usize>()
        + table.columns.iter().map(String::len).sum::<usize>()
        + table.rows.iter().flatten().map(String::len).sum::<usize>();
    if table.rows.len() > MAX_ROWS
        || table.warnings.len() > 100
        || table.warnings.iter().any(|s| s.len() > 8192)
        || table.columns.is_empty()
        || table.columns.len() > MAX_COLUMNS
        || size > 2_000_000
        || table.columns.iter().any(|s| s.len() > 8192)
        || table
            .rows
            .iter()
            .any(|r| r.len() > MAX_COLUMNS || r.iter().any(|v| v.len() > 8192))
    {
        return Err(invalid(
            "preview exceeds 2 MB, 20,000 rows, 64 columns or 8 KiB per cell",
        ));
    }
    Ok(())
}
#[derive(Debug, Serialize)]
pub struct ColumnSuggestion {
    pub column: usize,
    pub kind: crate::research::SelectorKind,
    pub valid: usize,
    pub sampled: usize,
    pub basis: String,
}
pub fn suggestions(table: &Table) -> Result<Vec<ColumnSuggestion>> {
    use crate::research::{Selector, SelectorKind::*};
    validate(table)?;
    let mut suggestions = vec![];
    for (column, header) in table.columns.iter().enumerate() {
        let header = header.to_ascii_lowercase();
        for kind in [Email, Phone, Domain, Ip, Wallet, Username, Name] {
            let named = match kind {
                Email => header.contains("email"),
                Phone => header.contains("phone") || header.contains("mobile"),
                Domain => header.contains("domain") || header.contains("website"),
                Ip => header == "ip" || header.contains("ip_address"),
                Wallet => header.contains("wallet"),
                Username => header.contains("username") || header.contains("handle"),
                Name => header.contains("name") && !header.contains("username"),
                _ => false,
            };
            if matches!(kind, Name | Username) && !named {
                continue;
            }
            let values = table
                .rows
                .iter()
                .take(100)
                .filter_map(|r| r.get(column))
                .filter(|v| !v.trim().is_empty())
                .collect::<Vec<_>>();
            let valid = values
                .iter()
                .filter(|v| Selector::parse(kind, v).is_ok())
                .count();
            if named || (values.len() >= 2 && valid * 10 >= values.len() * 9) {
                suggestions.push(ColumnSuggestion {
                    column,
                    kind,
                    valid,
                    sampled: values.len(),
                    basis: if named {
                        "column header and local syntax"
                    } else {
                        "local syntax sample; review ambiguous matches"
                    }
                    .into(),
                });
            }
        }
    }
    suggestions.sort_by_key(|s| (s.column, std::cmp::Reverse(s.valid)));
    Ok(suggestions)
}
pub fn candidates(table: &Table, column: usize) -> Result<crate::candidates::CandidatePreview> {
    validate(table)?;
    if column >= table.columns.len() {
        return Err(invalid("choose a valid text column"));
    }
    let segments = table
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row.get(column).map(|text| crate::candidates::Segment {
                origin_id: format!("local-file-row-{}", index + 1),
                text: text.clone(),
            })
        })
        .collect::<Vec<_>>();
    crate::candidates::extract(&segments)
}
pub fn mapped_columns(
    table: &Table,
    columns: &[usize],
    kind: crate::research::SelectorKind,
) -> Result<Vec<crate::research::BulkRow>> {
    validate(table)?;
    if columns.is_empty()
        || columns.len() > 4
        || columns.iter().any(|i| *i >= table.columns.len())
        || columns
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != columns.len()
    {
        return Err(invalid("choose 1–4 distinct valid columns"));
    }
    if columns.len() > 1 && kind != crate::research::SelectorKind::Name {
        return Err(invalid(
            "multiple columns may only be joined for an explicit name selector",
        ));
    }
    let joined = Table {
        columns: vec!["Mapped selector".into()],
        rows: table
            .rows
            .iter()
            .map(|r| {
                vec![columns
                    .iter()
                    .filter_map(|i| r.get(*i))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")]
            })
            .collect(),
        warnings: vec![],
    };
    mapped(&joined, 0, kind)
}
fn delimited(text: &str, separator: char) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut iter = text.chars().peekable();
    while let Some(c) = iter.next() {
        if quoted {
            if c == '"' {
                if iter.peek() == Some(&'"') {
                    iter.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(c);
            }
        } else {
            match c {
                '"' if cell.is_empty() && !closed => quoted = true,
                c if c == separator => {
                    row.push(std::mem::take(&mut cell));
                    closed = false;
                }
                '\r' | '\n' => {
                    if c == '\r' && iter.peek() == Some(&'\n') {
                        iter.next();
                    }
                    row.push(std::mem::take(&mut cell));
                    if row.iter().any(|s| !s.is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                    closed = false;
                }
                _ if closed => return Err(invalid("unexpected text after a closing quote")),
                '"' => return Err(invalid("quote inside an unquoted cell")),
                _ => cell.push(c),
            }
        }
        if rows.len() > MAX_ROWS + 1 || row.len() > MAX_COLUMNS || cell.len() > 8192 {
            return Err(invalid("delimited import exceeds preview bounds"));
        }
    }
    if quoted {
        return Err(invalid("unterminated quoted cell"));
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}
fn json(text: &str) -> Result<Table> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| invalid("invalid JSON import"))?;
    let rows = value.as_array().ok_or_else(|| {
        invalid("JSON preview needs an array of scalar values, arrays or objects")
    })?;
    if rows.len() > MAX_ROWS {
        return Err(invalid("too many JSON rows"));
    }
    let scalar = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        _ => v.to_string(),
    };
    if rows.iter().all(|v| v.is_object()) {
        let mut columns = Vec::<String>::new();
        for row in rows {
            for key in row
                .as_object()
                .ok_or_else(|| invalid("expected object"))?
                .keys()
            {
                if !columns.contains(key) {
                    columns.push(key.clone());
                    if columns.len() > MAX_COLUMNS {
                        return Err(invalid("too many JSON columns"));
                    }
                }
            }
        }
        Ok(Table {
            rows: rows
                .iter()
                .map(|r| columns.iter().map(|k| scalar(&r[k])).collect())
                .collect(),
            columns,
            warnings: vec![
                "Nested JSON values remain JSON text; select a scalar identifier column.".into(),
            ],
        })
    } else if rows.iter().all(|v| v.is_array()) {
        let width = rows
            .iter()
            .filter_map(|v| v.as_array().map(Vec::len))
            .max()
            .unwrap_or(1);
        Ok(Table {
            columns: (1..=width).map(|i| format!("Column {i}")).collect(),
            rows: rows
                .iter()
                .map(|r| {
                    r.as_array()
                        .map(|a| a.iter().map(scalar).collect())
                        .unwrap_or_default()
                })
                .collect(),
            warnings: vec![],
        })
    } else if rows.iter().all(|v| !v.is_array() && !v.is_object()) {
        Ok(Table {
            columns: vec!["Value".into()],
            rows: rows.iter().map(|v| vec![scalar(v)]).collect(),
            warnings: vec![],
        })
    } else {
        Err(invalid(
            "mixed JSON row structures need a consistent column mapping",
        ))
    }
}
pub fn mapped(
    table: &Table,
    column: usize,
    kind: crate::research::SelectorKind,
) -> Result<Vec<crate::research::BulkRow>> {
    validate(table)?;
    if column >= table.columns.len() {
        return Err(invalid("invalid bulk column"));
    }
    // Newlines inside a name are not separate authorizations. Validate each mapped cell intact.
    let mut result: Vec<crate::research::BulkRow> = Vec::new();
    let mut positions = std::collections::HashMap::<String, usize>::new();
    for row in &table.rows {
        let value = row.get(column).map_or("", String::as_str).trim();
        if value.is_empty() {
            continue;
        }
        let parsed = crate::research::Selector::parse(kind, value);
        let identity = parsed.as_ref().map_or(value, |s| s.normalized.as_str());
        if let Some(index) = positions.get(identity) {
            result[*index].repeats += 1;
            continue;
        }
        positions.insert(identity.into(), result.len());
        let (selector, error) = match parsed {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        result.push(crate::research::BulkRow {
            value: value.into(),
            kind,
            selector,
            error,
            repeats: 1,
            origin_ids: vec![],
        });
    }
    Ok(result)
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::research::SelectorKind;
    #[test]
    fn mapping_suggests_syntax_without_sending_and_joins_only_explicit_name_columns() {
        use crate::research::SelectorKind::*;
        let table = parse(
            "first,last,email\nJane,Doe,Case@example.org\nJohn,Roe,other@example.org",
            "csv",
            true,
        )
        .unwrap();
        assert!(suggestions(&table)
            .unwrap()
            .iter()
            .any(|s| s.column == 2 && s.kind == Email && s.valid == 2));
        let names = mapped_columns(&table, &[0, 1], Name).unwrap();
        assert_eq!(names[0].selector.as_ref().unwrap().normalized, "Jane Doe");
        assert!(mapped_columns(&table, &[0, 1], Email).is_err());
        assert!(mapped_columns(&table, &[0, 0], Name).is_err());
        let malicious = Table {
            columns: vec!["email".into()],
            rows: vec![vec!["x".repeat(8193)]],
            warnings: vec![],
        };
        assert!(mapped(&malicious, 0, Email).is_err());
    }
    #[test]
    fn quoted_cells_do_not_split_identifiers_or_drop_unselected_rows() {
        let table = parse(
            "name,email\r\n\"Doe, Jane\",Jane@EXAMPLE.org\r\n\"A\"\"B\",Jane@example.org\r\n",
            "csv",
            true,
        )
        .unwrap();
        assert_eq!(table.rows[0][0], "Doe, Jane");
        assert_eq!(table.rows[1][0], "A\"B");
        let rows = mapped(&table, 1, SelectorKind::Email).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].repeats, 2);
        assert_eq!(
            rows[0].selector.as_ref().unwrap().normalized,
            "Jane@example.org"
        );
        assert!(parse("\"unfinished", "csv", false).is_err());
    }
    #[test]
    fn json_columns_and_import_bounds_are_explicit() {
        let t = parse(r#"[{"email":"a@example.org"},{"name":"A"}]"#, "json", false).unwrap();
        assert_eq!(t.columns, vec!["email", "name"]);
        assert_eq!(t.rows[1][0], "");
        assert!(parse(&"a\n".repeat(20_001), "text", false).is_err());
        let t = parse("one\none\ntwo", "text", false).unwrap();
        assert_eq!(mapped(&t, 0, SelectorKind::Username).unwrap().len(), 2);
    }
}
