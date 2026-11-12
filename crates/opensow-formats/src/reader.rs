//! Bounds-checked little-endian reading. Every parser in this crate reads through [`Reader`] instead of indexing
//! byte slices, so a damaged or truncated file gives an [`Error`] and never a panic.

use crate::{Error, Result};

/// Little-endian cursor over a byte slice with bounds-checked reads.
#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    /// A reader over `data` that starts at byte `pos`.
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Reader { data, pos }
    }

    /// The whole slice the reader was made over (not only what is left).
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// The offset of the next byte to be read.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// How many bytes are left to read.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Reads the next `n` bytes. `what` names the thing being read; it goes into the error when the data is too short.
    pub fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or(Error::Truncated { what, offset: self.pos })?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// Reads one byte.
    pub fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.bytes(1, what)?[0])
    }

    /// Reads a little-endian `u16`.
    pub fn u16(&mut self, what: &'static str) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2, what)?.try_into().unwrap()))
    }

    /// Reads a little-endian `i16`.
    pub fn i16(&mut self, what: &'static str) -> Result<i16> {
        Ok(self.u16(what)? as i16)
    }

    /// Reads a little-endian `u32`.
    pub fn u32(&mut self, what: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4, what)?.try_into().unwrap()))
    }

    /// Reads a little-endian `i32`.
    pub fn i32(&mut self, what: &'static str) -> Result<i32> {
        Ok(self.u32(what)? as i32)
    }

    /// NUL-terminated string; the terminator is consumed but not returned.
    pub fn cstr(&mut self, what: &'static str) -> Result<&'a [u8]> {
        let rest = &self.data[self.pos.min(self.data.len())..];
        let len = rest.iter().position(|&b| b == 0).ok_or(Error::Truncated { what, offset: self.pos })?;
        self.pos += len + 1;
        Ok(&rest[..len])
    }
}
