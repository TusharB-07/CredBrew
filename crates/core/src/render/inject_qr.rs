use qrcode::EcLevel;
use qrcode::QrCode;
use quick_xml::events::{BytesStart, BytesEnd, Event};
use quick_xml::{Reader, Writer};
use crate::error::CoreError;
use crate::render::QrZone;
use resvg::usvg::TreeParsing;

pub fn inject_qr(
    svg: &str,
    short_code: &str,
    public_base_url: &str,
    zone: &QrZone,
) -> Result<String, CoreError> {
    let verify_url = format!("{}/v/{}", public_base_url.trim_end_matches('/'), short_code);

    let code = QrCode::with_error_correction_level(verify_url.as_bytes(), EcLevel::M)
        .map_err(|e| CoreError::QrGeneration(e.to_string()))?;

    let modules = code.width() + 4; // 2-module quiet zone on each side

    let path_data = generate_qr_path(&code, 2);

    let mut reader = Reader::from_str(svg);
    reader.trim_text(true);

    let mut writer = Writer::new(Vec::with_capacity(svg.len() + 256));
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|e| CoreError::Render(format!("usvg parse error: {}", e)))?;
    let size = tree.size;
    let canvas_width = size.width() as f32;
    let canvas_height = size.height() as f32;

    let qr_size = zone.size_pct / 100.0 * canvas_width.min(canvas_height);
    let scale = qr_size / modules as f32;
    let qr_x = zone.x_pct / 100.0 * canvas_width - qr_size / 2.0;
    let qr_y = zone.y_pct / 100.0 * canvas_height - qr_size / 2.0;

    let mut found_qr_zone = false;

    let transform_attr = format!("translate({:.2},{:.2}) scale({:.6})", qr_x, qr_y, scale);
    let modules_bytes = modules.to_string();

    let transform_key = b"transform";
    let x_key = b"x";
    let y_key = b"y";
    let width_key = b"width";
    let height_key = b"height";
    let fill_key = b"fill";
    let d_key = b"d";
    let zero = b"0";
    let white = b"white";
    let black = b"black";

    let _zero_slice = zero.as_ref();
    let white_slice = white.as_ref();
    let black_slice = black.as_ref();

    let write_qr_group = |writer: &mut Writer<Vec<u8>>| -> Result<(), CoreError> {
        let mut g = BytesStart::new("g");
        g.push_attribute((&transform_key[..], transform_attr.as_bytes()));
        writer.write_event(Event::Start(g)).map_err(|e| CoreError::Render(e.to_string()))?;

        let mut rect = BytesStart::new("rect");
        rect.push_attribute((&x_key[..], &zero[..]));
        rect.push_attribute((&y_key[..], &zero[..]));
        rect.push_attribute((&width_key[..], modules_bytes.as_bytes()));
        rect.push_attribute((&height_key[..], modules_bytes.as_bytes()));
        rect.push_attribute((&fill_key[..], white_slice));
        writer.write_event(Event::Empty(rect)).map_err(|e| CoreError::Render(e.to_string()))?;

        let mut path = BytesStart::new("path");
        path.push_attribute((&d_key[..], path_data.as_bytes()));
        path.push_attribute((&fill_key[..], black_slice));
        writer.write_event(Event::Empty(path)).map_err(|e| CoreError::Render(e.to_string()))?;

        writer.write_event(Event::End(BytesEnd::new("g"))).map_err(|e| CoreError::Render(e.to_string()))?;
        Ok(())
    };

    let has_qr_zone = |e: &BytesStart| -> Result<bool, CoreError> {
        for attr in e.attributes() {
            let attr = attr.map_err(|e| CoreError::Render(e.to_string()))?;
            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
            let value = std::str::from_utf8(&attr.value).unwrap_or("");
            if key == "id" && value == "qr-zone" {
                return Ok(true);
            }
        }
        Ok(false)
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let name_str = std::str::from_utf8(name.as_ref()).unwrap_or("");
                if (name_str == "rect" || name_str == "path" || name_str == "g" || name_str == "text")
                    && has_qr_zone(&e)?
                {
                    found_qr_zone = true;
                    write_qr_group(&mut writer)?;
                    continue;
                }
                writer.write_event(Event::Start(e)).map_err(|e| CoreError::Render(e.to_string()))?;
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let name_str = std::str::from_utf8(name.as_ref()).unwrap_or("");
                if (name_str == "rect" || name_str == "path" || name_str == "g" || name_str == "text")
                    && has_qr_zone(&e)?
                {
                    found_qr_zone = true;
                    write_qr_group(&mut writer)?;
                    continue;
                }
                writer.write_event(Event::Empty(e)).map_err(|e| CoreError::Render(e.to_string()))?;
            }
            Ok(Event::End(e)) => {
                writer.write_event(Event::End(e)).map_err(|e| CoreError::Render(e.to_string()))?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::Render(e.to_string())),
            Ok(ev) => {
                writer.write_event(ev).map_err(|e| CoreError::Render(e.to_string()))?;
            }
        }
    }

    if !found_qr_zone {
        return Err(CoreError::Render("SVG must contain an element with id=\"qr-zone\"".into()));
    }

    let result = writer.into_inner();
    let result_str = String::from_utf8(result).map_err(|e| CoreError::Render(e.to_string()))?;
    Ok(result_str)
}

fn generate_qr_path(code: &QrCode, quiet_zone: usize) -> String {
    let width = code.width();
    let size = width + 2 * quiet_zone;
    let mut path = String::new();

    for y in 0..size {
        let mut x = 0;
        while x < size {
            let is_dark = if x >= quiet_zone && x < quiet_zone + width && y >= quiet_zone && y < quiet_zone + width {
                matches!(code[(x - quiet_zone, y - quiet_zone)], qrcode::Color::Dark)
            } else {
                false
            };

            if is_dark {
                let start = x;
                while x < size {
                    let is_dark_cont = if x >= quiet_zone && x < quiet_zone + width && y >= quiet_zone && y < quiet_zone + width {
                        matches!(code[(x - quiet_zone, y - quiet_zone)], qrcode::Color::Dark)
                    } else {
                        false
                    };
                    if !is_dark_cont {
                        break;
                    }
                    x += 1;
                }
                path.push_str(&format!("M{} {}h{}v1h{}z ", start, y, x - start, -(x as i32 - start as i32)));
            } else {
                x += 1;
            }
        }
    }

    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_generation() {
        let code = QrCode::with_error_correction_level(b"https://example.com/v/abc12345", EcLevel::M).unwrap();
        assert!(code.width() > 0);
    }

    #[test]
    fn test_qr_path_generation() {
        let code = QrCode::with_error_correction_level(b"https://example.com/v/abc12345", EcLevel::M).unwrap();
        let path = generate_qr_path(&code, 2);
        assert!(path.contains('M'));
        assert!(path.contains('h'));
        assert!(path.contains('v'));
        assert!(path.contains('z'));
    }

    #[test]
    fn test_inject_qr_basic() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="800"><rect id="qr-zone" x="0" y="0" width="80" height="80"/></svg>"#;
        let zone = QrZone {
            x_pct: 85.0,
            y_pct: 5.0,
            size_pct: 10.0,
        };
        let result = inject_qr(svg, "abc12345", "https://example.com", &zone).unwrap();
        assert!(result.contains("<g transform="));
        assert!(result.contains("<path d="));
        assert!(!result.contains("id=\"qr-zone\""));
    }

    #[test]
    fn test_qr_deterministic() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="800"><rect id="qr-zone" x="0" y="0" width="80" height="80"/></svg>"#;
        let zone = QrZone {
            x_pct: 85.0,
            y_pct: 5.0,
            size_pct: 10.0,
        };
        let result1 = inject_qr(svg, "abc12345", "https://example.com", &zone).unwrap();
        let result2 = inject_qr(svg, "abc12345", "https://example.com", &zone).unwrap();
        assert_eq!(result1, result2);
    }
}