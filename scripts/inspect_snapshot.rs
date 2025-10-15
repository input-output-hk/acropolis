#!/usr/bin/env rust-script
//! Inspect snapshot CBOR structure
//!
//! ```cargo
//! [dependencies]
//! minicbor = "0.26.0"
//! hex = "0.4"
//! ```

use minicbor::{data::Type, decode::Decoder};
use std::env;
use std::fs::File;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <snapshot.cbor>", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    let mut f = File::open(path)?;

    // Read first 256KB to inspect structure
    let mut buffer = vec![0u8; 256 * 1024];
    let n = f.read(&mut buffer)?;
    buffer.truncate(n);

    println!("Read {} bytes from {}", n, path);
    println!("\nInspecting CBOR structure...\n");

    let mut dec = Decoder::new(&buffer);

    // Check top-level type
    match dec.datatype()? {
        Type::Array | Type::ArrayIndef => {
            let arr_len = dec.array()?;
            match arr_len {
                Some(len) => {
                    println!("✓ Top-level: Array with {} elements", len);

                    // Try to read first few elements
                    for i in 0..len.min(5) {
                        println!("\nElement [{}]:", i);
                        match dec.datatype()? {
                            Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                                let val = dec.u64()?;
                                println!("  Type: Unsigned integer");
                                println!("  Value: {}", val);
                            }
                            Type::Array | Type::ArrayIndef => {
                                let sub_len = dec.array()?;
                                println!("  Type: Array");
                                println!("  Length: {:?}", sub_len);
                                // Skip this array
                                skip_value(&mut dec)?;
                            }
                            Type::Map | Type::MapIndef => {
                                let map_len = dec.map()?;
                                println!("  Type: Map");
                                println!("  Length: {:?}", map_len);
                                // Skip this map
                                skip_value(&mut dec)?;
                            }
                            Type::Bytes | Type::BytesIndef => {
                                let bytes = dec.bytes()?;
                                println!("  Type: Bytes");
                                println!("  Length: {}", bytes.len());
                                if bytes.len() <= 32 {
                                    println!("  Value: {}", hex::encode(bytes));
                                }
                            }
                            other => {
                                println!("  Type: {:?}", other);
                                skip_value(&mut dec)?;
                            }
                        }
                    }
                }
                None => println!("✓ Top-level: Indefinite-length array"),
            }
        }
        other => {
            println!("✗ Unexpected top-level type: {:?}", other);
        }
    }

    Ok(())
}

fn skip_value(dec: &mut Decoder) -> Result<(), minicbor::decode::Error> {
    dec.skip()
}
