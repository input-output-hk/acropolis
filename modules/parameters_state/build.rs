// Build-time script to download generics
use reqwest::blocking::get;
use serde_json::from_reader;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;

const OUTPUT_DIR: &str = "downloads";

fn fetch_text(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(path) = env::var("ACROPOLIS_OFFLINE_MIRROR") {
        if !path.is_empty() {
            if let Ok(file) = File::open(path) {
                if let Ok(map) = from_reader::<_, HashMap<String, String>>(file) {
                    if let Some(path_str) = map.get(url) {
                        if let Ok(s) = fs::read_to_string(&Path::new(path_str).to_path_buf()) {
                            return Ok(s);
                        }
                    }
                }
            }
        }
    }
    Ok(get(url)?.error_for_status()?.text()?)
}

/// Download a URL to a file in OUTPUT_DIR
fn download(url_base: &str, epoch: &str, filename: &str, rename: &Vec<(&str, &str)>) {
    let url = format!("{}/{}-genesis.json", url_base, epoch);
    let mut data = fetch_text(&url).expect("Failed to fetch {url}");

    for (what, with) in rename.iter() {
        data = data.replace(&format!("\"{what}\""), &format!("\"{with}\""));
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
        ("slotsPerKESPeriod", "slotsPerKesPeriod"),
        ("maxKESEvolutions", "maxKesEvolutions"),
    ];

    let main = "https://book.world.dev.cardano.org/environments/mainnet";
    download(main, "byron", "mainnet-byron-genesis.json", &vec![]);
    download(
        main,
        "shelley",
        "mainnet-shelley-genesis.json",
        &shelley_fix,
    );
    download(main, "alonzo", "mainnet-alonzo-genesis.json", &vec![]);
    download(main, "conway", "mainnet-conway-genesis.json", &vec![]);

    let sancho =
        "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis";
    download(sancho, "byron", "sanchonet-byron-genesis.json", &vec![]);
    download(
        sancho,
        "shelley",
        "sanchonet-shelley-genesis.json",
        &shelley_fix,
    );
    download(sancho, "alonzo", "sanchonet-alonzo-genesis.json", &vec![]);
    download(sancho, "conway", "sanchonet-conway-genesis.json", &vec![]);
}
