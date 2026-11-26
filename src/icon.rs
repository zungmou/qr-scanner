use eframe::egui;
use std::sync::Arc;

/// Generates a 64x64 RGBA icon featuring a stylized QR finder pattern.
pub fn generate_icon_rgba() -> (Vec<u8>, u32, u32) {
    let size = 64u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let set_pixel = |buf: &mut [u8], x: u32, y: u32, r: u8, g: u8, b: u8, a: u8| {
        if x < size && y < size {
            let idx = ((y * size + x) * 4) as usize;
            buf[idx] = r;
            buf[idx + 1] = g;
            buf[idx + 2] = b;
            buf[idx + 3] = a;
        }
    };

    // Rounded dark-slate background (#1E293B) with subtle teal accent
    let radius = 12.0f32;
    for y in 0..size {
        for x in 0..size {
            let dx = if (x as f32) < radius {
                radius - x as f32
            } else if (x as f32) > (size as f32 - 1.0 - radius) {
                x as f32 - (size as f32 - 1.0 - radius)
            } else {
                0.0
            };
            let dy = if (y as f32) < radius {
                radius - y as f32
            } else if (y as f32) > (size as f32 - 1.0 - radius) {
                y as f32 - (size as f32 - 1.0 - radius)
            } else {
                0.0
            };
            if dx * dx + dy * dy <= radius * radius {
                set_pixel(&mut rgba, x, y, 30, 41, 59, 255);
            }
        }
    }

    // Helper to draw a filled rectangle
    let draw_rect = |buf: &mut [u8], x0: u32, y0: u32, w: u32, h: u32, r: u8, g: u8, b: u8| {
        for y in y0..(y0 + h) {
            for x in x0..(x0 + w) {
                set_pixel(buf, x, y, r, g, b, 255);
            }
        }
    };

    // Helper to draw a QR finder pattern (outer 16x16 white, inner 10x10 dark, center 6x6 cyan)
    let draw_finder = |buf: &mut [u8], x0: u32, y0: u32| {
        draw_rect(buf, x0, y0, 18, 18, 248, 250, 252);
        draw_rect(buf, x0 + 3, y0 + 3, 12, 12, 30, 41, 59);
        draw_rect(buf, x0 + 6, y0 + 6, 6, 6, 56, 189, 248);
    };

    // Three QR finder corners
    draw_finder(&mut rgba, 10, 10);
    draw_finder(&mut rgba, 36, 10);
    draw_finder(&mut rgba, 10, 36);

    // Bottom-right data modules in cyan/white
    draw_rect(&mut rgba, 36, 36, 6, 6, 56, 189, 248);
    draw_rect(&mut rgba, 48, 36, 6, 6, 248, 250, 252);
    draw_rect(&mut rgba, 36, 48, 6, 6, 248, 250, 252);
    draw_rect(&mut rgba, 45, 45, 9, 9, 56, 189, 248);

    (rgba, size, size)
}

pub fn create_tray_icon() -> tray_icon::Icon {
    let (rgba, width, height) = generate_icon_rgba();
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon")
}

pub fn create_egui_icon() -> Arc<egui::IconData> {
    let (rgba, width, height) = generate_icon_rgba();
    Arc::new(egui::IconData {
        rgba,
        width,
        height,
    })
}
