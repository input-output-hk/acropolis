// Build-time script to download generics
use reqwest::blocking::get;
use std::fs;
use std::io::Write;
use std::path::Path;

const OUTPUT_DIR: &str = "downloads";

/// Download a URL to a file in OUTPUT_DIR
fn download(url_base: &str, epoch: &str, filename: &str, rename: &Vec<(&str,&str)>) {
    let url = format!("{}/{}-genesis.json", url_base, epoch);
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

    let shelley_fix = vec![
        ("slotsPerKESPeriod","slotsPerKesPeriod"),("maxKESEvolutions","maxKesEvolutions")
    ];

    let main = "https://book.world.dev.cardano.org/environments/mainnet";
    download(main, "byron", "mainnet-byron-genesis.json", &vec![]);
    download(main, "shelley", "mainnet-shelley-genesis.json", &shelley_fix);
    download(main, "alonzo", "mainnet-alonzo-genesis.json", &vec![]);
    download(main, "conway", "mainnet-conway-genesis.json", &vec![]);

    let sancho = 
        "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis";
    download(sancho, "byron", "sanchonet-byron-genesis.json", &vec![]);
    download(sancho, "shelley", "sanchonet-shelley-genesis.json", &shelley_fix);
    download(sancho, "alonzo", "sanchonet-alonzo-genesis.json", &vec![]);
    download(sancho, "conway", "sanchonet-conway-genesis.json", &vec![]);
}
