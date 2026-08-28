use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use serde::Serialize;

use crate::image_metrics::{VISUAL_MAE, VISUAL_MAX, load_rgba};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        Self {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }

    fn contains(self, x: u32, y: u32) -> bool {
        let x = x as f64;
        let y = y as f64;
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: u64,
}

#[derive(Debug, Serialize)]
pub struct MaxErrorSample {
    pub x: u32,
    pub y: u32,
    pub reference_rgba: [u8; 4],
    pub capture_rgba: [u8; 4],
    pub candidate_overlapping_source_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct NodeDiff {
    pub source_id: String,
    pub runtime_id: Option<String>,
    pub computed_rectangle: Rect,
    pub clipping_rectangle: Rect,
    pub stack_index: Option<usize>,
    pub pixel_intersection: PixelRect,
    pub mae: f64,
    pub per_channel_mae: [f64; 4],
    pub rmse: f64,
    pub max_error: u8,
    pub exact_differing_pixel_ratio: f64,
    pub thresholded_differing_pixel_ratio: f64,
    pub maximum_error_samples: Vec<MaxErrorSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct NodeDiffReport {
    schema_version: u32,
    run: String,
    source_tree: String,
    nodes: Vec<NodeDiff>,
}

struct NodeGeometry {
    source_id: String,
    runtime_id: Option<String>,
    rect: Rect,
    clip: Rect,
    stack_index: Option<usize>,
    pixels: PixelRect,
}

pub fn write_node_diff(root: &Path, run: &Path, package: &Path) -> Result<PathBuf, String> {
    let reference = load_rgba(&run.join("reference.png"))?;
    let capture = load_rgba(&run.join("capture.png"))?;
    if reference.dimensions() != capture.dimensions() {
        return Err("reference and capture dimensions differ".into());
    }
    let package: serde_json::Value = read_json(package)?;
    let computed: Vec<serde_json::Value> = read_json(&run.join("computed.json"))?;
    let mapping: Vec<serde_json::Value> = read_json(&run.join("mapping.json"))?;
    let nodes = package["nodes"]
        .as_object()
        .ok_or("package nodes missing")?;
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: capture.width() as f64,
        height: capture.height() as f64,
    };
    let rects = computed
        .iter()
        .filter_map(|row| {
            let source_id = row["source_id"].as_str()?.to_string();
            let width = row["width"].as_f64()?;
            let height = row["height"].as_f64()?;
            let x = row["x"].as_f64()? - width / 2.0;
            let y = row["y"].as_f64()? - height / 2.0;
            Some((
                source_id,
                Rect {
                    x,
                    y,
                    width,
                    height,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let runtime_ids = mapping
        .iter()
        .filter_map(|row| {
            Some((
                row["source_id"].as_str()?.to_string(),
                row["runtime_id"].as_str().map(str::to_string),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut parents = BTreeMap::new();
    for (parent, node) in nodes {
        for (index, child) in node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .enumerate()
        {
            parents.insert(child.to_string(), (parent.clone(), index));
        }
    }
    let geometries = rects
        .iter()
        .map(|(source_id, rect)| {
            let mut clip = viewport;
            let mut cursor = source_id.as_str();
            let mut seen = BTreeSet::new();
            while let Some((parent, _)) = parents.get(cursor) {
                if !seen.insert(parent.as_str()) {
                    break;
                }
                if node_clips(&nodes[parent])
                    && let Some(parent_rect) = rects.get(parent)
                {
                    clip = clip.intersection(*parent_rect);
                }
                cursor = parent;
            }
            let visible = rect.intersection(clip).intersection(viewport);
            NodeGeometry {
                source_id: source_id.clone(),
                runtime_id: runtime_ids.get(source_id).cloned().flatten(),
                rect: *rect,
                clip,
                stack_index: parents.get(source_id).map(|(_, index)| *index),
                pixels: pixel_rect(visible, capture.width(), capture.height()),
            }
        })
        .collect::<Vec<_>>();
    let crop_root = run.join("node-diff-crops");
    let mut diffs = Vec::with_capacity(geometries.len());
    for geometry in &geometries {
        let mut diff = compare_node(&reference, &capture, geometry, &geometries);
        if diff.mae > VISUAL_MAE || diff.max_error > VISUAL_MAX {
            diff.artifacts = write_crops(&reference, &capture, &crop_root, geometry)?;
        }
        diffs.push(diff);
    }
    let path = run.join("node-diff.json");
    let report = NodeDiffReport {
        schema_version: 1,
        run: run.display().to_string(),
        source_tree: crate::lock::tracked_source_tree(root)?,
        nodes: diffs,
    };
    fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    Ok(path)
}

fn node_clips(node: &serde_json::Value) -> bool {
    node["layout"]["overflow"] == "hidden" || node["style"]["clipping"] == true
}

fn pixel_rect(rect: Rect, width: u32, height: u32) -> PixelRect {
    let x = rect.x.floor().clamp(0.0, width as f64) as u32;
    let y = rect.y.floor().clamp(0.0, height as f64) as u32;
    let right = (rect.x + rect.width).ceil().clamp(0.0, width as f64) as u32;
    let bottom = (rect.y + rect.height).ceil().clamp(0.0, height as f64) as u32;
    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    PixelRect {
        x,
        y,
        width,
        height,
        pixels: width as u64 * height as u64,
    }
}

fn compare_node(
    reference: &RgbaImage,
    capture: &RgbaImage,
    node: &NodeGeometry,
    geometries: &[NodeGeometry],
) -> NodeDiff {
    let area = node.pixels;
    let channels = area.pixels * 4;
    let mut sum = [0u64; 4];
    let mut square_sum = 0u64;
    let mut max_error = 0u8;
    let mut exact = 0u64;
    let mut thresholded = 0u64;
    let mut samples = Vec::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let a = reference.get_pixel(x, y).0;
            let b = capture.get_pixel(x, y).0;
            let differences =
                std::array::from_fn::<_, 4, _>(|channel| a[channel].abs_diff(b[channel]));
            let pixel_max = differences.into_iter().max().unwrap_or(0);
            for channel in 0..4 {
                sum[channel] += differences[channel] as u64;
                square_sum += differences[channel] as u64 * differences[channel] as u64;
            }
            exact += u64::from(pixel_max > 0);
            thresholded += u64::from(pixel_max > 8);
            if pixel_max > max_error {
                max_error = pixel_max;
                samples.clear();
            }
            if pixel_max == max_error && pixel_max > 0 && samples.len() < 32 {
                samples.push(MaxErrorSample {
                    x,
                    y,
                    reference_rgba: a,
                    capture_rgba: b,
                    candidate_overlapping_source_ids: geometries
                        .iter()
                        .filter(|candidate| {
                            candidate.pixels.pixels > 0
                                && candidate.clip.contains(x, y)
                                && candidate.rect.contains(x, y)
                        })
                        .map(|candidate| candidate.source_id.clone())
                        .collect(),
                });
            }
        }
    }
    let pixels = area.pixels.max(1) as f64;
    let channels = channels.max(1) as f64;
    NodeDiff {
        source_id: node.source_id.clone(),
        runtime_id: node.runtime_id.clone(),
        computed_rectangle: node.rect,
        clipping_rectangle: node.clip,
        stack_index: node.stack_index,
        pixel_intersection: area,
        mae: sum.iter().sum::<u64>() as f64 / channels,
        per_channel_mae: std::array::from_fn(|channel| sum[channel] as f64 / pixels),
        rmse: (square_sum as f64 / channels).sqrt(),
        max_error,
        exact_differing_pixel_ratio: exact as f64 / pixels,
        thresholded_differing_pixel_ratio: thresholded as f64 / pixels,
        maximum_error_samples: samples,
        artifacts: None,
    }
}

fn write_crops(
    reference: &RgbaImage,
    capture: &RgbaImage,
    root: &Path,
    node: &NodeGeometry,
) -> Result<Option<BTreeMap<String, String>>, String> {
    let area = node.pixels;
    if area.pixels == 0 {
        return Ok(None);
    }
    let dir = root.join(safe_name(&node.source_id));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let reference_crop =
        image::imageops::crop_imm(reference, area.x, area.y, area.width, area.height).to_image();
    let capture_crop =
        image::imageops::crop_imm(capture, area.x, area.y, area.width, area.height).to_image();
    let mut heatmap = RgbaImage::new(area.width, area.height);
    for (x, y, pixel) in heatmap.enumerate_pixels_mut() {
        let a = reference_crop.get_pixel(x, y).0;
        let b = capture_crop.get_pixel(x, y).0;
        let error = a
            .iter()
            .zip(b)
            .map(|(left, right)| left.abs_diff(right))
            .max()
            .unwrap_or(0)
            .saturating_mul(4);
        *pixel = Rgba([error, 0, 0, 255]);
    }
    let paths = [
        ("reference", dir.join("reference.png"), reference_crop),
        ("capture", dir.join("capture.png"), capture_crop),
        ("heatmap", dir.join("heatmap-amplified.png"), heatmap),
    ];
    let mut artifacts = BTreeMap::new();
    for (name, path, image) in paths {
        image.save(&path).map_err(|e| e.to_string())?;
        artifacts.insert(name.to_string(), path.display().to_string());
    }
    Ok(Some(artifacts))
}

fn safe_name(source_id: &str) -> String {
    source_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_only_intersecting_pixels() {
        let reference = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        let mut capture = reference.clone();
        capture.put_pixel(2, 2, Rgba([40, 20, 0, 255]));
        let node = NodeGeometry {
            source_id: "node".into(),
            runtime_id: Some("runtime".into()),
            rect: Rect {
                x: 1.0,
                y: 1.0,
                width: 2.0,
                height: 2.0,
            },
            clip: Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            stack_index: Some(0),
            pixels: PixelRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
                pixels: 4,
            },
        };
        let diff = compare_node(&reference, &capture, &node, std::slice::from_ref(&node));
        assert_eq!(diff.max_error, 40);
        assert_eq!(
            diff.maximum_error_samples[0].candidate_overlapping_source_ids,
            ["node"]
        );
        assert_eq!(diff.exact_differing_pixel_ratio, 0.25);
    }
}
