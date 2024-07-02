use std::error::Error;

use vroom::memory::*;
use vroom::vfio::Vfio;

use std::fs::{self, File};
use std::io::Write;

use std::{env, process};
use vroom::Allocating;
use vroom::NvmeDevice;

pub fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    args.next();

    let pci_addr = match args.next() {
        Some(arg) => arg,
        None => {
            eprintln!("Usage: cargo run --example init <pci bus id>");
            process::exit(1);
        }
    };
    Vfio::set_pagesize(PAGESIZE_2MIB);
    let mut nvme = vroom::init(&pci_addr)?;

    fs::remove_file("output.txt").ok();
    let mut output_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open("output.txt")?;

    // const ITERATIONS: u64 = 2 << 13;
    const ITERATIONS: u64 = 2 << 8;

    for _ in 0..ITERATIONS {
        // let start = std::time::Instant::now();
        nvme.allocate::<u8>(PAGESIZE_2MIB)?;
        // let duration = start.elapsed();
        // println!("{:?}", duration.as_nanos());
        // writeln!(output_file, "{:?}", duration.as_nanos())?;
    }

    Ok(())
}
