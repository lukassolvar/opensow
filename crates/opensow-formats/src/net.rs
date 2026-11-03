//! Multiplayer packets, decoded from `State of War.exe` (message classes `CNetMessage` `0x4e30` bytes and `CNetMessage2`
//! `0x3f4` bytes, sent over DirectPlay 4A; docs/network.md).
//!
//! Both are bit streams: bit `p` is bit `p & 7` of byte `p >> 3`. Numeric fields written by `0x4528f0(value, n)` and read
//! by `0x451200(n)` are least significant bit first; the input package records (`0x451540`, `0x451920`) write their values
//! most significant bit first.

use crate::{Error, Result};

/// Reads bits from a byte slice.
#[derive(Debug, Clone)]
pub struct Bits<'a> {
    data: &'a [u8],
    pub pos: usize,
    pub len: usize,
}

impl<'a> Bits<'a> {
    pub fn new(data: &'a [u8], len: usize) -> Bits<'a> {
        Bits { data, pos: 0, len: len.min(data.len() * 8) }
    }

    pub fn remaining(&self) -> usize {
        self.len - self.pos
    }

    pub fn bit(&mut self) -> Result<bool> {
        if self.pos >= self.len {
            return Err(Error::Truncated { what: "net bit", offset: self.pos });
        }
        let b = self.data[self.pos >> 3] >> (self.pos & 7) & 1;
        self.pos += 1;
        Ok(b == 1)
    }

    /// `0x451200(n)`: the first bit read is the least significant.
    pub fn lsb(&mut self, n: u32) -> Result<u32> {
        let mut v = 0;
        for i in 0..n {
            v |= (self.bit()? as u32) << i;
        }
        Ok(v)
    }

    /// Input-package fields: the first bit read is the most significant.
    pub fn msb(&mut self, n: u32) -> Result<u32> {
        let mut v = 0;
        for _ in 0..n {
            v = v << 1 | self.bit()? as u32;
        }
        Ok(v)
    }
}

/// Writes bits into a growing byte vector.
#[derive(Debug, Clone, Default)]
pub struct BitWriter {
    pub data: Vec<u8>,
    pub len: usize,
}

impl BitWriter {
    pub fn bit(&mut self, b: bool) {
        if self.len >> 3 >= self.data.len() {
            self.data.push(0);
        }
        if b {
            self.data[self.len >> 3] |= 1 << (self.len & 7);
        }
        self.len += 1;
    }

    /// `0x4528f0(value, n)`.
    pub fn lsb(&mut self, value: u32, n: u32) {
        for i in 0..n {
            self.bit(value >> i & 1 == 1);
        }
    }

    pub fn msb(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            self.bit(value >> i & 1 == 1);
        }
    }
}

/// A keyboard event of an input package: `01`, a type bit (1 = key down, 0 = key up), the 8-bit scan code (MSB first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: u8,
    pub down: bool,
}

/// The mouse record that ends an input package: `10`, two button bits, then x and y as 16-bit words (MSB first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mouse {
    pub a: bool,
    pub b: bool,
    pub x: i16,
    pub y: i16,
}

/// One player's input for a tick (`CNetMessage2`, `0x3f4` bytes per player at `0x70dfb8`): 16 reserved bits (zero when the
/// package is built; the exe starts reading at bit 16), any number of key records, one mouse record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPackage {
    pub head: u16,
    pub keys: Vec<Key>,
    pub mouse: Option<Mouse>,
}

impl InputPackage {
    pub fn parse(data: &[u8], len_bits: usize) -> Result<InputPackage> {
        let mut r = Bits::new(data, len_bits);
        let head = r.lsb(16)? as u16;
        let mut keys = Vec::new();
        loop {
            let mut peek = r.clone();
            if r.remaining() < 11 || peek.bit()? || !peek.bit()? {
                break;
            }
            r.pos += 2;
            let down = r.bit()?;
            keys.push(Key { code: r.msb(8)? as u8, down });
        }
        let mut mouse = None;
        if r.remaining() >= 36 {
            let mut peek = r.clone();
            if peek.bit()? && !peek.bit()? {
                r.pos += 2;
                let (a, b) = (r.bit()?, r.bit()?);
                mouse = Some(Mouse { a, b, x: r.msb(16)? as u16 as i16, y: r.msb(16)? as u16 as i16 });
            }
        }
        Ok(InputPackage { head, keys, mouse })
    }

    pub fn write(&self) -> BitWriter {
        let mut w = BitWriter::default();
        w.lsb(self.head as u32, 16);
        for k in &self.keys {
            w.bit(false);
            w.bit(true);
            w.bit(k.down);
            w.msb(k.code as u32, 8);
        }
        if let Some(m) = self.mouse {
            w.bit(true);
            w.bit(false);
            w.bit(m.a);
            w.bit(m.b);
            w.msb(m.x as u16 as u32, 16);
            w.msb(m.y as u16 as u32, 16);
        }
        w
    }
}

/// Field layout of the transport header (`CNetMessage`, buffer at `+0xc`; selector reader `0x4510d0`, writer `0x4527c0`):
/// `(first bit, width)` for selectors 0..=7 (selector 7 is the 3-bit field at bit 4). The payload starts at bit [`PAYLOAD_BIT`] (header type 0) or bit 7/8 for the
/// shorter headers the receiver `0x451390` selects.
pub const HEADER_FIELDS: [(u32, u32); 8] = [(0, 4), (7, 18), (0x19, 6), (0x1f, 12), (0x2b, 9), (0x34, 9), (0x3d, 6), (4, 3)];
pub const PAYLOAD_BIT: u32 = 0x43;

/// The decoded header fields of a transport message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub fields: [u32; 8],
}

impl Header {
    pub fn parse(data: &[u8], len_bits: usize) -> Result<Header> {
        let mut fields = [0; 8];
        for (i, &(at, n)) in HEADER_FIELDS.iter().enumerate() {
            let mut r = Bits::new(data, len_bits);
            r.pos = at as usize;
            fields[i] = r.lsb(n)?;
        }
        Ok(Header { fields })
    }

    pub fn write(&self) -> BitWriter {
        let mut order: Vec<usize> = (0..8).collect();
        order.sort_by_key(|&i| HEADER_FIELDS[i].0);
        let mut w = BitWriter::default();
        for i in order {
            w.lsb(self.fields[i], HEADER_FIELDS[i].1);
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_order() {
        let mut w = BitWriter::default();
        w.lsb(0b101, 3);
        w.msb(0b101, 3);
        assert_eq!(w.data, vec![0b00101101]);
        let mut r = Bits::new(&w.data, w.len);
        assert_eq!(r.lsb(3).unwrap(), 5);
        assert_eq!(r.msb(3).unwrap(), 5);
        assert!(r.bit().is_err());
    }

    #[test]
    fn input_package_round_trip() {
        let p = InputPackage {
            head: 0,
            keys: vec![Key { code: 0x41, down: true }, Key { code: 0xff, down: false }],
            mouse: Some(Mouse { a: true, b: false, x: -5, y: 639 }),
        };
        let w = p.write();
        assert_eq!(w.len, 16 + 22 + 36);
        assert_eq!(InputPackage::parse(&w.data, w.len).unwrap(), p);
        let none = InputPackage { head: 0x1234, keys: vec![], mouse: None };
        let w = none.write();
        assert_eq!(InputPackage::parse(&w.data, w.len).unwrap(), none);
    }

    #[test]
    fn header_fields() {
        let h = Header { fields: [3, 200_000, 41, 4000, 300, 511, 63, 5] };
        let w = h.write();
        assert_eq!(w.len, 0x43);
        assert_eq!(Header::parse(&w.data, w.len).unwrap(), h);
    }
}
