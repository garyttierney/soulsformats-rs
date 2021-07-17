use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};

use byteorder::{ByteOrder, ReadBytesExt};
use encoding_rs::{SHIFT_JIS, UTF_16BE, UTF_16LE};

pub fn invalid_data(message: &str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

fn expect_num<R: Read + ?Sized, T: Sized + PartialEq>(
    reader: &mut R,
    read_fn: fn(&mut R) -> Result<T, Error>,
    value: T,
) -> Result<(), Error> {
    match read_fn(reader)? {
        v if v == value => Ok(()),
        _ => Err(Error::new(
            ErrorKind::InvalidData,
            "Unexpected data".to_string(),
        )),
    }
}

pub trait SeekableReadExt: Read + Seek {
    #[inline]
    fn at<T: Sized, F: FnOnce(&mut Self) -> Result<T, Error>>(
        &mut self,
        position: u64,
        consumer: F,
    ) -> Result<T, Error> {
        let current_pos = self.stream_position()?;
        self.seek(SeekFrom::Start(position))?;

        let result = consumer(self);
        self.seek(SeekFrom::Start(current_pos))?;

        result
    }
}

pub trait ReadExt: Read {
    #[inline]
    fn expect_u8(&mut self, value: u8) -> Result<(), Error> {
        expect_num(self, |r| r.read_u8(), value)
    }

    #[inline]
    fn expect_i32<O: ByteOrder>(&mut self, value: i32) -> Result<(), Error> {
        expect_num(self, |r| r.read_i32::<O>(), value)
    }

    #[inline]
    fn expect(&mut self, bytes: &[u8]) -> Result<bool, Error> {
        for (pos, byte) in bytes.iter().enumerate() {
            if self.read_u8()? != *byte {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("Unexpected data at position {}", pos),
                ));
            }
        }

        Ok(true)
    }

    #[inline]
    fn read_bool(&mut self) -> Result<bool, Error> {
        self.read_u8().map(|v| v == 1)
    }

    #[inline]
    fn read_cstr(&mut self) -> Result<String, Error> {
        let mut data = Vec::new();
        loop {
            match self.read_u8()? {
                0 => break,
                char => data.push(char),
            }
        }

        let (decoded, ..) = SHIFT_JIS.decode(&data[..]);
        Ok(decoded.to_string())
    }

    #[inline]
    fn read_utf16(&mut self, is_big_endian: bool) -> Result<String, Error> {
        let mut data = Vec::new();
        loop {
            match (self.read_u8()?, self.read_u8()?) {
                (0, 0) => break,
                (l, h) => {
                    data.push(l);
                    data.push(h);
                }
            }
        }

        let (name, ..) = if is_big_endian {
            UTF_16BE.decode(&data[..])
        } else {
            UTF_16LE.decode(&data[..])
        };

        Ok(name.to_string())
    }
}

impl<R: Read + ?Sized> ReadExt for R {}
impl<R: Read + Seek + ?Sized> SeekableReadExt for R {}
