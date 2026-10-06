//! Daemon socket framing: `[u8 type][u32 le length][payload]`.

use crate::gaze::GazeSample;

pub const HEADER_SIZE: usize = 5;

pub const CMD_SUBSCRIBE: u8 = 0x01;
pub const CMD_GET_DISPLAY_AREA: u8 = 0x02;
pub const CMD_SET_DISPLAY_AREA: u8 = 0x03;
pub const CMD_SET_DISPLAY_AREA_CORNERS: u8 = 0x04;
pub const CMD_START_CALIBRATION: u8 = 0x20;
pub const CMD_ADD_CALIBRATION_POINT: u8 = 0x21;
pub const CMD_FINISH_CALIBRATION: u8 = 0x22;
pub const CMD_CAL_APPLY: u8 = 0x23;
pub const CMD_DISCONNECT: u8 = 0xff;

pub const SRV_GAZE: u8 = 0x01;
pub const SRV_RESPONSE: u8 = 0x02;
pub const SRV_DISPLAY_AREA: u8 = 0x03;
pub const SRV_SCREEN_WARP: u8 = 0x04;
pub const SRV_ERR: u8 = 0xff;

pub const STREAM_GAZE: u32 = 0x500;

pub const ERR_FAILED: u32 = 0x01;
pub const ERR_TOO_LARGE: u32 = 0x02;

/// WebSocket responses have to fit the daemon's fixed frame buffer.
pub const WS_MAX_RESPONSE_PAYLOAD: usize = 8192 - HEADER_SIZE - 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub kind: u8,
    pub payload: Vec<u8>,
}

pub fn encode(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SIZE + payload.len());
    out.push(kind);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn encode_gaze(sample: GazeSample) -> Vec<u8> {
    let bytes = sample.to_bytes();
    encode(SRV_GAZE, &bytes)
}

pub fn encode_response(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(1 + payload.len());
    body.push(cmd);
    body.extend_from_slice(payload);
    encode(SRV_RESPONSE, &body)
}

pub fn encode_error(cmd: u8, code: u32) -> Vec<u8> {
    let mut body = vec![cmd];
    body.extend_from_slice(&code.to_le_bytes());
    encode(SRV_ERR, &body)
}

pub fn encode_screen_warp(values: Option<[f64; 9]>) -> Vec<u8> {
    match values {
        Some(values) => {
            let mut payload = [0u8; 72];
            for (index, value) in values.iter().enumerate() {
                payload[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
            }
            encode(SRV_SCREEN_WARP, &payload)
        }
        None => encode(SRV_SCREEN_WARP, &[]),
    }
}

/// Pull one complete message off the front of `buf`. `None` means it still
/// needs bytes. A payload above 8 MiB is rejected.
pub fn pop_message(buf: &mut Vec<u8>) -> Result<Option<Message>, ()> {
    if buf.len() < HEADER_SIZE {
        return Ok(None);
    }
    let kind = buf[0];
    let len = u32::from_le_bytes(buf[1..5].try_into().unwrap()) as usize;
    if len > 8 * 1024 * 1024 {
        return Err(());
    }
    let end = HEADER_SIZE + len;
    if buf.len() < end {
        return Ok(None);
    }
    let payload = buf[HEADER_SIZE..end].to_vec();
    buf.drain(..end);
    Ok(Some(Message { kind, payload }))
}

pub fn read_f64s<const N: usize>(payload: &[u8]) -> Option<[f64; N]> {
    if payload.len() < N * 8 {
        return None;
    }
    let mut out = [0.0; N];
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 8;
        *slot = f64::from_le_bytes(payload[start..start + 8].try_into().unwrap());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_message_matches_the_python_client() {
        let msg = encode(CMD_SUBSCRIBE, &STREAM_GAZE.to_le_bytes());
        assert_eq!(msg, [0x01, 4, 0, 0, 0, 0x00, 0x05, 0, 0]);
    }

    #[test]
    fn gaze_frame_carries_a_392_byte_sample() {
        let frame = encode_gaze(GazeSample::default());
        assert_eq!(frame.len(), HEADER_SIZE + crate::gaze::GAZE_SIZE);
        assert_eq!(frame[0], SRV_GAZE);
        assert_eq!(
            u32::from_le_bytes(frame[1..5].try_into().unwrap()) as usize,
            crate::gaze::GAZE_SIZE
        );
    }

    #[test]
    fn pop_waits_for_a_complete_message_then_returns_it() {
        let mut buf = encode(CMD_GET_DISPLAY_AREA, &[]).to_vec();
        buf.truncate(3);
        assert_eq!(pop_message(&mut buf).unwrap(), None);
        buf.extend_from_slice(&encode(CMD_GET_DISPLAY_AREA, &[])[3..]);
        let msg = pop_message(&mut buf).unwrap().unwrap();
        assert_eq!(msg.kind, CMD_GET_DISPLAY_AREA);
        assert!(msg.payload.is_empty());
        assert!(buf.is_empty());
    }

    #[test]
    fn error_frame_names_the_command() {
        let frame = encode_error(CMD_START_CALIBRATION, ERR_FAILED);
        assert_eq!(frame[0], SRV_ERR);
        assert_eq!(frame[HEADER_SIZE], CMD_START_CALIBRATION);
        assert_eq!(
            u32::from_le_bytes(frame[HEADER_SIZE + 1..HEADER_SIZE + 5].try_into().unwrap()),
            ERR_FAILED
        );
    }
}
