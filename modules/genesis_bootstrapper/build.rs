use anyhow::{Result};



#[tokio::main]
async fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=downloads");

    Ok(())
}
