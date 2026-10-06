//! TLV fields on the Eye Tracker 5 bulk protocol.
//!
//! A field is one type byte, a 4-byte big-endian size, then that many body
//! bytes. A prolog (type 5, size 4) carries a tag and then its children.

#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn read_u8(&mut self) -> Option<u8> {
        let v = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }

    fn read_u32_be(&mut self) -> Option<u32> {
        let bytes: [u8; 4] = self.buf.get(self.pos..self.pos + 4)?.try_into().ok()?;
        self.pos += 4;
        Some(u32::from_be_bytes(bytes))
    }

    fn read_i32_be(&mut self) -> Option<i32> {
        let bytes: [u8; 4] = self.buf.get(self.pos..self.pos + 4)?.try_into().ok()?;
        self.pos += 4;
        Some(i32::from_be_bytes(bytes))
    }

    fn read_i64_be(&mut self) -> Option<i64> {
        let bytes: [u8; 8] = self.buf.get(self.pos..self.pos + 8)?.try_into().ok()?;
        self.pos += 8;
        Some(i64::from_be_bytes(bytes))
    }

    fn header(&mut self, expect_type: u8, expect_size: u32) -> Option<()> {
        if self.read_u8()? != expect_type {
            return None;
        }
        if self.read_u32_be()? != expect_size {
            return None;
        }
        Some(())
    }

    pub fn read_prolog_tag(&mut self) -> Option<u32> {
        self.header(5, 4)?;
        self.read_u32_be()
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        self.header(2, 4)?;
        self.read_u32_be()
    }

    pub fn read_s64(&mut self) -> Option<i64> {
        self.header(6, 8)?;
        self.read_i64_be()
    }

    pub fn read_fixed16x16(&mut self) -> Option<f64> {
        self.header(3, 4)?;
        Some(self.read_i32_be()? as f64 / 65536.0)
    }

    pub fn read_q42(&mut self) -> Option<f64> {
        self.header(4, 8)?;
        Some(self.read_i64_be()? as f64 / Q42_SCALE)
    }

    pub fn read_xds_row(&mut self) -> Option<u32> {
        let tag = self.read_prolog_tag()?;
        if tag & 0xffff != 0x0bb8 {
            return None;
        }
        Some((tag >> 16) & 0xfff)
    }

    pub fn read_xds_column(&mut self) -> Option<u32> {
        if self.read_prolog_tag()? != 0x020bb9 {
            return None;
        }
        self.read_u32()
    }

    pub fn read_point3d(&mut self) -> Option<[f64; 3]> {
        if self.read_prolog_tag()? != 0x031f41 {
            return None;
        }
        Some([self.read_q42()?, self.read_q42()?, self.read_q42()?])
    }

    pub fn read_point2d(&mut self) -> Option<[f64; 2]> {
        if self.read_prolog_tag()? != 0x021f40 {
            return None;
        }
        Some([self.read_q42()?, self.read_q42()?])
    }
}

const Q42_SCALE: f64 = 4398046511104.0;

pub fn put_tag(out: &mut Vec<u8>, tag: u32) {
    out.push(5);
    out.extend_from_slice(&4u32.to_be_bytes());
    out.extend_from_slice(&tag.to_be_bytes());
}

pub fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.push(2);
    out.extend_from_slice(&4u32.to_be_bytes());
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn put_q42(out: &mut Vec<u8>, value: f64) {
    out.push(4);
    out.extend_from_slice(&8u32.to_be_bytes());
    let scaled = (value * Q42_SCALE).round() as i64;
    out.extend_from_slice(&scaled.to_be_bytes());
}

pub fn put_point(out: &mut Vec<u8>, x: f64, y: f64, z: f64) {
    put_tag(out, 0x31f41);
    put_q42(out, x);
    put_q42(out, y);
    put_q42(out, z);
}

pub fn q42_encode(mm: f64) -> i64 {
    (mm * Q42_SCALE).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q42_matches_the_captured_scale() {
        assert_eq!(q42_encode(200.0), 879_609_302_220_800);
        assert_eq!(q42_encode(0.0), 0);
        assert_eq!(q42_encode(-200.0), -879_609_302_220_800);
    }
}
