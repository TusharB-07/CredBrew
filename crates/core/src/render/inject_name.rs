use quick_xml::events::{BytesText, Event};
use quick_xml::{Reader, Writer};
use crate::error::CoreError;
use crate::render::{shrink_to_fit, NameZone};
use resvg::usvg::TreeParsing;

pub fn inject_name(svg: &str, name: &str, zone: &NameZone) -> Result<String, CoreError> {
    let escaped_name = escape_xml(name);

    let mut reader = Reader::from_str(svg);
    reader.trim_text(true);

    let mut events: Vec<Event> = Vec::new();
    let mut text_ranges: Vec<(usize, usize)> = Vec::new();
    let mut text_event_indexes: Vec<usize> = Vec::new();
    let mut concat = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Text(t)) => {
                let content = t.unescape().map_err(|e| CoreError::Render(e.to_string()))?;
                let start = concat.len();
                let end = start + content.len();
                concat.push_str(&content);
                text_event_indexes.push(events.len());
                text_ranges.push((start, end));
                events.push(Event::Text(t));
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::Render(e.to_string())),
            Ok(ev) => {
                events.push(ev);
            }
        }
    }

    let marker = "{{NAME}}";
    let marker_start = concat.find(marker).ok_or_else(|| CoreError::Render("{{NAME}} placeholder not found".into()))?;
    let marker_end = marker_start + marker.len();

    let replaced = text_event_indexes
        .iter()
        .zip(text_ranges.iter())
        .enumerate()
        .filter(|(_, (_, (start, end)))| *end > marker_start && *start < marker_end)
        .map(|(i, (idx, _))| (*idx, i))
        .collect::<Vec<_>>();

    if replaced.is_empty() {
        return Err(CoreError::Render("{{NAME}} placeholder not found".into()));
    }

    let mut writer = Writer::new(Vec::with_capacity(svg.len() + 256));
    let mut wrote_name = false;
    for (idx, ev) in events.into_iter().enumerate() {
        if replaced.iter().any(|(event_idx, _)| *event_idx == idx) {
            if !wrote_name {
                wrote_name = true;
                writer.write_event(Event::Text(BytesText::new(&escaped_name))).map_err(|e| CoreError::Render(e.to_string()))?;
            }
            continue;
        }
        writer.write_event(ev).map_err(|e| CoreError::Render(e.to_string()))?;
    }

    let result = writer.into_inner();
    let result_str = String::from_utf8(result).map_err(|e| CoreError::Render(e.to_string()))?;

    let tree = resvg::usvg::Tree::from_str(&result_str, &resvg::usvg::Options::default())
        .map_err(|e| CoreError::Render(format!("usvg parse after inject: {}", e)))?;

    let final_svg = shrink_to_fit(&result_str, name, zone, tree.size.width() as f32, tree.size.height() as f32)?;

    Ok(final_svg)
}

fn escape_xml(s: &str) -> String {
    let amp = "&".to_string();
    let lt = "<".to_string();
    let gt = ">".to_string();
    s.replace('&', &amp).replace('<', &lt).replace('>', &gt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_name_basic() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="800"><text x="50%" y="50%">{{NAME}}</text></svg>"#;
        let zone = NameZone {
            x_pct: 10.0,
            y_pct: 70.0,
            w_pct: 80.0,
            font_size_pct: 6.0,
            color: "#111111".into(),
            align: "center".into(),
        };
        let result = inject_name(svg, "Jane", &zone).unwrap();
        assert!(result.contains("Jane"));
        assert!(!result.contains("{{NAME}}"));
    }

    #[test]
    fn test_inject_name_tspan_split() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="800"><text><tspan>{{</tspan><tspan>NAME</tspan><tspan>}}</tspan></text></svg>"#;
        let zone = NameZone {
            x_pct: 10.0,
            y_pct: 70.0,
            w_pct: 80.0,
            font_size_pct: 6.0,
            color: "#111111".into(),
            align: "center".into(),
        };
        let result = inject_name(svg, "Test", &zone).unwrap();
        assert!(result.contains("Test"));
    }
}