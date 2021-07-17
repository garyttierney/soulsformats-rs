use std::fs;
use std::fs::File;

use std::io::{BufReader, Cursor, Read};

use souls_types::bnd4::Bnd4Archive;
use souls_types::DcxReader;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <filename>", args[0]);
        return;
    }

    let fname = std::path::Path::new(&*args[1]);
    let output_dir = std::path::Path::new(&*args[2]);
    let file = fs::File::open(&fname).unwrap();

    let mut dcx_reader = DcxReader::new(BufReader::new(file)).unwrap();
    let mut output = Vec::new();
    dcx_reader.read_to_end(&mut output).unwrap();

    let mut archive = Bnd4Archive::new(Cursor::new(&output[..])).unwrap();

    for index in 0..archive.len() {
        let mut file = archive.file(index).unwrap();
        let archive_file_path = std::path::Path::new(file.name().unwrap());
        let disk_file_path = output_dir.join(archive_file_path.file_name().unwrap());
        let mut output_file = File::create(disk_file_path).unwrap();

        std::io::copy(&mut file, &mut output_file).expect("unable to extract file");
    }
}
