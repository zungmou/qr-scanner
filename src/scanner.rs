use image::{DynamicImage, GrayImage, RgbaImage};
use xcap::Monitor;

pub struct ScreenSnapshot {
    pub image: RgbaImage,
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
}

/// Captures the primary monitor (or the monitor containing the cursor).
pub fn capture_primary_screen() -> Result<ScreenSnapshot, String> {
    let monitors = Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| Monitor::all().ok()?.into_iter().next())
        .ok_or_else(|| "No monitor found".to_string())?;

    let origin_x = monitor.x().unwrap_or(0);
    let origin_y = monitor.y().unwrap_or(0);
    let image = monitor
        .capture_image()
        .map_err(|e| format!("Failed to capture screen: {e}"))?;

    let width = image.width();
    let height = image.height();

    Ok(ScreenSnapshot {
        image,
        origin_x,
        origin_y,
        width,
        height,
    })
}

/// Crops the snapshot to the given pixel rectangle and decodes any QR codes inside.
pub fn decode_qr_in_region(
    snapshot: &ScreenSnapshot,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Vec<String> {
    if w < 8 || h < 8 {
        return Vec::new();
    }

    let clamped_x = x.min(snapshot.width.saturating_sub(1));
    let clamped_y = y.min(snapshot.height.saturating_sub(1));
    let clamped_w = w.min(snapshot.width.saturating_sub(clamped_x));
    let clamped_h = h.min(snapshot.height.saturating_sub(clamped_y));

    if clamped_w < 8 || clamped_h < 8 {
        return Vec::new();
    }

    let cropped = image::imageops::crop_imm(
        &snapshot.image,
        clamped_x,
        clamped_y,
        clamped_w,
        clamped_h,
    )
    .to_image();

    let gray: GrayImage = DynamicImage::ImageRgba8(cropped).to_luma8();
    let width = gray.width();
    let height = gray.height();
    let mut results = Vec::new();

    // 1. Decode QR codes with rqrr
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(
        width as usize,
        height as usize,
        |x, y| gray.get_pixel(x as u32, y as u32).0[0],
    );
    let grids = prepared.detect_grids();
    for grid in grids {
        if let Ok((_meta, content)) = grid.decode() {
            if !content.is_empty() && !results.contains(&content) {
                results.push(content);
            }
        }
    }

    // 2. Decode barcodes and other 1D/2D formats with rxing
    let luma_data = gray.into_raw();
    if let Ok(multi_results) = rxing::helpers::detect_multiple_in_luma_with_hints(
        luma_data,
        width,
        height,
        &mut rxing::DecodeHints {
            TryHarder: Some(true),
            ..Default::default()
        },
    ) {
        for res in multi_results {
            let text = res.getText().to_string();
            if !text.is_empty() && !results.contains(&text) {
                results.push(text);
            }
        }
    }

    results
}
