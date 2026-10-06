//! `~/.config/tobii.json`. The display plane is width, height, depth, tilt,
//! and a center expressed as a number or an anchor such as `b - 10`.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayArea {
    pub w_mm: f64,
    pub h_mm: f64,
    pub ox_mm: f64,
    pub oy_mm: f64,
    pub z_mm: f64,
    pub tilt_deg: f64,
}

impl Default for DisplayArea {
    fn default() -> Self {
        Self {
            w_mm: 1500.0,
            h_mm: 1000.0,
            ox_mm: -750.0,
            oy_mm: -500.0,
            z_mm: 0.0,
            tilt_deg: 0.0,
        }
    }
}

impl DisplayArea {
    /// Corners sent to the device, with tilt rotating the top edge around X.
    pub fn corners(self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let angle = self.tilt_deg * std::f64::consts::PI / 180.0;
        let (sin_a, cos_a) = angle.sin_cos();
        let bl = [self.ox_mm, self.oy_mm, self.z_mm];
        let tl = [
            self.ox_mm,
            self.oy_mm + self.h_mm * cos_a,
            self.z_mm + self.h_mm * sin_a,
        ];
        let tr = [self.ox_mm + self.w_mm, tl[1], tl[2]];
        (tl, tr, bl)
    }
}

pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/tobii.json"))
}

pub fn warp_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/tobii/screen_warp.json"))
}

pub fn load_display_area() -> DisplayArea {
    let Some(path) = config_path() else {
        return DisplayArea::default();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return DisplayArea::default();
    };
    parse_display_area(&text).unwrap_or_default()
}

pub fn parse_display_area(text: &str) -> Option<DisplayArea> {
    let value: Value = serde_json::from_str(text).ok()?;
    let obj = value.get("display_area")?.as_object()?;
    let mut area = DisplayArea::default();
    if let Some(v) = obj.get("w_mm").and_then(Value::as_f64) {
        area.w_mm = v;
    }
    if let Some(v) = obj.get("h_mm").and_then(Value::as_f64) {
        area.h_mm = v;
    }
    if let Some(v) = obj.get("z_mm").and_then(Value::as_f64) {
        area.z_mm = v;
    }
    if let Some(v) = obj.get("tilt").and_then(Value::as_f64) {
        area.tilt_deg = v;
    }
    let half_w = area.w_mm / 2.0;
    let half_h = area.h_mm / 2.0;
    if let Some(cx) = obj.get("cx").and_then(|v| position(v, half_w, false)) {
        area.ox_mm = -cx - half_w;
    }
    if let Some(cy) = obj.get("cy").and_then(|v| position(v, half_h, true)) {
        area.oy_mm = -cy - half_h;
    }
    Some(area)
}

pub fn load_screen_warp() -> Option<[f64; 9]> {
    let path = warp_path()?;
    let text = fs::read_to_string(path).ok()?;
    parse_screen_warp(&text)
}

pub fn parse_screen_warp(text: &str) -> Option<[f64; 9]> {
    let value: Value = serde_json::from_str(text).ok()?;
    let rows = value.get("homography")?.as_array()?;
    if rows.len() != 3 {
        return None;
    }
    let mut out = [0.0; 9];
    for (row_index, row) in rows.iter().enumerate() {
        let row = row.as_array()?;
        if row.len() != 3 {
            return None;
        }
        for (col_index, cell) in row.iter().enumerate() {
            let number = cell.as_f64()?;
            if !number.is_finite() {
                return None;
            }
            out[row_index * 3 + col_index] = number;
        }
    }
    Some(out)
}

fn position(value: &Value, half: f64, vertical: bool) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return Some(number);
    }
    let text = value.as_str()?;
    anchor(text, half, vertical)
}

fn anchor(expr: &str, half: f64, vertical: bool) -> Option<f64> {
    let expr = expr.trim_start();
    let mut chars = expr.chars();
    let mark = chars.next()?;
    let base = match mark {
        't' if vertical => half,
        'b' if vertical => -half,
        'l' if !vertical => -half,
        'r' if !vertical => half,
        'c' => 0.0,
        _ => return None,
    };
    let rest = chars.as_str().trim_start();
    if rest.is_empty() {
        return Some(base);
    }
    let (sign, number) = if let Some(rest) = rest.strip_prefix('+') {
        (1.0, rest.trim_start())
    } else if let Some(rest) = rest.strip_prefix('-') {
        (-1.0, rest.trim_start())
    } else {
        return None;
    };
    let offset: f64 = number.parse().ok()?;
    Some(base + sign * offset)
}

pub fn write_default_config() -> std::io::Result<PathBuf> {
    let path = config_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set")
    })?;
    if path.exists() {
        eprintln!("tobiifreed: {} already exists", path.display());
        return Ok(path);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(
        &path,
        "{\n  \"display_area\": {\n    \"w_mm\": 800,\n    \"h_mm\": 340,\n    \"z_mm\": 0,\n    \"tilt\": 0,\n    \"cx\": 0,\n    \"cy\": \"b - 10\"\n  }\n}\n",
    )?;
    eprintln!("tobiifreed: created {}", path.display());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_anchor_places_the_panel_above_the_sensor() {
        let area = parse_display_area(
            r#"{"display_area":{"w_mm":290,"h_mm":170,"z_mm":0,"tilt":0,"cx":0,"cy":"b - 10"}}"#,
        )
        .unwrap();
        assert!((area.ox_mm - -145.0).abs() < 1e-9);
        assert!((area.oy_mm - 10.0).abs() < 1e-9);
    }

    #[test]
    fn screen_warp_is_nine_finite_numbers() {
        let values = parse_screen_warp(r#"{"homography":[[1,0,0],[0,1,0],[0,0,1]]}"#).unwrap();
        assert_eq!(values[0], 1.0);
        assert_eq!(values[8], 1.0);
        assert!(parse_screen_warp(r#"{"homography":[[1,0,0],[0,1,0],[0,0,"no"]]}"#).is_none());
    }
}
