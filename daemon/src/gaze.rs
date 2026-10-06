//! Fixed 392-byte gaze sample. The layout matches the socket payload the
//! Python client unpacks as `<4Iq46d`.

use crate::tlv::Reader;

pub const GAZE_SIZE: usize = 392;

const BIT_TIMESTAMP: u32 = 1 << 0;
const BIT_FRAME_COUNTER: u32 = 1 << 1;
const BIT_VALIDITY_L: u32 = 1 << 2;
const BIT_VALIDITY_R: u32 = 1 << 3;
const BIT_PUPIL_L: u32 = 1 << 4;
const BIT_PUPIL_R: u32 = 1 << 5;
const BIT_GAZE_2D: u32 = 1 << 6;
const BIT_GAZE_2D_L: u32 = 1 << 7;
const BIT_GAZE_2D_R: u32 = 1 << 8;
const BIT_EYE_ORIGIN_L: u32 = 1 << 9;
const BIT_EYE_ORIGIN_R: u32 = 1 << 10;
const BIT_GAZE_DIR_L: u32 = 1 << 11;
const BIT_GAZE_DIR_R: u32 = 1 << 12;
const BIT_GAZE_3D_L: u32 = 1 << 13;
const BIT_GAZE_3D_R: u32 = 1 << 14;
const BIT_EYE_ORIGIN_L_DISP: u32 = 1 << 15;
const BIT_EYE_ORIGIN_R_DISP: u32 = 1 << 16;
const BIT_TRACKBOX_L_DISP: u32 = 1 << 17;
const BIT_TRACKBOX_R_DISP: u32 = 1 << 18;
const BIT_EYE_ORIGIN_RAW_L: u32 = 1 << 19;
const BIT_EYE_ORIGIN_RAW_R: u32 = 1 << 20;
const BIT_GAZE_2D_UNFILTERED: u32 = 1 << 21;

const KIND_S64: u8 = 0;
const KIND_U32: u8 = 1;
const KIND_POINT2D: u8 = 2;
const KIND_POINT3D: u8 = 3;
const KIND_FIXED: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GazeSample {
    pub present_mask: u32,
    pub frame_counter: u32,
    pub validity_l: u32,
    pub validity_r: u32,
    pub timestamp_us: i64,
    pub pupil_l_mm: f64,
    pub pupil_r_mm: f64,
    pub gaze_xy: [f64; 2],
    pub gaze_xy_l: [f64; 2],
    pub gaze_xy_r: [f64; 2],
    pub eye_origin_l: [f64; 3],
    pub eye_origin_r: [f64; 3],
    pub trackbox_l: [f64; 3],
    pub trackbox_r: [f64; 3],
    pub gaze_3d_l: [f64; 3],
    pub gaze_3d_r: [f64; 3],
    pub eye_origin_l_display: [f64; 3],
    pub eye_origin_r_display: [f64; 3],
    pub trackbox_l_display: [f64; 3],
    pub trackbox_r_display: [f64; 3],
    pub eye_origin_raw_l: [f64; 3],
    pub eye_origin_raw_r: [f64; 3],
    pub gaze_xy_unfiltered: [f64; 2],
}

impl Default for GazeSample {
    fn default() -> Self {
        Self {
            present_mask: 0,
            frame_counter: 0,
            validity_l: 0,
            validity_r: 0,
            timestamp_us: 0,
            pupil_l_mm: 0.0,
            pupil_r_mm: 0.0,
            gaze_xy: [0.0; 2],
            gaze_xy_l: [0.0; 2],
            gaze_xy_r: [0.0; 2],
            eye_origin_l: [0.0; 3],
            eye_origin_r: [0.0; 3],
            trackbox_l: [0.0; 3],
            trackbox_r: [0.0; 3],
            gaze_3d_l: [0.0; 3],
            gaze_3d_r: [0.0; 3],
            eye_origin_l_display: [0.0; 3],
            eye_origin_r_display: [0.0; 3],
            trackbox_l_display: [0.0; 3],
            trackbox_r_display: [0.0; 3],
            eye_origin_raw_l: [0.0; 3],
            eye_origin_raw_r: [0.0; 3],
            gaze_xy_unfiltered: [0.0; 2],
        }
    }
}

impl GazeSample {
    pub fn to_bytes(self) -> [u8; GAZE_SIZE] {
        let mut out = [0u8; GAZE_SIZE];
        out[0..4].copy_from_slice(&self.present_mask.to_le_bytes());
        out[4..8].copy_from_slice(&self.frame_counter.to_le_bytes());
        out[8..12].copy_from_slice(&self.validity_l.to_le_bytes());
        out[12..16].copy_from_slice(&self.validity_r.to_le_bytes());
        out[16..24].copy_from_slice(&self.timestamp_us.to_le_bytes());
        let mut cursor = 24;
        for value in self.floats() {
            out[cursor..cursor + 8].copy_from_slice(&value.to_le_bytes());
            cursor += 8;
        }
        debug_assert_eq!(cursor, GAZE_SIZE);
        out
    }

    fn floats(self) -> [f64; 46] {
        let mut out = [0.0; 46];
        let mut cursor = 0;
        let mut put = |values: &[f64]| {
            for value in values {
                out[cursor] = *value;
                cursor += 1;
            }
        };
        put(&[self.pupil_l_mm, self.pupil_r_mm]);
        put(&self.gaze_xy);
        put(&self.gaze_xy_l);
        put(&self.gaze_xy_r);
        put(&self.eye_origin_l);
        put(&self.eye_origin_r);
        put(&self.trackbox_l);
        put(&self.trackbox_r);
        put(&self.gaze_3d_l);
        put(&self.gaze_3d_r);
        put(&self.eye_origin_l_display);
        put(&self.eye_origin_r_display);
        put(&self.trackbox_l_display);
        put(&self.trackbox_r_display);
        put(&self.eye_origin_raw_l);
        put(&self.eye_origin_raw_r);
        put(&self.gaze_xy_unfiltered);
        out
    }
}

fn column_kind(col: u32) -> Option<u8> {
    Some(match col {
        0x01 => KIND_S64,
        0x02 | 0x03 | 0x04 | 0x08 | 0x09 | 0x0a | 0x17 | 0x18 | 0x22 | 0x24 | 0x25 | 0x27 => {
            KIND_POINT3D
        }
        0x05 | 0x0b | 0x1c | 0x20 | 0x19 | 0x1a => KIND_POINT2D,
        0x06 | 0x0c | 0x29 | 0x2b => KIND_FIXED,
        0x07 | 0x0d | 0x0e | 0x11 | 0x14 | 0x15 | 0x16 | 0x1b | 0x1d | 0x1e | 0x1f | 0x21
        | 0x23 | 0x26 | 0x28 | 0x2a | 0x2c => KIND_U32,
        _ => return None,
    })
}

fn skip_kind(reader: &mut Reader, kind: u8) -> Option<()> {
    match kind {
        KIND_S64 => {
            reader.read_s64()?;
        }
        KIND_U32 => {
            reader.read_u32()?;
        }
        KIND_FIXED => {
            reader.read_fixed16x16()?;
        }
        KIND_POINT2D => {
            reader.read_point2d()?;
        }
        KIND_POINT3D => {
            reader.read_point3d()?;
        }
        _ => return None,
    }
    Some(())
}

/// Decode a 0x500 notification payload. A short or unknown column stops the
/// walk and keeps the fields already filled.
pub fn decode_gaze(payload: &[u8]) -> Option<GazeSample> {
    if payload.len() < 2 {
        return None;
    }
    let mut sample = GazeSample::default();
    let mut reader = Reader::new(payload);
    reader.set_pos(2);
    let n_cols = reader.read_xds_row()?;
    for _ in 0..n_cols {
        if reader.remaining() == 0 {
            break;
        }
        let Some(col) = reader.read_xds_column() else {
            break;
        };
        let stored = match col {
            0x01 => reader.read_s64().map(|v| {
                sample.timestamp_us = v;
                sample.present_mask |= BIT_TIMESTAMP;
            }),
            0x02 => reader.read_point3d().map(|v| {
                sample.eye_origin_l = v;
                sample.present_mask |= BIT_EYE_ORIGIN_L;
            }),
            0x03 => reader.read_point3d().map(|v| {
                sample.trackbox_l = v;
                sample.present_mask |= BIT_GAZE_DIR_L;
            }),
            0x04 => reader.read_point3d().map(|v| {
                sample.gaze_3d_l = v;
                sample.present_mask |= BIT_GAZE_3D_L;
            }),
            0x05 => reader.read_point2d().map(|v| {
                sample.gaze_xy_l = v;
                sample.present_mask |= BIT_GAZE_2D_L;
            }),
            0x06 => reader.read_fixed16x16().map(|v| {
                sample.pupil_l_mm = v;
                sample.present_mask |= BIT_PUPIL_L;
            }),
            0x07 => reader.read_u32().map(|v| {
                sample.validity_l = v;
                sample.present_mask |= BIT_VALIDITY_L;
            }),
            0x08 => reader.read_point3d().map(|v| {
                sample.eye_origin_r = v;
                sample.present_mask |= BIT_EYE_ORIGIN_R;
            }),
            0x09 => reader.read_point3d().map(|v| {
                sample.trackbox_r = v;
                sample.present_mask |= BIT_GAZE_DIR_R;
            }),
            0x0a => reader.read_point3d().map(|v| {
                sample.gaze_3d_r = v;
                sample.present_mask |= BIT_GAZE_3D_R;
            }),
            0x0b => reader.read_point2d().map(|v| {
                sample.gaze_xy_r = v;
                sample.present_mask |= BIT_GAZE_2D_R;
            }),
            0x0c => reader.read_fixed16x16().map(|v| {
                sample.pupil_r_mm = v;
                sample.present_mask |= BIT_PUPIL_R;
            }),
            0x0d => reader.read_u32().map(|v| {
                sample.validity_r = v;
                sample.present_mask |= BIT_VALIDITY_R;
            }),
            0x14 => reader.read_u32().map(|v| {
                sample.frame_counter = v;
                sample.present_mask |= BIT_FRAME_COUNTER;
            }),
            0x22 => reader.read_point3d().map(|v| {
                sample.eye_origin_l_display = v;
                sample.present_mask |= BIT_EYE_ORIGIN_L_DISP;
            }),
            0x24 => reader.read_point3d().map(|v| {
                sample.eye_origin_r_display = v;
                sample.present_mask |= BIT_EYE_ORIGIN_R_DISP;
            }),
            0x25 => reader.read_point3d().map(|v| {
                sample.trackbox_l_display = v;
                sample.present_mask |= BIT_TRACKBOX_L_DISP;
            }),
            0x27 => reader.read_point3d().map(|v| {
                sample.trackbox_r_display = v;
                sample.present_mask |= BIT_TRACKBOX_R_DISP;
            }),
            0x17 => reader.read_point3d().map(|v| {
                sample.eye_origin_raw_l = v;
                sample.present_mask |= BIT_EYE_ORIGIN_RAW_L;
            }),
            0x18 => reader.read_point3d().map(|v| {
                sample.eye_origin_raw_r = v;
                sample.present_mask |= BIT_EYE_ORIGIN_RAW_R;
            }),
            0x20 => reader.read_point2d().map(|v| {
                sample.gaze_xy_unfiltered = v;
                sample.present_mask |= BIT_GAZE_2D_UNFILTERED;
            }),
            0x1c => reader.read_point2d().map(|v| {
                sample.gaze_xy = v;
                sample.present_mask |= BIT_GAZE_2D;
            }),
            _ => match column_kind(col) {
                Some(kind) => skip_kind(&mut reader, kind).map(|_| ()),
                None => break,
            },
        };
        if stored.is_none() {
            break;
        }
    }
    Some(sample)
}

/// Display-area reply: a 2-byte prefix, then three point3d values.
pub fn decode_display_area(payload: &[u8]) -> Option<[[f64; 3]; 3]> {
    if payload.len() < 2 {
        return None;
    }
    let mut reader = Reader::new(payload);
    reader.set_pos(2);
    Some([
        reader.read_point3d()?,
        reader.read_point3d()?,
        reader.read_point3d()?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_sample_is_392_bytes_with_gaze_at_the_python_offset() {
        let mut sample = GazeSample::default();
        sample.present_mask = BIT_GAZE_2D;
        sample.gaze_xy = [0.25, 0.75];
        let bytes = sample.to_bytes();
        assert_eq!(bytes.len(), 392);
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), BIT_GAZE_2D);
        let x = f64::from_le_bytes(bytes[40..48].try_into().unwrap());
        let y = f64::from_le_bytes(bytes[48..56].try_into().unwrap());
        assert_eq!(x, 0.25);
        assert_eq!(y, 0.75);
    }
}
