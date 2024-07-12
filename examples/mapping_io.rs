use rand::seq::SliceRandom;
use rand::thread_rng;
use std::error::Error;
use vroom::memory::*;
use vroom::vfio::Vfio;

use std::fs::{self, File};
use std::io::Write;

use std::{env, process};
use vroom::Allocating;

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

    const PAGE_SIZE: Pagesize = Pagesize::Page2M;
    let mut nvme = vroom::init(&pci_addr)?;
    nvme.set_page_size(PAGE_SIZE);

    // const ITERATIONS: u64 = 2 << 13;
    const ITERATIONS: u64 = 2 << 5;

    // let buffer = nvme.allocate::<u8>(PAGESIZE_2MIB)?;

    let random = false;

    let mut latencies = Vec::new();
    let mut lba = 0;
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let mut rng = rand::thread_rng();

    for _ in 0..ITERATIONS {
        let mut total = 0;

        //let mut total = duration.as_nanos();

        // println!("{:?}", duration.as_nanos());
        // writeln!(output_file, "{:?}", duration.as_nanos())?;
        let mut buffers = vec![];
        if PAGE_SIZE == Pagesize::Page2M {
            let buffer = nvme.allocate::<u8>(PAGESIZE_2MIB)?;
            for b in 0..512 {
                let slice = buffer.slice(PAGESIZE_4KIB * b..PAGESIZE_4KIB * (b + 1));
                buffers.push(slice);
            }
        } else {
            for _b in 0..512 {
                let buffer = nvme.allocate::<u8>(PAGESIZE_4KIB)?;
                buffers.push(buffer);
            }
        }

        // buffers.shuffle(&mut rng);

        for buffer in buffers {
            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            lba = 0;
            let start = std::time::Instant::now();
            nvme.read(&buffer, lba)?;
            let duration = start.elapsed();
            total += duration.as_nanos();

            latencies.push(total);

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
