//! Reassembly of device-to-host bulk chunks into TTP frames.
//!
//! An inbound envelope starts with `01 00 00 00` and a little-endian length
//! that includes the 8-byte envelope. A large payload arrives as a first
//! chunk plus continuation chunks that each wear their own 8-byte envelope.
//! Those continuation headers are stripped so the accumulator holds one frame.

use crate::frame::MAGIC_NOTIFY;
use crate::gaze::{self, GazeSample};

const ENVELOPE: usize = 8;
const TTP_HEADER: usize = 24;
const ACC_CAP: usize = 1 << 21;

pub const ERR_BAD_DIR: u32 = 1;
pub const ERR_BAD_LEN: u32 = 2;
pub const ERR_OVERFLOW: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub magic: u32,
    pub seq: u32,
    pub op: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum Inbound {
    Frame(Frame),
    Gaze(GazeSample),
    Error(u32),
}

pub struct Parser {
    acc: Vec<u8>,
}

impl Parser {
    pub fn new() -> Self {
        Self { acc: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.acc.clear();
    }

    pub fn len(&self) -> usize {
        self.acc.len()
    }

    pub fn feed(&mut self, mut chunk: &[u8]) -> Vec<Inbound> {
        let mut out = Vec::new();
        if self.acc.len() >= ENVELOPE + TTP_HEADER {
            let plen = u32::from_be_bytes(self.acc[ENVELOPE + 20..ENVELOPE + 24].try_into().unwrap());
            let frame_size = ENVELOPE + TTP_HEADER + plen as usize;
            if self.acc.len() < frame_size
                && chunk.len() >= ENVELOPE
                && chunk[0] == 0x01
                && chunk[1] == 0
                && chunk[2] == 0
                && chunk[3] == 0
            {
                chunk = &chunk[ENVELOPE..];
            }
        }
        if self.acc.len() + chunk.len() > ACC_CAP {
            out.push(Inbound::Error(ERR_OVERFLOW));
            self.acc.clear();
            return out;
        }
        self.acc.extend_from_slice(chunk);

        loop {
            match self.next_frame() {
                Ok(None) => break,
                Ok(Some((size, frame))) => {
                    self.acc.drain(..size);
                    if frame.magic == MAGIC_NOTIFY && frame.op == 0x500 {
                        if let Some(sample) = gaze::decode_gaze(&frame.payload) {
                            out.push(Inbound::Gaze(sample));
                        }
                    }
                    out.push(Inbound::Frame(frame));
                }
                Err(code) => {
                    out.push(Inbound::Error(code));
                    self.acc.clear();
                    break;
                }
            }
        }
        out
    }

    fn next_frame(&self) -> Result<Option<(usize, Frame)>, u32> {
        if self.acc.len() < ENVELOPE {
            return Ok(None);
        }
        if self.acc[0] != 0x01 {
            return Err(ERR_BAD_DIR);
        }
        let env_len = u32::from_le_bytes(self.acc[4..8].try_into().unwrap()) as usize;
        if env_len < ENVELOPE + TTP_HEADER {
            return Err(ERR_BAD_LEN);
        }
        if self.acc.len() < ENVELOPE + TTP_HEADER {
            return Ok(None);
        }
        let ttp = &self.acc[ENVELOPE..];
        let magic = u32::from_be_bytes(ttp[0..4].try_into().unwrap());
        let seq = u32::from_be_bytes(ttp[4..8].try_into().unwrap());
        let op = u32::from_be_bytes(ttp[12..16].try_into().unwrap());
        let plen = u32::from_be_bytes(ttp[20..24].try_into().unwrap()) as usize;
        let frame_size = ENVELOPE + TTP_HEADER + plen;
        if frame_size > ACC_CAP {
            return Err(ERR_BAD_LEN);
        }
        if self.acc.len() < frame_size {
            return Ok(None);
        }
        let payload = ttp[TTP_HEADER..TTP_HEADER + plen].to_vec();
        Ok(Some((
            frame_size,
            Frame {
                magic,
                seq,
                op,
                payload,
            },
        )))
    }
}

pub fn fake_inbound(magic: u32, seq: u32, op: u32, payload: &[u8]) -> Vec<u8> {
    let total = (ENVELOPE + TTP_HEADER + payload.len()) as u32;
    let mut out = vec![0u8; total as usize];
    out[0] = 0x01;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    let ttp = &mut out[ENVELOPE..];
    ttp[0..4].copy_from_slice(&magic.to_be_bytes());
    ttp[4..8].copy_from_slice(&seq.to_be_bytes());
    ttp[12..16].copy_from_slice(&op.to_be_bytes());
    ttp[20..24].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    ttp[TTP_HEADER..].copy_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{MAGIC_NOTIFY, MAGIC_RSP};

    fn frames(events: &[Inbound]) -> Vec<&Frame> {
        events
            .iter()
            .filter_map(|event| match event {
                Inbound::Frame(frame) => Some(frame),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parses_one_complete_frame() {
        let mut parser = Parser::new();
        let bytes = fake_inbound(MAGIC_RSP, 42, 0x3e8, &[0xde, 0xad, 0xbe, 0xef]);
        let events = parser.feed(&bytes);
        let got = frames(&events);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].magic, MAGIC_RSP);
        assert_eq!(got[0].seq, 42);
        assert_eq!(got[0].op, 0x3e8);
        assert_eq!(got[0].payload, [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(parser.len(), 0);
    }

    #[test]
    fn parses_two_concatenated_frames() {
        let mut parser = Parser::new();
        let mut bytes = fake_inbound(MAGIC_RSP, 1, 0x100, &[0x11]);
        bytes.extend(fake_inbound(MAGIC_NOTIFY, 0, 0x500, &[0x22, 0x23]));
        let events = parser.feed(&bytes);
        let got = frames(&events);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].op, 0x500);
        assert_eq!(got[1].payload[0], 0x22);
    }

    #[test]
    fn parses_a_frame_split_across_chunks() {
        let mut parser = Parser::new();
        let bytes = fake_inbound(MAGIC_RSP, 7, 0x200, &[0xa1, 0xa2, 0xa3, 0xa4]);
        let first = parser.feed(&bytes[..20]);
        assert!(frames(&first).is_empty());
        assert_eq!(parser.len(), 20);
        let second = parser.feed(&bytes[20..]);
        let got = frames(&second);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].seq, 7);
        assert_eq!(parser.len(), 0);
    }

    #[test]
    fn rejects_a_bad_direction_byte() {
        let mut parser = Parser::new();
        let events = parser.feed(&[0x02, 0, 0, 0, 0x20, 0, 0, 0]);
        assert!(frames(&events).is_empty());
        assert!(matches!(events.last(), Some(Inbound::Error(ERR_BAD_DIR))));
        assert_eq!(parser.len(), 0);
    }

    #[test]
    fn rejects_an_impossibly_small_length() {
        let mut parser = Parser::new();
        let events = parser.feed(&[0x01, 0, 0, 0, 10, 0, 0, 0]);
        assert!(matches!(events.last(), Some(Inbound::Error(ERR_BAD_LEN))));
    }
}
