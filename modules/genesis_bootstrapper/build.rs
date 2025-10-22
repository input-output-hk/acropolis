// Build-time script to download generics
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::from_reader;

const OUTPUT_DIR: &str = "downloads";

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    if let Ok(path) = env::var("ACROPOLIS_OFFLINE_MIRROR") {
        if let Ok(file) = File::open(&path) {
            if let Ok(map) = from_reader::<_, HashMap<String, String>>(file) {
                if let Some(path) = map.get(url) {
                    if let Ok(bytes) = fs::read(&Path::new(path).to_path_buf()) {
                        return Ok(bytes);
                    }
                }
            }
        }
    }
    let req = client.get(url).build().with_context(|| format!("Failed to request {url}"))?;
    let resp = client.execute(req).await.with_context(|| format!("Failed to fetch {url}"))?;
    Ok(resp.bytes().await.context("Failed to read response")?.to_vec())
}

/// Download a URL to a file in OUTPUT_DIR
async fn download(client: &reqwest::Client, url: &str, filename: &str) -> Result<()> {
    let data = fetch_bytes(client, url).await?;

    let output_path = Path::new(OUTPUT_DIR);
    if !output_path.exists() {
        fs::create_dir_all(output_path)
            .with_context(|| format!("Failed to create {OUTPUT_DIR} directory"))?;
    }

    let file_path = output_path.join(filename);
    let mut file = fs::File::create(&file_path)
        .with_context(|| format!("Failed to create file {}", file_path.display()))?;
    file.write_all(data.as_ref())
        .with_context(|| format!("Failed to write file {}", file_path.display()))?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs"); // Ensure the script runs if modified
    let client = reqwest::Client::new();

    tokio::try_join!(
        download(
            &client,
            "https://book.world.dev.cardano.org/environments/mainnet/byron-genesis.json",
            "mainnet-byron-genesis.json",
        ),
        download(
            &client,
            "https://book.world.dev.cardano.org/environments/mainnet/shelley-genesis.json",
            "mainnet-shelley-genesis.json",
        ),
        download(
            &client,
            "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/byron-genesis.json",
            "sanchonet-byron-genesis.json",
        ),
        download(
            &client,
            "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/shelley-genesis.json",
            "sanchonet-shelley-genesis.json",
        )
    )?;

    Ok(())
}
