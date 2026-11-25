//! Shared calendar math, time formatting, and hierarchical tree-row helpers for picker & tree widgets.

use std::collections::HashSet;

/// Parse an ISO `"YYYY-MM-DD"` string, falling back to `(2026, 1, 15)` on invalid input.
pub fn parse_iso_date(s: &str) -> (i32, u32, u32) {
    let mut parts = s.trim().split('-');
    if let (Some(ys), Some(ms), Some(ds), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
        && let (Ok(y), Ok(m), Ok(d)) = (ys.parse::<i32>(), ms.parse::<u32>(), ds.parse::<u32>())
        && (1900..=2200).contains(&y)
        && (1..=12).contains(&m)
    {
        let max_d = days_in_month(y, m);
        if (1..=max_d).contains(&d) {
            return (y, m, d);
        }
    }
    (2026, 1, 15)
}

/// Format `(year, month, day)` as `"YYYY-MM-DD"`.
pub fn format_iso_date(year: i32, month: u32, day: u32) -> String {
    let m = month.clamp(1, 12);
    let d = day.clamp(1, days_in_month(year, m));
    format!("{year:04}-{m:02}-{d:02}")
}

/// Returns whether `year` is a Gregorian leap year.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Returns the number of days in `(year, month)` for `month` in `1..=12`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Returns weekday index (`0 = Sunday .. 6 = Saturday`) using Tomohiko Sakamoto's algorithm.
pub fn weekday_sunday0(year: i32, month: u32, day: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let m = month.clamp(1, 12) as usize;
    let y = if m < 3 { year - 1 } else { year };
    let w = (y + y / 4 - y / 100 + y / 400 + T[m - 1] + day as i32).rem_euclid(7);
    w as u32
}

/// Human-readable English month name for `month` in `1..=12`.
pub fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Month",
    }
}

/// Shift `(year, month)` by `delta_months` (e.g. `-1` or `+1`), keeping `month` in `1..=12`.
pub fn shift_month(year: i32, month: u32, delta_months: i32) -> (i32, u32) {
    let total = year * 12 + (month.clamp(1, 12) as i32 - 1) + delta_months;
    let ny = total.div_euclid(12);
    let nm = total.rem_euclid(12) as u32 + 1;
    (ny, nm)
}

/// Parse `"HH:MM"` (24-hour), falling back to `(9, 30)` on invalid input.
pub fn parse_time_hm(s: &str) -> (u32, u32) {
    let mut parts = s.trim().split(':');
    if let (Some(hs), Some(ms), None) = (parts.next(), parts.next(), parts.next())
        && let (Ok(h), Ok(m)) = (hs.parse::<u32>(), ms.parse::<u32>())
        && h < 24
        && m < 60
    {
        return (h, m);
    }
    (9, 30)
}

/// Format `(hour, minute)` as `"HH:MM"` (24-hour).
pub fn format_time_hm(hour: u32, minute: u32) -> String {
    format!("{:02}:{:02}", hour % 24, minute % 60)
}

/// Visible row entry derived from slash-delimited `TreeView` paths (for example `"src/main.rs"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeVisibleRow {
    pub raw_index: usize,
    pub path: String,
    pub label: String,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
}

/// Derive visible tree rows from slash-delimited item paths, respecting `collapsed` parent paths.
pub fn derive_visible_tree_rows(items: &[String], collapsed: &HashSet<String>) -> Vec<TreeVisibleRow> {
    let mut out = Vec::with_capacity(items.len());
    for (idx, raw) in items.iter().enumerate() {
        let trimmed = raw.trim_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        // Hide row if any proper ancestor path is collapsed.
        let mut hidden = false;
        let mut pos = 0;
        while let Some(slash) = trimmed[pos..].find('/') {
            let anc = &trimmed[..pos + slash];
            if collapsed.contains(anc) {
                hidden = true;
                break;
            }
            pos += slash + 1;
        }
        if hidden {
            continue;
        }

        let prefix = format!("{trimmed}/");
        let has_children = items
            .iter()
            .enumerate()
            .any(|(j, other)| j != idx && other.trim_matches('/').starts_with(&prefix));
        let depth = trimmed.matches('/').count();
        let label = trimmed
            .rsplit('/')
            .next()
            .unwrap_or(trimmed)
            .to_string();
        let expanded = has_children && !collapsed.contains(trimmed);

        out.push(TreeVisibleRow {
            raw_index: idx,
            path: trimmed.to_string(),
            label,
            depth,
            has_children,
            expanded,
        });
    }
    out
}

/// Parse `"MenuLabel:Item1,Item2"` specs into `(title, items)` pairs for [`WidgetKind::MenuBar`](crate::WidgetKind::MenuBar).
pub fn parse_menubar_specs(raw: &[String]) -> Vec<(String, Vec<String>)> {
    raw.iter()
        .filter_map(|entry| {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some((title, items_str)) = trimmed.split_once(':') {
                let items: Vec<String> = items_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Some((title.trim().to_string(), items))
            } else {
                Some((trimmed.to_string(), Vec::new()))
            }
        })
        .collect()
}

/// Compute top-level menu header hit rectangles for a `MenuBar` rect.
pub fn menubar_header_rects(bar_rect: craftui_core::Rect, menus: &[(String, Vec<String>)]) -> Vec<craftui_core::Rect> {
    let mut x = bar_rect.x + 8.0;
    let mut out = Vec::with_capacity(menus.len());
    for (title, _) in menus {
        let w = (title.chars().count() as f32 * 8.5 + 22.0).max(52.0);
        out.push(craftui_core::Rect::new(x, bar_rect.y + 3.0, w, (bar_rect.height - 6.0).max(24.0)));
        x += w + 4.0;
    }
    out
}

/// Compute the floating dropdown rectangle below a specific `MenuBar` header.
pub fn menubar_dropdown_rect(
    header_rect: craftui_core::Rect,
    item_count: usize,
) -> craftui_core::Rect {
    let w = 180.0f32.max(header_rect.width + 24.0);
    let h = (item_count.max(1) as f32 * 30.0 + 8.0).min(280.0);
    craftui_core::Rect::new(header_rect.x, header_rect.max_y() + 2.0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_math_and_leap_years() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(parse_iso_date("2026-11-25"), (2026, 11, 25));
        assert_eq!(format_iso_date(2026, 2, 31), "2026-02-28");
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(weekday_sunday0(2026, 11, 25), 3); // Wednesday
    }

    #[test]
    fn tree_rows_collapse_descendants() {
        let items = vec![
            "src".into(),
            "src/core".into(),
            "src/core/mod.rs".into(),
            "Cargo.toml".into(),
        ];
        let mut collapsed = HashSet::new();
        assert_eq!(derive_visible_tree_rows(&items, &collapsed).len(), 4);

        collapsed.insert("src/core".into());
        let rows = derive_visible_tree_rows(&items, &collapsed);
        assert_eq!(rows.len(), 3);
        assert!(rows[1].has_children);
        assert!(!rows[1].expanded);
        assert_eq!(rows[2].label, "Cargo.toml");
    }
}
