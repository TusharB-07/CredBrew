use ttf_parser::Face;
use crate::error::CoreError;
use crate::render::NameZone;

lazy_static::lazy_static! {
    static ref FONT_DB: fontdb::Database = {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../../assets/fonts/NotoSans-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/fonts/NotoSans-Bold.ttf").to_vec());
        db.set_sans_serif_family("Noto Sans");
        db
    };
    static ref NOTO_REGULAR_FACE: Face<'static> = Face::parse(include_bytes!("../../assets/fonts/NotoSans-Regular.ttf"), 0).unwrap();
    static ref NOTO_BOLD_FACE: Face<'static> = Face::parse(include_bytes!("../../assets/fonts/NotoSans-Bold.ttf"), 0).unwrap();
}

pub fn measure_text_width(text: &str, font_size: f32, bold: bool) -> f32 {
    let face: &Face<'static> = if bold { &NOTO_BOLD_FACE } else { &NOTO_REGULAR_FACE };
    let scale = font_size / face.units_per_em() as f32;
    let mut width = 0.0;
    for ch in text.chars() {
        if let Some(glyph_id) = face.glyph_index(ch) {
            if let Some(advance) = face.glyph_hor_advance(glyph_id) {
                width += advance as f32 * scale;
            }
        }
    }
    width
}

pub fn shrink_to_fit(
    svg: &str,
    name: &str,
    zone: &NameZone,
    canvas_width: f32,
    canvas_height: f32,
) -> Result<String, CoreError> {
    let max_width = zone.w_pct / 100.0 * canvas_width;
    let base_font_size = zone.font_size_pct / 100.0 * canvas_height;
    let min_font_size = base_font_size * 0.4;

    let mut font_size = base_font_size;
    let mut width = measure_text_width(name, font_size, false);

    while width > max_width && font_size > min_font_size {
        font_size *= 0.95;
        width = measure_text_width(name, font_size, false);
    }

    if font_size < min_font_size {
        font_size = min_font_size;
    }

    let mut reader = quick_xml::Reader::from_str(svg);
    reader.trim_text(true);
    let mut writer = quick_xml::Writer::new(Vec::with_capacity(svg.len() + 256));

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => {
                let name_bytes = e.name();
                if name_bytes.as_ref() == b"text" {
                    let mut new_elem = quick_xml::events::BytesStart::new("text");
                    for attr in e.attributes() {
                        let attr = attr.map_err(|e| CoreError::Render(e.to_string()))?;
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        let value = std::str::from_utf8(&attr.value).unwrap_or("");
                        if key == "font-size" {
                            new_elem.push_attribute(("font-size", format!("{}", font_size).as_str()));
                        } else {
                            new_elem.push_attribute((key, value));
                        }
                    }
                    writer.write_event(quick_xml::events::Event::Start(new_elem)).map_err(|e| CoreError::Render(e.to_string()))?;
                } else {
                    writer.write_event(quick_xml::events::Event::Start(e.clone())).map_err(|e| CoreError::Render(e.to_string()))?;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                writer.write_event(quick_xml::events::Event::End(e)).map_err(|e| CoreError::Render(e.to_string()))?;
            }
            Ok(quick_xml::events::Event::Text(e)) => {
                writer.write_event(quick_xml::events::Event::Text(e)).map_err(|e| CoreError::Render(e.to_string()))?;
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(CoreError::Render(e.to_string())),
            Ok(ev) => {
                writer.write_event(ev).map_err(|e| CoreError::Render(e.to_string()))?;
            }
        }
    }

    let result = writer.into_inner();
    let result_str = String::from_utf8(result).map_err(|e| CoreError::Render(e.to_string()))?;
    Ok(result_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measure_text_width() {
        let width = measure_text_width("Hello", 48.0, false);
        assert!(width > 0.0);
    }

    #[test]
    fn test_long_name_shrinks() {
        let long_name = "Maximilian Konstantinopoulos-Fitzgerald";
        let width_normal = measure_text_width(long_name, 48.0, false);
        let width_small = measure_text_width(long_name, 20.0, false);
        assert!(width_small < width_normal);
    }

    #[test]
 fn test_shrink_to_fit_basic() {
        let svg = r##"<svg width="800" height="800"><text x="80" y="560" font-size="48" fill="#111">Maximilian Konstantinopoulos-Fitzgerald</text></svg>"##;
        let zone = NameZone {
            x_pct: 10.0,
            y_pct: 70.0,
            w_pct: 80.0,
            font_size_pct: 6.0,
            color: "#111111".into(),
            align: "center".into(),
        };
        let result = shrink_to_fit(svg, "Maximilian Konstantinopoulos-Fitzgerald", &zone, 800.0, 800.0).unwrap();
        assert!(result.contains("font-size"));
    }
}