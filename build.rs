use std::env;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/sample-app");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("sample-app.zip.br");

    // Create zip in memory
    let mut zip_buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut zip_buf);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // Bundle format: seal-game-of-life/content/<files>
        let app_name = "seal-game-of-life";
        zip.add_directory(format!("{app_name}/"), options).unwrap();
        zip.add_directory(format!("{app_name}/content/"), options).unwrap();

        let sample_dir = Path::new("assets/sample-app");
        for entry in fs::read_dir(sample_dir).expect("assets/sample-app must exist") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap().to_str().unwrap();
                zip.start_file(format!("{app_name}/content/{name}"), options).unwrap();
                zip.write_all(&fs::read(&path).unwrap()).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    // Brotli-compress the zip
    let zip_bytes = zip_buf.into_inner();
    let mut br_buf = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(&mut br_buf, 4096, 9, 22);
        encoder.write_all(&zip_bytes).unwrap();
    }

    fs::write(&dest, &br_buf).unwrap();
}
