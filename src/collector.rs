// collector.rs — Section-delimited report ingestion
//
// Session reports produced by this framework (and by the systems it was
// extracted from) are NOT pure CSV: a single file carries multiple named
// sections, each delimited by a "=== SECTION NAME ===" marker line, each
// with its own CSV header and rows. This module extracts one section by
// marker and parses its rows.

use std::path::Path;

/// One OHLCV bar. The framework's unit of market data.
#[derive(Debug, Clone)]
pub struct Bar {
    pub index:     usize,
    pub timestamp: String,
    pub open:      f64,
    pub high:      f64,
    pub low:       f64,
    pub close:     f64,
    pub volume:    f64,
}

/// Extract the CSV lines of one section from a section-delimited report.
/// Returns (header_line, data_lines). The section starts at the line
/// containing `marker`, its header is the next non-empty line, and it ends
/// at the next "===" line or EOF.
pub fn extract_section<'a>(
    lines: &[&'a str],
    marker: &str,
) -> Result<(&'a str, Vec<&'a str>), String> {
    let start = lines
        .iter()
        .position(|l| l.contains(marker))
        .ok_or_else(|| format!("No '{}' section found", marker))?;

    let header_idx = start + 1;
    if header_idx >= lines.len() {
        return Err(format!("No header after '{}' marker", marker));
    }
    let header = lines[header_idx];

    let mut rows = Vec::new();
    for line in &lines[header_idx + 1..] {
        let line = line.trim();
        if line.starts_with("===") {
            break;
        }
        if line.is_empty() {
            continue;
        }
        rows.push(line);
    }
    Ok((header, rows))
}

/// Load market data bars from the "MARKET DATA" section of a report file.
/// Expected header: index,timestamp,open,high,low,close,volume
pub fn load_bars(path: &Path) -> Result<Vec<Bar>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();

    let (header, raw_rows) = extract_section(&lines, "MARKET DATA")?;
    if !header.starts_with("index,timestamp,open,high,low,close,volume") {
        return Err(format!("Unexpected MARKET DATA header: {}", header));
    }

    let mut bars = Vec::with_capacity(raw_rows.len());
    for line in raw_rows {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 7 {
            continue;
        }
        bars.push(Bar {
            index:     fields[0].parse().unwrap_or(0),
            timestamp: fields[1].to_string(),
            open:      fields[2].parse().unwrap_or(0.0),
            high:      fields[3].parse().unwrap_or(0.0),
            low:       fields[4].parse().unwrap_or(0.0),
            close:     fields[5].parse().unwrap_or(0.0),
            volume:    fields[6].parse().unwrap_or(0.0),
        });
    }

    if bars.is_empty() {
        Err("No data rows found in MARKET DATA section".into())
    } else {
        Ok(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_section() {
        let content = "=== HEADER ===\na,b\n1,2\n\n=== MARKET DATA ===\nindex,timestamp,open,high,low,close,volume\n0,t,1,2,0.5,1.5,100\n=== FOOTER ===\nx\n";
        let lines: Vec<&str> = content.lines().collect();
        let (header, rows) = extract_section(&lines, "MARKET DATA").unwrap();
        assert!(header.starts_with("index,"));
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn test_missing_section() {
        let lines = vec!["a,b", "1,2"];
        assert!(extract_section(&lines, "MARKET DATA").is_err());
    }
}
