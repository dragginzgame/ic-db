//! Database-owned row codec v3: exact little-endian integers, strict 0/1 tags,
//! and u32-length-prefixed UTF-8/binary fields. Schema admission owns field types
//! and order; this module owns bounded payload encoding, not index ordering.
//!
//! Fixed-width payloads and exact array decoding are adapted from IcyDB's owned
//! db/data/persisted_row/codec/scalar at 6d421e3601a3d71e42af23ea5fe6b168cb6ef2ac.
//! Extend that scalar approach to 128 bits; omit its slot envelope/value layer.

use crate::{Error, FieldType, Principal};

pub const ROW_CODEC_VERSION: u32 = 3;
pub const MAX_ROW_BYTES: usize = 4096;
// A durable contract bound, independent of any future supplier API expansion.
pub(crate) const MAX_PRINCIPAL_BYTES: usize = 29;

#[derive(Default)]
pub struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    #[must_use]
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > MAX_ROW_BYTES - self.bytes.len() {
            return Err(Error::Limit);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn nat64(&mut self, value: u64) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat8(&mut self, value: u8) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat16(&mut self, value: u16) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat32(&mut self, value: u32) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int8(&mut self, value: i8) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int16(&mut self, value: i16) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int32(&mut self, value: i32) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn nat128(&mut self, value: u128) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int64(&mut self, value: i64) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn int128(&mut self, value: i128) -> Result<(), Error> {
        self.append(&value.to_le_bytes())
    }

    pub fn boolean(&mut self, value: bool) -> Result<(), Error> {
        self.append(&[u8::from(value)])
    }

    pub fn principal(&mut self, value: &Principal) -> Result<(), Error> {
        if value.as_slice().len() > MAX_PRINCIPAL_BYTES {
            return Err(Error::Limit);
        }
        self.bytes(value.as_slice())
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // Admit the complete framed payload before allocating output. Length is
        // format-defined u32, never a host-sized integer or an adaptive encoding.
        let len = u32::try_from(bytes.len()).map_err(|_| Error::Limit)?;
        let framed = bytes.len().checked_add(4).ok_or(Error::Limit)?;
        if framed > MAX_ROW_BYTES - self.bytes.len() {
            return Err(Error::Limit);
        }
        self.append(&len.to_le_bytes())?;
        self.append(bytes)
    }

    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

pub struct Decoder<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_ROW_BYTES {
            return Err(Error::Corrupt);
        }
        Ok(Self { bytes, position: 0 })
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self.position.checked_add(len).ok_or(Error::Corrupt)?;
        let bytes = self.bytes.get(self.position..end).ok_or(Error::Corrupt)?;
        self.position = end;
        Ok(bytes)
    }

    // IcyDB's decode_fixed uses this exact slice-to-array check. Here the admitted
    // schema supplies the width in a sequential row; finish rejects surplus data.
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| Error::Corrupt)
    }

    pub fn nat64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.fixed()?))
    }
    pub fn nat8(&mut self) -> Result<u8, Error> {
        Ok(u8::from_le_bytes(self.fixed()?))
    }
    pub fn nat16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.fixed()?))
    }
    pub fn nat32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.fixed()?))
    }
    pub fn int8(&mut self) -> Result<i8, Error> {
        Ok(i8::from_le_bytes(self.fixed()?))
    }
    pub fn int16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.fixed()?))
    }
    pub fn int32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.fixed()?))
    }
    pub fn nat128(&mut self) -> Result<u128, Error> {
        Ok(u128::from_le_bytes(self.fixed()?))
    }
    pub fn int64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.fixed()?))
    }
    pub fn int128(&mut self) -> Result<i128, Error> {
        Ok(i128::from_le_bytes(self.fixed()?))
    }
    pub fn boolean(&mut self) -> Result<bool, Error> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Corrupt),
        }
    }

    fn bounded_bytes(&mut self, max: usize) -> Result<&'a [u8], Error> {
        let len = usize::try_from(u32::from_le_bytes(self.fixed()?)).map_err(|_| Error::Corrupt)?;
        if len > max {
            return Err(Error::Corrupt);
        }
        self.take(len)
    }
    pub fn blob(&mut self, max: usize) -> Result<Vec<u8>, Error> {
        Ok(self.blob_ref(max)?.to_vec())
    }
    pub fn principal(&mut self) -> Result<Principal, Error> {
        // IcyDB's owned scalar decoder admits raw identity bytes, not text or Wire.
        Principal::try_from_slice(self.bounded_bytes(MAX_PRINCIPAL_BYTES)?)
            .map_err(|_| Error::Corrupt)
    }
    pub fn text(&mut self, max: usize) -> Result<String, Error> {
        Ok(self.text_ref(max)?.to_owned())
    }
    // Borrow from the supplier's row buffer, as IcyDB's direct scalar views do.
    // Owning decode and inspection share these exact length/UTF-8 checks.
    pub(crate) fn blob_ref(&mut self, max: usize) -> Result<&'a [u8], Error> {
        self.bounded_bytes(max)
    }
    pub(crate) fn text_ref(&mut self, max: usize) -> Result<&'a str, Error> {
        std::str::from_utf8(self.bounded_bytes(max)?).map_err(|_| Error::Corrupt)
    }
    // Skipping avoids materialization, not validation. Reuse the scalar decoder
    // so an unselected invalid tag, UTF-8 payload or length cannot be hidden.
    pub(crate) fn skip(&mut self, value: FieldType) -> Result<(), Error> {
        match value {
            FieldType::Nat8 => {
                self.nat8()?;
            }
            FieldType::Nat16 => {
                self.nat16()?;
            }
            FieldType::Nat32 => {
                self.nat32()?;
            }
            FieldType::Nat64 => {
                self.nat64()?;
            }
            FieldType::Nat128 => {
                self.nat128()?;
            }
            FieldType::Int8 => {
                self.int8()?;
            }
            FieldType::Int16 => {
                self.int16()?;
            }
            FieldType::Int32 => {
                self.int32()?;
            }
            FieldType::Int64 => {
                self.int64()?;
            }
            FieldType::Int128 => {
                self.int128()?;
            }
            FieldType::Bool => {
                self.boolean()?;
            }
            FieldType::Principal => {
                self.principal()?;
            }
            FieldType::Text { max_bytes } => {
                self.text_ref(max_bytes as usize)?;
            }
            FieldType::Blob { max_bytes } => {
                self.blob_ref(max_bytes as usize)?;
            }
        }
        Ok(())
    }
    pub fn finish(self) -> Result<(), Error> {
        if self.position != self.bytes.len() {
            return Err(Error::Corrupt);
        }
        Ok(())
    }
}
