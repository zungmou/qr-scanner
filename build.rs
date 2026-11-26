use std::path::PathBuf;

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let ico_path = out_dir.join("app_icon.ico");

        let size = 64u32;
        let mut img = image::RgbaImage::new(size, size);

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
                    img.put_pixel(x, y, image::Rgba([30, 41, 59, 255]));
                }
            }
        }

        let draw_rect = |img: &mut image::RgbaImage, x0: u32, y0: u32, w: u32, h: u32, c: [u8; 4]| {
            for y in y0..(y0 + h) {
                for x in x0..(x0 + w) {
                    if x < size && y < size {
                        img.put_pixel(x, y, image::Rgba(c));
                    }
                }
            }
        };

        let draw_finder = |img: &mut image::RgbaImage, x0: u32, y0: u32| {
            draw_rect(img, x0, y0, 18, 18, [248, 250, 252, 255]);
            draw_rect(img, x0 + 3, y0 + 3, 12, 12, [30, 41, 59, 255]);
            draw_rect(img, x0 + 6, y0 + 6, 6, 6, [56, 189, 248, 255]);
        };

        draw_finder(&mut img, 10, 10);
        draw_finder(&mut img, 36, 10);
        draw_finder(&mut img, 10, 36);

        draw_rect(&mut img, 36, 36, 6, 6, [56, 189, 248, 255]);
        draw_rect(&mut img, 48, 36, 6, 6, [248, 250, 252, 255]);
        draw_rect(&mut img, 36, 48, 6, 6, [248, 250, 252, 255]);
        draw_rect(&mut img, 45, 45, 9, 9, [56, 189, 248, 255]);

        img.save_with_format(&ico_path, image::ImageFormat::Ico)
            .expect("Failed to save ICO");

        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().unwrap());
        res.compile().expect("Failed to compile Windows resources");
    }
}
