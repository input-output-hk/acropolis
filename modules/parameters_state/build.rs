// Build-time script to download generics
use reqwest::blocking::get;
use std::fs;
use std::io::Write;
use std::path::Path;

const OUTPUT_DIR: &str = "downloads";

/// Download a URL to a file in OUTPUT_DIR
fn download(url: &str, filename: &str, rename: &Vec<(&str,&str)>) {
    let response = get(url).expect("Failed to fetch {url}");
    let mut data = response.text().expect("Failed to read response");

    for (what,with) in rename.iter() {
        data = data.replace(&format!("\"{what}\""),&format!("\"{with}\""));
    }

    let output_path = Path::new(OUTPUT_DIR);
    if !output_path.exists() {
        fs::create_dir_all(output_path).expect("Failed to create {OUTPUT_DIR} directory");
    }

    let file_path = output_path.join(filename);
    let mut file = fs::File::create(&file_path).expect("Failed to create file {file_path}");
    file.write_all(data.as_bytes()).expect("Failed to write file {file_path}");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs"); // Ensure the script runs if modified

    download(
        "https://book.world.dev.cardano.org/environments/mainnet/byron-genesis.json",
        "mainnet-byron-genesis.json",
        &vec![]
    );
    download(
        "https://book.world.dev.cardano.org/environments/mainnet/shelley-genesis.json",
        "mainnet-shelley-genesis.json",
        &vec![("slotsPerKESPeriod","slotsPerKesPeriod"),("maxKESEvolutions","maxKesEvolutions")]
    );
    download(
        "https://book.world.dev.cardano.org/environments/mainnet/alonzo-genesis.json",
        "mainnet-alonzo-genesis.json",
        &vec![]
    );
    download(
        "https://book.world.dev.cardano.org/environments/mainnet/conway-genesis.json",
        "mainnet-conway-genesis.json",
        &vec![]
    );
}
