use std::error::Error;

use libc::statvfs64;
use vroom::memory::*;

use std::fs::{self, File};
use std::io::Write;

use std::{env, process};
use vroom::Allocating;
use vroom::NvmeDevice;

use rand::Rng;
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

    let mut nvme = vroom::init(&pci_addr)?;

    fs::remove_file("output.txt").ok();
    // let mut output_file = std::fs::OpenOptions::new()
    //     .read(true)
    //     .write(true)
    //     .create(true)
    //     .open("output.txt")?;

    // const ITERATIONS: u64 = 2 << 13;
    const ITERATIONS: u64 = 2 << 6;

    // let buffer = nvme.allocate::<u8>(PAGESIZE_2MIB)?;

    let random = true;

    let mut latencies = Vec::new();
    let mut lba = 0;
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let mut rng = rand::thread_rng();

    for _ in 0..ITERATIONS {
        lba = if random {
            rng.gen_range(0..ns_blocks)
        } else {
            (lba + 1) % ns_blocks
        };

        let mut total = 0;

        let start = std::time::Instant::now();
        let buffer = nvme.allocate::<u8>(PAGESIZE_2MIB)?;
        let duration = start.elapsed();
        //let mut total = duration.as_nanos();

        // println!("{:?}", duration.as_nanos());
        // writeln!(output_file, "{:?}", duration.as_nanos())?;

        for b in 0..512 {
            let slice = &buffer.slice(PAGESIZE_4KIB * b..PAGESIZE_4KIB * (b + 1));

            let start = std::time::Instant::now();
            nvme.read(slice, lba)?;
            let duration = start.elapsed();
            total += duration.as_nanos();

            if b != 0 {
                latencies.push(total);
            }
            total = 0;
        }
    }

    write_nanos_to_file(latencies, false)?;
    Ok(())
}

fn write_nanos_to_file(latencies: Vec<u128>, write: bool) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(format!(
        "vroom_qd1_{}_latencies.txt",
        if write { "write" } else { "read" }
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}
