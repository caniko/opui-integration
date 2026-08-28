use std::path::Path;

use image::{Rgba, RgbaImage};

pub const VISUAL_MAE: f64 = 18.0;
pub const VISUAL_MAX: u8 = 220;
pub const POLICY_ID: &str = "opui-v1-visual-18-220";

#[derive(Clone, Debug)]
pub struct ImageStats {
    pub w: u32,
    pub h: u32,
    pub min: [u8; 4],
    pub max: [u8; 4],
    pub mean: [f64; 4],
    pub var: [f64; 4],
    pub unique_buckets: usize,
    pub clear_ratio: f64,
    pub bbox: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Debug)]
pub struct DiffStats {
    pub mae: f64,
    pub rmse: f64,
    pub max: u8,
    pub exact_diff_ratio: f64,
    pub thresh_diff_ratio: f64,
    pub bbox: Option<(u32, u32, u32, u32)>,
}

pub fn load_rgba(path: &Path) -> Result<RgbaImage, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(img.to_rgba8())
}

pub fn stats(img: &RgbaImage, clear: [u8; 4]) -> ImageStats {
    let (w, h) = img.dimensions();
    let n = (w * h) as usize;
    let mut min = [255u8; 4];
    let mut max = [0u8; 4];
    let mut sum = [0u64; 4];
    let mut clear_n = 0usize;
    let mut buckets = [0u8; 4096];
    let mut x0 = w;
    let mut y0 = h;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    for (x, y, p) in img.enumerate_pixels() {
        let c = p.0;
        for i in 0..4 {
            min[i] = min[i].min(c[i]);
            max[i] = max[i].max(c[i]);
            sum[i] += c[i] as u64;
        }
        if c == clear {
            clear_n += 1;
        } else {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let b = ((c[0] as usize >> 4) << 8) | ((c[1] as usize >> 4) << 4) | (c[2] as usize >> 4);
        buckets[b] = 1;
    }
    let mean = [
        sum[0] as f64 / n as f64,
        sum[1] as f64 / n as f64,
        sum[2] as f64 / n as f64,
        sum[3] as f64 / n as f64,
    ];
    let mut acc = [0f64; 4];
    for p in img.pixels() {
        for i in 0..4 {
            let d = p.0[i] as f64 - mean[i];
            acc[i] += d * d;
        }
    }
    let bbox = (clear_n < n).then_some((x0, y0, x1, y1));
    ImageStats {
        w,
        h,
        min,
        max,
        mean,
        var: [
            acc[0] / n as f64,
            acc[1] / n as f64,
            acc[2] / n as f64,
            acc[3] / n as f64,
        ],
        unique_buckets: buckets.iter().filter(|b| **b == 1).count(),
        clear_ratio: clear_n as f64 / n as f64,
        bbox,
    }
}

pub fn reject_corrupt(img: &RgbaImage, expect_w: u32, expect_h: u32) -> Result<ImageStats, String> {
    if img.width() != expect_w || img.height() != expect_h {
        return Err(format!(
            "size {}x{} want {expect_w}x{expect_h}",
            img.width(),
            img.height()
        ));
    }
    let raw = img.as_raw();
    if raw.len() != (expect_w * expect_h * 4) as usize {
        return Err("truncated image".into());
    }
    let s = stats(img, [0, 0, 0, 0]);
    if s.min == [0, 0, 0, 0] && s.max == [0, 0, 0, 0] {
        return Err("transparent uniform image".into());
    }
    if s.min == [0, 0, 0, 255] && s.max == [0, 0, 0, 255] {
        return Err("opaque black image".into());
    }
    if s.unique_buckets <= 1 && s.var.iter().all(|v| *v < 1e-6) {
        return Err("opaque clear-color image".into());
    }
    Ok(s)
}

pub fn assert_control(img: &RgbaImage) -> Result<(), String> {
    let s = reject_corrupt(img, img.width(), img.height())?;
    let (w, h) = img.dimensions();
    let corners = [
        img.get_pixel(0, 0).0,
        img.get_pixel(w - 1, 0).0,
        img.get_pixel(0, h - 1).0,
        img.get_pixel(w - 1, h - 1).0,
    ];
    for c in corners {
        if !near(c, [128, 128, 128, 255], 12) {
            return Err(format!("control corner not gray: {c:?}"));
        }
    }
    let cx = w / 2;
    let cy = h / 2;
    let mid = img.get_pixel(cx, cy).0;
    if !near(mid, [0, 0, 255, 255], 12) {
        return Err(format!("control center not blue: {mid:?}"));
    }
    let _ = s;
    let mut x0 = img.width();
    let mut y0 = img.height();
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    let mut found = false;
    for (x, y, p) in img.enumerate_pixels() {
        if near(p.0, [0, 0, 255, 255], 12) {
            found = true;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if !found {
        return Err("control has no blue pixels".into());
    }
    let bw = x1.saturating_sub(x0) + 1;
    let bh = y1.saturating_sub(y0) + 1;
    if !(150..=180).contains(&bw) || !(70..=90).contains(&bh) {
        return Err(format!("control blue bbox {bw}x{bh}, want ~160x80"));
    }
    Ok(())
}

pub fn diff(a: &RgbaImage, b: &RgbaImage) -> Result<DiffStats, String> {
    if a.dimensions() != b.dimensions() {
        return Err(format!(
            "size {}x{} vs {}x{}",
            a.width(),
            a.height(),
            b.width(),
            b.height()
        ));
    }
    let n = a.as_raw().len();
    let mut mae = 0u64;
    let mut sq = 0u64;
    let mut max = 0u8;
    let mut exact = 0usize;
    let mut thresh = 0usize;
    let mut x0 = a.width();
    let mut y0 = a.height();
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        let mut pix_diff = false;
        let mut pix_th = false;
        for i in 0..4 {
            let d = pa.0[i].abs_diff(pb.0[i]);
            mae += d as u64;
            sq += (d as u64) * (d as u64);
            max = max.max(d);
            if d > 0 {
                pix_diff = true;
            }
            if d > 8 {
                pix_th = true;
            }
        }
        if pix_diff {
            exact += 1;
        }
        if pix_th {
            thresh += 1;
        }
    }
    for (x, y, (pa, pb)) in a
        .enumerate_pixels()
        .zip(b.pixels())
        .map(|((x, y, pa), pb)| (x, y, (pa, pb)))
    {
        if pa.0 != pb.0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let pixels = (a.width() * a.height()) as f64;
    Ok(DiffStats {
        mae: mae as f64 / n as f64,
        rmse: (sq as f64 / n as f64).sqrt(),
        max,
        exact_diff_ratio: exact as f64 / pixels,
        thresh_diff_ratio: thresh as f64 / pixels,
        bbox: (exact > 0).then_some((x0, y0, x1, y1)),
    })
}

pub fn heatmap(a: &RgbaImage, b: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::new(a.width(), a.height());
    for (x, y, (pa, pb)) in a
        .enumerate_pixels()
        .zip(b.pixels())
        .map(|((x, y, pa), pb)| (x, y, (pa, pb)))
    {
        let d =
            pa.0.iter()
                .zip(pb.0.iter())
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
        out.put_pixel(x, y, Rgba([d, 0, 0, 255]));
    }
    out
}

pub fn tile_mae(a: &RgbaImage, b: &RgbaImage, tile: u32) -> Vec<(u32, u32, f64)> {
    let mut out = Vec::new();
    let mut y = 0;
    while y < a.height() {
        let mut x = 0;
        while x < a.width() {
            let x1 = (x + tile).min(a.width());
            let y1 = (y + tile).min(a.height());
            let mut s = 0u64;
            let mut n = 0u64;
            for yy in y..y1 {
                for xx in x..x1 {
                    let pa = a.get_pixel(xx, yy).0;
                    let pb = b.get_pixel(xx, yy).0;
                    for i in 0..4 {
                        s += pa[i].abs_diff(pb[i]) as u64;
                        n += 1;
                    }
                }
            }
            out.push((x, y, s as f64 / n as f64));
            x += tile;
        }
        y += tile;
    }
    out
}

pub fn passes_visual(d: &DiffStats) -> bool {
    d.mae <= VISUAL_MAE && d.max <= VISUAL_MAX
}

fn near(c: [u8; 4], t: [u8; 4], tol: u8) -> bool {
    c.iter().zip(t.iter()).all(|(a, b)| a.abs_diff(*b) <= tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    #[test]
    fn rejects_transparent() {
        let img = solid(8, 8, [0, 0, 0, 0]);
        assert!(
            reject_corrupt(&img, 8, 8)
                .unwrap_err()
                .contains("transparent")
        );
    }

    #[test]
    fn rejects_black() {
        let img = solid(8, 8, [0, 0, 0, 255]);
        assert!(reject_corrupt(&img, 8, 8).unwrap_err().contains("black"));
    }

    #[test]
    fn rejects_uniform_clear() {
        let img = solid(8, 8, [32, 32, 32, 255]);
        assert!(
            reject_corrupt(&img, 8, 8)
                .unwrap_err()
                .contains("clear-color")
        );
    }

    #[test]
    fn rejects_wrong_size() {
        let img = solid(4, 4, [1, 2, 3, 255]);
        assert!(reject_corrupt(&img, 8, 8).unwrap_err().contains("size"));
    }

    #[test]
    fn rejects_truncated_file() {
        let dir = std::env::temp_dir().join("opui-trunc.png");
        std::fs::write(&dir, b"\x89PNG\r\n\x1a\ntrunc").unwrap();
        assert!(load_rgba(&dir).is_err());
        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn control_geometry() {
        let mut img = solid(320, 180, [128, 128, 128, 255]);
        for y in 50..130 {
            for x in 80..240 {
                img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
        assert_control(&img).unwrap();
    }
}
