use std::io;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};
use std::ops::Deref;

use bitflags::bitflags;
use byteorder::{BigEndian, ReadBytesExt};
use byteorder::{ByteOrder, LittleEndian};

use crate::io::{invalid_data, ReadExt, SeekableReadExt};
use crate::DcxReader;

bitflags! {
    #[repr(C)]
    struct Bnd4EntryFlags : u8 {
        const COMPRESSED = 0b0010_0000;
        const HAS_ID = 0b0000_0010;
        const HAS_NAME = 0b0000_0100;
        const HAS_NAME_2 = 0b0000_1000;
        const UNK = 0b0100_0000;
    }
}

bitflags! {
    #[repr(C)]
    struct Bnd4Flags : u8 {
        /// File is big-endian regardless of the big-endian byte.
        const BIG_ENDIAN = 0b0000_0001;

        /// Files have ID numbers.
        const SUPPORTS_IDS = 0b0000_0010;

        /// Files have name strings; Names2 may or may not be set. Perhaps the distinction is related to whether it's a full path or just the filename?
        const SUPPORTS_PATHS = 0b0000_0100;

        /// </summary>
        const SUPPORTS_NAMES = 0b0000_1000;

        /// File data offsets are 64-bit.
        const LONG_OFFSETS = 0b0001_0000;

        /// Files may be compressed.
        const SUPPORTS_COMPRESSION = 0b0010_0000;
    }
}

impl Bnd4Flags {
    pub fn has_long_offsets(&self) -> bool {
        self.contains(Self::LONG_OFFSETS)
    }

    pub fn supports_filenames(&self) -> bool {
        self.intersects(Self::SUPPORTS_PATHS | Self::SUPPORTS_NAMES)
    }

    pub fn supports_ids(&self) -> bool {
        self.contains(Self::SUPPORTS_IDS)
    }
}

pub struct Bnd4Archive<R: Read + Seek> {
    archive_info: Bnd4ArchiveInfo,
    entries: Vec<Bnd4FileInfo>,

    /// Used to decide how to parse UTF-16 names when we look them up for a file.
    is_big_endian: bool,
    reader: R,
}

pub struct Bnd4ArchiveInfo {
    file_count: u32,
    header_size: u64,
    version: [u8; 8],
    file_header_size: u64,
    file_header_end: u64,
    unicode: bool,
    format: Bnd4Flags,
    extended: bool,
    name_buckets_offset: u64,
}

impl<R: Read + Seek> Deref for Bnd4Archive<R> {
    type Target = Bnd4ArchiveInfo;

    fn deref(&self) -> &Self::Target {
        &self.archive_info
    }
}

impl<R: Read + Seek> Bnd4Archive<R> {
    /// Reads the archive information from the header. `is_big_endian` is stored
    /// to decide how to parse UTF-16 names.
    fn read_archive_info<Order: ByteOrder>(
        reader: &mut R,
        is_flags_le: bool,
    ) -> Result<Bnd4ArchiveInfo, std::io::Error> {
        let file_count = reader.read_u32::<Order>()?;
        let header_size = reader.read_u64::<Order>()?;

        let mut version = [0u8; 8];
        reader.read_exact(&mut version)?;

        let file_header_size = reader.read_u64::<Order>()?;
        let file_header_end = reader.read_u64::<Order>()?;
        let unicode = reader.read_bool()?;

        let mut format_bits = reader.read_u8()?;
        if !is_flags_le {
            format_bits = format_bits.reverse_bits();
        }

        let format = Bnd4Flags::from_bits(format_bits)
            .ok_or_else(|| invalid_data("Invalid BND4 archive flags"))?;

        let extended = reader.read_bool()?;
        reader.expect_u8(0)?;
        reader.read_u32::<Order>()?;
        let name_buckets_offset = reader.read_u64::<Order>()?;

        Ok(Bnd4ArchiveInfo {
            file_count,
            header_size,
            version,
            file_header_size,
            file_header_end,
            unicode,
            format,
            extended,
            name_buckets_offset,
        })
    }

    fn read_entry_info<Order: ByteOrder>(
        reader: &mut R,
        format: Bnd4Flags,
        is_flags_little_endian: bool,
    ) -> Result<Bnd4FileInfo, Error> {
        let mut flag_bits = reader.read_u8()?;
        if !is_flags_little_endian {
            flag_bits = flag_bits.reverse_bits();
        }

        let flags = Bnd4EntryFlags::from_bits(flag_bits)
            .ok_or_else(|| invalid_data("Invalid BND4 entry flags"))?;

        reader.expect_u8(0)?;
        reader.expect_u8(0)?;
        reader.expect_u8(0)?;
        reader.expect_i32::<Order>(-1)?;

        let size = reader.read_u64::<Order>()?;
        let decompressed_size = if format.contains(Bnd4Flags::SUPPORTS_COMPRESSION) {
            Some(reader.read_u64::<Order>()?)
        } else {
            None
        };

        let data_offset = if format.has_long_offsets() {
            reader.read_u64::<Order>()?
        } else {
            reader.read_u32::<Order>()? as u64
        };

        let id = if format.supports_ids() {
            Some(reader.read_i32::<Order>()?)
        } else {
            None
        };

        let name_offset = if format.supports_filenames() {
            Some(reader.read_u32::<Order>()?)
        } else {
            None
        };

        Ok(Bnd4FileInfo {
            flags,
            size,
            data_offset,
            decompressed_size,
            id,
            name_offset,
        })
    }

    pub fn file(&mut self, index: usize) -> Result<Bnd4File, Error> {
        if index >= self.archive_info.file_count as usize {
            panic!(); // @TODO: error
        }

        let is_big_endian_str = self.is_big_endian;
        let is_unicode = self.unicode;

        let entry = &self.entries[index];
        let name = match entry.name_offset {
            Some(offset) => Some(self.reader.at(offset as u64, |r| {
                if is_unicode {
                    r.read_utf16(is_big_endian_str)
                } else {
                    r.read_cstr()
                }
            })?),
            None => None,
        };

        self.reader.seek(SeekFrom::Start(entry.data_offset))?;

        let data = (&mut self.reader as &mut dyn Read).take(entry.size);
        let reader = if entry.flags.contains(Bnd4EntryFlags::COMPRESSED) {
            Bnd4FileReader::Compressed(DcxReader::new(data)?)
        } else {
            Bnd4FileReader::Uncompressed(data)
        };

        Ok(Bnd4File {
            archive: &self.archive_info,
            entry,
            name,
            reader,
        })
    }

    pub fn len(&self) -> usize {
        self.archive_info.file_count as usize
    }

    /// Read the archive information and file listing using the byte order specified in the archive header.
    fn new_from_header<O: ByteOrder>(
        reader: &mut R,
        is_flags_little_endian: bool,
    ) -> Result<(Bnd4ArchiveInfo, Vec<Bnd4FileInfo>), Error> {
        let archive_info = Self::read_archive_info::<O>(reader, is_flags_little_endian)?;

        let mut entries = Vec::with_capacity(archive_info.file_count as usize);
        for _ in 0..archive_info.file_count {
            let entry_info =
                Self::read_entry_info::<O>(reader, archive_info.format, is_flags_little_endian)?;

            entries.push(entry_info);
        }

        Ok((archive_info, entries))
    }

    pub fn new(mut reader: R) -> Result<Self, std::io::Error> {
        reader.expect(b"BND4")?;

        // unk04
        let _ = reader.read_bool()?;
        // unk05
        let _ = reader.read_bool()?;

        reader.expect_u8(0)?;
        reader.expect_u8(0)?;
        reader.expect_u8(0)?;

        let is_big_endian = reader.read_bool()?;
        let is_flags_little_endian = reader.read_bool()?;
        reader.read_u8()?;

        let (archive_info, entries) = if is_big_endian {
            Self::new_from_header::<BigEndian>(&mut reader, is_flags_little_endian)
        } else {
            Self::new_from_header::<LittleEndian>(&mut reader, is_flags_little_endian)
        }?;

        Ok(Self {
            archive_info,
            is_big_endian,
            entries,
            reader,
        })
    }
}

pub struct Bnd4File<'a> {
    archive: &'a Bnd4ArchiveInfo,
    entry: &'a Bnd4FileInfo,
    reader: Bnd4FileReader<'a>,
    name: Option<String>,
}

pub struct Bnd4FileInfo {
    flags: Bnd4EntryFlags,
    size: u64,
    data_offset: u64,
    decompressed_size: Option<u64>,
    id: Option<i32>,
    name_offset: Option<u32>,
}

enum Bnd4FileReader<'archive> {
    Uncompressed(io::Take<&'archive mut dyn Read>),
    Compressed(DcxReader<io::Take<&'archive mut dyn Read>>),
}

impl<'archive> Bnd4File<'archive> {
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl<'archive> Deref for Bnd4File<'archive> {
    type Target = Bnd4FileInfo;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl<'archive> Read for Bnd4File<'archive> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        match &mut self.reader {
            Bnd4FileReader::Compressed(ref mut r) => r.read(buf),
            Bnd4FileReader::Uncompressed(ref mut r) => r.read(buf),
        }
    }
}
