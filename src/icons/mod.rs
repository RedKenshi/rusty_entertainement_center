use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct IconData {
    pub primary_path: String,
    pub secondary_path: String,
    pub viewbox_width: f32,
    pub viewbox_height: f32,
}

const ICONS_ROOT: &str = "assets/icons/svgs";

pub fn load_icon(name: &str, style: &str) -> IconData {
    let style_dir = style_to_dir(style);
    let svg_path = Path::new(ICONS_ROOT).join(style_dir).join(format!("{name}.svg"));

    match fs::read_to_string(&svg_path) {
        Ok(content) => parse_svg(&content),
        Err(err) => {
            eprintln!("icon not found ({svg_path:?}): {err}");
            IconData::default()
        }
    }
}

fn style_to_dir(style: &str) -> &'static str {
    match style {
        "solid" => "solid",
        "regular" => "regular",
        "light" => "light",
        "thin" => "thin",
        "duotone" => "duotone",
        "brands" => "brands",
        _ => "solid",
    }
}

fn parse_svg(content: &str) -> IconData {
    let (viewbox_width, viewbox_height) = parse_viewbox(content).unwrap_or((512.0, 512.0));

    let mut primary_path = String::new();
    let mut secondary_path = String::new();
    let mut unclassified_paths = Vec::new();

    for tag in extract_path_tags(content) {
        let Some(d) = extract_attr(&tag, "d") else {
            continue;
        };

        let class = extract_attr(&tag, "class").unwrap_or_default();
        if class.contains("fa-primary") {
            primary_path = d;
        } else if class.contains("fa-secondary") {
            secondary_path = d;
        } else {
            unclassified_paths.push(d);
        }
    }

    if primary_path.is_empty() {
        if unclassified_paths.len() == 1 {
            primary_path = unclassified_paths.remove(0);
        } else if !unclassified_paths.is_empty() {
            primary_path = unclassified_paths.join(" ");
        }
    }

    IconData {
        primary_path,
        secondary_path,
        viewbox_width,
        viewbox_height,
    }
}

fn parse_viewbox(content: &str) -> Option<(f32, f32)> {
    let viewbox = extract_attr(content, "viewBox")?;
    let mut parts = viewbox.split_whitespace();
    parts.next()?;
    parts.next()?;
    let width: f32 = parts.next()?.parse().ok()?;
    let height: f32 = parts.next()?.parse().ok()?;
    Some((width, height))
}

fn extract_path_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut search_from = 0;

    while let Some(rel_start) = content[search_from..].find("<path") {
        let start = search_from + rel_start;
        let Some(rel_end) = content[start..].find('>') else {
            break;
        };
        let end = start + rel_end + 1;
        tags.push(content[start..end].to_string());
        search_from = end;
    }

    tags
}

fn extract_attr(source: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = source.find(&needle)? + needle.len();
    let rest = &source[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_solid_hard_drive() {
        let icon = load_icon("hard-drive", "solid");
        assert!(!icon.primary_path.is_empty());
        assert!(icon.secondary_path.is_empty());
        assert_eq!(icon.viewbox_width, 512.0);
    }

    #[test]
    fn loads_duotone_hard_drive_with_two_paths() {
        let icon = load_icon("hard-drive", "duotone");
        assert!(!icon.primary_path.is_empty());
        assert!(!icon.secondary_path.is_empty());
    }

    #[test]
    fn unknown_icon_returns_empty_paths() {
        let icon = load_icon("this-icon-does-not-exist", "solid");
        assert!(icon.primary_path.is_empty());
    }
}
