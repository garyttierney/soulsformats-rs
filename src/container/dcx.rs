use std::io::Read;
use std::mem::size_of;

use byteorder::BigEndian;
use flate2::read::ZlibDecoder;
use zerocopy::{AsBytes, ByteSlice, FromBytes, LayoutVerified, Unaligned, U32};

#[derive(FromBytes, AsBytes, Debug)]
#[repr(C)]
pub struct Metadata {
    dcx_magic: [u8; 4],
    format_magic: [u8; 4],
    dcs_offset: U32<BigEndian>,
    dcp_offset: U32<BigEndian>,
    unk1: U32<BigEndian>,
    unk2: U32<BigEndian>,
    dcs_magic: [u8; 4],
    compressed_size: U32<BigEndian>,
    size: U32<BigEndian>,
    dcp_magic: [u8; 4],
    algorithm: [u8; 4],
    unk3: [u32; 6],
    dca_magic: [u8; 4],
    dca_size: U32<BigEndian>,
}

pub struct DcxReader<R: Read> {
    codec: DcxCompressionCodec<R>,
}

pub enum DcxCompressionCodec<R: Read> {
    Deflate(ZlibDecoder<R>),
}

impl<R: Read> DcxReader<R> {
    pub fn new(mut reader: R) -> Result<Self, std::io::Error> {
        let mut header_buffer = [0u8; size_of::<Metadata>()];
        reader.read_exact(&mut header_buffer)?;

        let header = LayoutVerified::<_, Metadata>::new(&header_buffer[..]).unwrap();
        assert_eq!(&header.dcx_magic, b"DCX\0");
        assert_eq!(&header.dcp_magic, b"DCP\0");
        assert_eq!(&header.dcs_magic, b"DCS\0");

        let codec = match &header.algorithm {
            b"DFLT" => DcxCompressionCodec::Deflate(ZlibDecoder::new(reader)),
            b"KRAK" => unimplemented!("Oodle Kraken"),
            b"EDGE" => unimplemented!("Edge?"),
            _ => unimplemented!(),
        };

        Ok(Self { codec })
    }
}

impl<R: Read> Read for DcxReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.codec {
            DcxCompressionCodec::Deflate(compressor) => compressor.read(buf),
            _ => unimplemented!(),
        }
    }
}
