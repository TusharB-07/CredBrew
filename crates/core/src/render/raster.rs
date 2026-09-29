use resvg::tiny_skia::Pixmap;
use resvg::usvg::{TreeParsing, TreeTextToPath};
use crate::error::CoreError;

const MAX_DIMENSION: u32 = 2048;
const CERT_SCALE: f32 = 300.0 / 96.0; // usvg uses 96 DPI base

fn build_font_db() -> resvg::usvg::fontdb::Database {
    let mut db = resvg::usvg::fontdb::Database::new();
    db.load_font_data(include_bytes!("../../assets/fonts/NotoSans-Regular.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/fonts/NotoSans-Bold.ttf").to_vec());
    db.set_sans_serif_family("Noto Sans");
    db
}

pub fn render_png(svg: &str) -> Result<Vec<u8>, CoreError> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|e| CoreError::Render(format!("usvg parse error: {}", e)))?;

    // Convert text to paths using our font database
    let mut tree = tree;
    tree.convert_text(&build_font_db());

    let size = tree.size;
    let scale = (MAX_DIMENSION as f32 / size.width() as f32).min(1.0).min((MAX_DIMENSION as f32 / size.height() as f32).min(1.0));
    let width = (size.width() as f32 * scale).ceil() as u32;
    let height = (size.height() as f32 * scale).ceil() as u32;

    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| CoreError::Render("failed to create pixmap".into()))?;

    resvg::render(
        &tree,
        resvg::FitTo::Original,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        pixmap.as_mut(),
    );

    let png = pixmap.encode_png()
        .map_err(|e| CoreError::Render(format!("PNG encode error: {}", e)))?;

    Ok(png)
}

pub fn render_cert_png(svg: &str) -> Result<Vec<u8>, CoreError> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|e| CoreError::Render(format!("usvg parse error: {}", e)))?;

    // Convert text to paths using our font database
    let mut tree = tree;
    tree.convert_text(&build_font_db());

    let size = tree.size;
    let width = (size.width() as f32 * CERT_SCALE).ceil() as u32;
    let height = (size.height() as f32 * CERT_SCALE).ceil() as u32;

    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| CoreError::Render("failed to create pixmap".into()))?;

    resvg::render(
        &tree,
        resvg::FitTo::Original,
        resvg::tiny_skia::Transform::from_scale(CERT_SCALE, CERT_SCALE),
        pixmap.as_mut(),
    );

    let png = pixmap.encode_png()
        .map_err(|e| CoreError::Render(format!("PNG encode error: {}", e)))?;

    Ok(png)
}

pub fn get_font_db() -> std::sync::Arc<fontdb::Database> {
    static FONT_DB: once_cell::sync::Lazy<std::sync::Arc<fontdb::Database>> = once_cell::sync::Lazy::new(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../../assets/fonts/NotoSans-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/fonts/NotoSans-Bold.ttf").to_vec());
        db.set_sans_serif_family("Noto Sans");
        std::sync::Arc::new(db)
    });
    FONT_DB.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_png() {
        let svg = r#"<svg width="800" height="800" xmlns="http://www.w3.org/2000/svg"><rect width="800" height="800" fill="white"/><text x="400" y="400" font-size="48" text-anchor="middle" fill="black">Test</text></svg>"#;
        let png = render_png(svg).unwrap();
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]));
    }

    #[test]
    fn test_render_cert_png() {
        let svg = r#"<svg width="1122" height="794" xmlns="http://www.w3.org/2000/svg"><rect width="1122" height="794" fill="white"/><text x="561" y="397" font-size="48" text-anchor="middle" fill="black">Test</text></svg>"#;
        let png = render_cert_png(svg).unwrap();
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]));
    }
}