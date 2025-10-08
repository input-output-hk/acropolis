// Build-time script to download generics
use blake2::{digest::consts::U32, Blake2b, Digest};
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

const OUTPUT_DIR: &str = "downloads";

/// Download a URL to a file in OUTPUT_DIR
async fn download(
    client: &reqwest::Client,
    url: &str,
    filename: &str,
    hash_filename: Option<&str>,
) -> Result<()> {
    let request = client.get(url).build().with_context(|| format!("Failed to request {url}"))?;
    let response =
        client.execute(request).await.with_context(|| format!("Failed to fetch {url}"))?;
    let data = response.bytes().await.context("Failed to read response")?;

    let output_path = Path::new(OUTPUT_DIR);
    if !output_path.exists() {
        fs::create_dir_all(output_path)
            .with_context(|| format!("Failed to create {OUTPUT_DIR} directory"))?;
    }

    if let Some(hash_filename) = hash_filename {
        // hash the data
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(data.as_ref());
        let hash = hasher.finalize();
        let hash_file_path = output_path.join(hash_filename);
        let mut hash_file = fs::File::create(&hash_file_path)
            .with_context(|| format!("Failed to create file {}", hash_file_path.display()))?;

        hash_file
            .write_all(hex::encode(hash.as_slice()).as_bytes())
            .with_context(|| format!("Failed to write file {}", hash_file_path.display()))?;
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
            None,
        ),
        download(
            &client,
            "https://book.world.dev.cardano.org/environments/mainnet/shelley-genesis.json",
            "mainnet-shelley-genesis.json",
            Some("mainnet-shelley-genesis.hash"),
        ),
        download(
            &client,
            "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/byron-genesis.json",
            "sanchonet-byron-genesis.json",
            None,
        ),
        download(
            &client,
            "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/shelley-genesis.json",
            "sanchonet-shelley-genesis.json",
            Some("sanchonet-shelley-genesis.hash"),
        )
    )?;

    Ok(())
}
