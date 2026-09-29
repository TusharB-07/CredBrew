use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::Reader;
use quick_xml::Writer;
use std::io::Cursor;
use crate::error::CoreError;

const MAX_DATA_URI_SIZE: usize = 500_000;

pub fn sanitize_svg(svg: &str) -> Result<String, CoreError> {
    let mut reader = Reader::from_str(svg);
    reader.trim_text(true);

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut dangerous = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name_str = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();

                if name_str == "script" || name_str == "foreignObject" {
                    dangerous = true;
                    skip_element(&mut reader, &name_str);
                    continue;
                }

                let attrs_to_write = filter_attributes(&name_str, &e, &mut dangerous)?;

                let mut bytes_start = BytesStart::new(&name_str);
                for (key, value) in attrs_to_write {
                    bytes_start.push_attribute((key.as_bytes(), value.as_bytes()));
                }
                writer.write_event(Event::Start(bytes_start)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::Empty(e)) => {
                let name_str = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();

                if name_str == "script" || name_str == "foreignObject" {
                    dangerous = true;
                    continue;
                }

                let attrs_to_write = filter_attributes(&name_str, &e, &mut dangerous)?;

                let mut bytes_start = BytesStart::new(&name_str);
                for (key, value) in attrs_to_write {
                    bytes_start.push_attribute((key.as_bytes(), value.as_bytes()));
                }
                writer.write_event(Event::Empty(bytes_start)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::End(e)) => {
                writer.write_event(Event::End(e)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().map_err(|e| CoreError::Sanitization(e.to_string()))?;
                writer.write_event(Event::Text(BytesText::new(&text))).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::CData(e)) => {
                writer.write_event(Event::CData(e)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::Comment(e)) => {
                writer.write_event(Event::Comment(e)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::Decl(e)) => {
                writer.write_event(Event::Decl(e)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::PI(e)) => {
                writer.write_event(Event::PI(e)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::DocType(e)) => {
                writer.write_event(Event::DocType(e)).map_err(|e| CoreError::Sanitization(e.to_string()))?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::Sanitization(e.to_string())),
        }
    }

    let result = writer.into_inner().into_inner();
    let result_str = String::from_utf8(result).map_err(|e| CoreError::Sanitization(e.to_string()))?;

    if dangerous {
        tracing::warn!("SVG sanitization removed dangerous content");
    }

    Ok(result_str)
}

fn filter_attributes(
    name_str: &str,
    e: &BytesStart,
    dangerous: &mut bool,
) -> Result<Vec<(String, String)>, CoreError> {
    let mut attrs_to_write = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| CoreError::Sanitization(e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("").to_string();
        let value = std::str::from_utf8(&attr.value).unwrap_or("").to_string();

        if key.starts_with("on") {
            *dangerous = true;
            continue;
        }

        if (key == "href" || key == "xlink:href")
            && !value.starts_with('#')
            && !value.starts_with("data:") {
            *dangerous = true;
            continue;
        }

        if key == "href" && value.starts_with("javascript:") {
            *dangerous = true;
            continue;
        }

        if name_str == "image"
            && (key == "href" || key == "xlink:href")
            && value.starts_with("data:")
            && value.len() > MAX_DATA_URI_SIZE
        {
            *dangerous = true;
            continue;
        }

        attrs_to_write.push((key, value));
    }
    Ok(attrs_to_write)
}

fn skip_element(reader: &mut Reader<&[u8]>, element_name: &str) {
    let mut depth = 1;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if std::str::from_utf8(e.name().as_ref()).unwrap_or("") == element_name {
                    depth += 1;
                }
            }
            Ok(Event::End(e)) => {
                if std::str::from_utf8(e.name().as_ref()).unwrap_or("") == element_name {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
            }
            Ok(Event::Eof) => return,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_svg_passes() {
        let svg = r#"<svg><rect x="0" y="0" width="100" height="100"/></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(result.contains("rect"));
    }

    #[test]
    fn test_script_stripped() {
        let svg = r#"<svg><script>alert(1)</script></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(!result.contains("script"));
    }

    #[test]
    fn test_onclick_stripped() {
        let svg = r#"<svg><rect onclick="alert(1)"/></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(!result.contains("onclick"));
    }

    #[test]
    fn test_javascript_href_stripped() {
        let svg = r#"<svg><a href="javascript:alert(1)"/></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(!result.contains("javascript:"));
    }

    #[test]
    fn test_external_href_stripped() {
        let svg = r#"<svg><a href="http://evil.com"/></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(!result.contains("evil.com"));
    }

    #[test]
    fn test_data_uri_allowed() {
        let svg = r#"<svg><image href="data:image/png;base64,abc"/></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(result.contains("data:image/png"));
    }

    #[test]
    fn test_large_data_uri_rejected() {
        let large = "x".repeat(600_000);
        let svg = format!(r#"<svg><image href="data:image/png;base64,{large}"/></svg>"#);
        let result = sanitize_svg(&svg).unwrap();
        assert!(!result.contains("data:image/png"));
    }

    #[test]
    fn test_foreign_object_stripped() {
        let svg = r#"<svg><foreignObject><body/></foreignObject></svg>"#;
        let result = sanitize_svg(svg).unwrap();
        assert!(!result.contains("foreignObject"));
    }

    #[test]
    fn test_fragment_identifier_allowed() {
        let svg = r##"<svg><a href="#internal"/></svg>"##;
        let result = sanitize_svg(svg).unwrap();
        assert!(result.contains("#internal"));
    }
}