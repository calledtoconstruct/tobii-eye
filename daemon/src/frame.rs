//! Outbound USB frames: an 8-byte envelope around a 24-byte TTP header.
//!
//! The envelope length is the TTP size and does not include itself. Host
//! to device direction is 0. The realm key and the hello payload are the
//! bytes captured from the device's own session.

use hmac::{Hmac, Mac};
use md5::{Digest, Md5};

use crate::tlv::{put_point, put_q42, put_u32};

pub const OP_HELLO: u32 = 0x3e8;
pub const OP_SUBSCRIBE: u32 = 0x4c4;
pub const OP_SET_DISPLAY_AREA: u32 = 0x5a0;
pub const OP_GET_DISPLAY_AREA: u32 = 0x596;
pub const OP_CAL_START: u32 = 0x3f2;
pub const OP_CAL_STOP: u32 = 0x3fc;
pub const OP_CAL_CLEAR: u32 = 0x424;
pub const OP_CAL_POINT_ADD2D: u32 = 0x406;
pub const OP_CAL_POINTS_APPLY: u32 = 0x42e;
pub const OP_CAL_RETRIEVE: u32 = 0x44c;
pub const OP_CAL_APPLY: u32 = 0x456;
pub const OP_QUERY_REALM: u32 = 0x640;
pub const OP_OPEN_REALM: u32 = 0x76c;
pub const OP_REALM_RESPONSE: u32 = 0x776;
pub const OP_CLOSE_REALM: u32 = 0x77b;

pub const MAGIC_REQ: u32 = 0x51;
pub const MAGIC_RSP: u32 = 0x52;
pub const MAGIC_NOTIFY: u32 = 0x53;

/// Includes the trailing NUL that the device expects in the HMAC key.
pub const REALM_KEY: &[u8] = b"IS2LJC6GIRBBEK2K\x00";

const HELLO_PAYLOAD: &[u8] = &[
    0x00, 0x00, 0x17, 0x00, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x09, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x01, 0x00, 0x03, 0x00, 0x01, 0x00,
    0x04, 0x00, 0x01, 0x00, 0x05, 0x00, 0x01, 0x00, 0x06, 0x00, 0x01, 0x00, 0x07, 0x00, 0x01,
    0x00, 0x08,
];

pub fn hmac_md5(key: &[u8], message: &[u8]) -> [u8; 16] {
    let mut mac = Hmac::<Md5>::new_from_slice(key).expect("hmac key");
    mac.update(message);
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    out
}

pub fn md5(data: &[u8]) -> [u8; 16] {
    let digest = Md5::digest(data);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}

fn ttp(seq: u32, op: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 24 + payload.len()];
    out[0..4].copy_from_slice(&MAGIC_REQ.to_be_bytes());
    out[4..8].copy_from_slice(&seq.to_be_bytes());
    out[12..16].copy_from_slice(&op.to_be_bytes());
    out[20..24].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    out[24..].copy_from_slice(payload);
    out
}

fn envelope(ttp_frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + ttp_frame.len());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(ttp_frame.len() as u32).to_le_bytes());
    out.extend_from_slice(ttp_frame);
    out
}

fn framed(seq: u32, op: u32, payload: &[u8]) -> Vec<u8> {
    envelope(&ttp(seq, op, payload))
}

fn empty_cmd(seq: u32, op: u32) -> Vec<u8> {
    framed(seq, op, &[0x00, 0x00])
}

pub fn hello(seq: u32) -> Vec<u8> {
    framed(seq, OP_HELLO, HELLO_PAYLOAD)
}

pub fn subscribe(seq: u32, stream_id: u16) -> Vec<u8> {
    let mut pay = [
        0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x17, 0x00, 0x00, 0x00,
        0x04, 0x00, 0x00, 0x00, 0x00,
    ];
    pay[9] = (stream_id >> 8) as u8;
    pay[10] = stream_id as u8;
    framed(seq, OP_SUBSCRIBE, &pay)
}

pub fn query_realm(seq: u32) -> Vec<u8> {
    framed(seq, OP_QUERY_REALM, &[0x00, 0x00])
}

pub fn open_realm(seq: u32, realm_type: u32) -> Vec<u8> {
    let mut pay = Vec::new();
    pay.extend_from_slice(&[0x00, 0x00]);
    put_u32(&mut pay, realm_type);
    pay.push(0x00);
    framed(seq, OP_OPEN_REALM, &pay)
}

pub fn realm_response(seq: u32, realm_id: u32, field_210: u32, digest: &[u8; 16]) -> Vec<u8> {
    let mut pay = Vec::new();
    pay.extend_from_slice(&[0x00, 0x00]);
    put_u32(&mut pay, realm_id);
    put_u32(&mut pay, field_210);
    pay.extend_from_slice(digest);
    framed(seq, OP_REALM_RESPONSE, &pay)
}

pub fn close_realm(seq: u32, realm_id: u32) -> Vec<u8> {
    let mut pay = Vec::new();
    pay.extend_from_slice(&[0x00, 0x00]);
    put_u32(&mut pay, realm_id);
    framed(seq, OP_CLOSE_REALM, &pay)
}

fn display_payload(tl: [f64; 3], tr: [f64; 3], bl: [f64; 3]) -> Vec<u8> {
    let mut pay = vec![0x00, 0x00];
    put_point(&mut pay, tl[0], tl[1], tl[2]);
    put_point(&mut pay, tr[0], tr[1], tr[2]);
    put_point(&mut pay, bl[0], bl[1], bl[2]);
    crate::tlv::put_tag(&mut pay, 0x10100);
    put_u32(&mut pay, 0x3039);
    pay
}

pub fn set_display_area(seq: u32, w: f64, h: f64, ox: f64, oy: f64, z: f64) -> Vec<u8> {
    let tl = [ox, oy + h, z];
    let tr = [ox + w, oy + h, z];
    let bl = [ox, oy, z];
    framed(seq, OP_SET_DISPLAY_AREA, &display_payload(tl, tr, bl))
}

pub fn set_display_area_corners(
    seq: u32,
    tl: [f64; 3],
    tr: [f64; 3],
    bl: [f64; 3],
) -> Vec<u8> {
    framed(seq, OP_SET_DISPLAY_AREA, &display_payload(tl, tr, bl))
}

pub fn get_display_area(seq: u32) -> Vec<u8> {
    framed(seq, OP_GET_DISPLAY_AREA, &[])
}

pub fn cal_start(seq: u32) -> Vec<u8> {
    empty_cmd(seq, OP_CAL_START)
}

pub fn cal_stop(seq: u32) -> Vec<u8> {
    empty_cmd(seq, OP_CAL_STOP)
}

pub fn cal_clear(seq: u32) -> Vec<u8> {
    empty_cmd(seq, OP_CAL_CLEAR)
}

pub fn cal_points_apply(seq: u32) -> Vec<u8> {
    empty_cmd(seq, OP_CAL_POINTS_APPLY)
}

pub fn cal_retrieve(seq: u32) -> Vec<u8> {
    empty_cmd(seq, OP_CAL_RETRIEVE)
}

pub fn cal_point_add2d(seq: u32, x: f64, y: f64, eye_mask: u32) -> Vec<u8> {
    let mut pay = vec![0x00, 0x00];
    put_q42(&mut pay, x);
    put_q42(&mut pay, y);
    put_u32(&mut pay, eye_mask);
    framed(seq, OP_CAL_POINT_ADD2D, &pay)
}

pub fn cal_apply(seq: u32, blob: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + blob.len());
    payload.extend_from_slice(&[0x00, 0x00]);
    payload.extend_from_slice(blob);
    framed(seq, OP_CAL_APPLY, &payload)
}

/// Realm replies are not the same TLV as gaze. Each field is a 4-byte header
/// whose size is a big-endian u16 at bytes 2..4, then that many body bytes.
pub fn realm_u32_at(data: &[u8], index: usize) -> u32 {
    let mut pos = 2;
    let mut found = 0;
    while pos + 4 <= data.len() {
        let size = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if size == 4 && pos + 4 <= data.len() {
            if found == index {
                return u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
            }
            found += 1;
        }
        if pos + size > data.len() {
            break;
        }
        pos += size;
    }
    0
}

pub fn realm_first_u32(data: &[u8]) -> u32 {
    realm_u32_at(data, 0)
}

pub fn realm_challenge(data: &[u8]) -> Option<&[u8]> {
    let mut pos = 2;
    while pos + 4 <= data.len() {
        let size = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + size > data.len() {
            return None;
        }
        if size > 4 {
            return Some(&data[pos..pos + size]);
        }
        pos += size;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_frame_is_79_bytes() {
        let buf = hello(1);
        assert_eq!(buf.len(), 79);
        assert_eq!(buf[0], 0x00);
        assert_eq!(buf[4], 71);
        assert_eq!(buf[5], 0);
        assert_eq!(&buf[8..12], &[0, 0, 0, 0x51]);
        assert_eq!(&buf[12..16], &[0, 0, 0, 1]);
        assert_eq!(&buf[20..24], &[0, 0, 0x03, 0xe8]);
        assert_eq!(&buf[28..32], &[0, 0, 0, 47]);
        assert_eq!(buf[32], 0x00);
    }

    #[test]
    fn display_area_frame_is_196_bytes() {
        let buf = set_display_area(2, 400.0, 300.0, -200.0, 0.0, 0.0);
        assert_eq!(buf.len(), 196);
        assert_eq!(&buf[20..24], &[0, 0, 0x05, 0xa0]);
    }

    #[test]
    fn subscribe_carries_stream_id() {
        let buf = subscribe(3, 0x500);
        assert_eq!(buf.len(), 52);
        assert_eq!(&buf[22..24], &[0x04, 0xc4]);
        assert_eq!(&buf[41..43], &[0x05, 0x00]);
    }

    #[test]
    fn query_and_open_realm_match_captured_layout() {
        let query = query_realm(5);
        assert_eq!(query.len(), 34);
        assert_eq!(&query[22..24], &[0x06, 0x40]);

        let open = open_realm(5, 1);
        assert_eq!(open.len(), 44);
        assert_eq!(&open[22..24], &[0x07, 0x6c]);
        assert_eq!(open[32], 0x00);
        assert_eq!(open[33], 0x00);
        assert_eq!(open[34], 0x02);
        assert_eq!(open[42], 0x01);
        assert_eq!(open[43], 0x00);
    }

    #[test]
    fn calibration_and_realm_response_lengths() {
        let point = cal_point_add2d(5, 0.5, 0.5, 3);
        assert_eq!(point.len(), 69);
        assert_eq!(&point[22..24], &[0x04, 0x06]);

        let digest = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let realm = realm_response(5, 42, 7, &digest);
        assert_eq!(realm.len(), 68);
        assert_eq!(&realm[22..24], &[0x07, 0x76]);
        assert_eq!(realm[52], 1);
    }

    #[test]
    fn md5_and_hmac_match_the_known_vectors() {
        assert_eq!(
            md5(b""),
            [
                0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec,
                0xf8, 0x42, 0x7e
            ]
        );
        assert_eq!(
            md5(b"abc"),
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28,
                0xe1, 0x7f, 0x72
            ]
        );
        let key = [0x0bu8; 16];
        assert_eq!(
            hmac_md5(&key, b"Hi There"),
            [
                0x92, 0x94, 0x72, 0x7a, 0x36, 0x38, 0xbb, 0x1c, 0x13, 0xf4, 0x8e, 0xf8, 0x15,
                0x8b, 0xfc, 0x9d
            ]
        );
        assert_eq!(
            hmac_md5(b"Jefe", b"what do ya want for nothing?"),
            [
                0x75, 0x0c, 0x78, 0x3e, 0x6a, 0xb0, 0xb5, 0x03, 0xea, 0xa8, 0x6e, 0x31, 0x0a,
                0x5d, 0xb7, 0x38
            ]
        );
    }
}
