use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::time::Instant;
use std::{env, process, vec};
use vroom::vfio::Vfio;
use vroom::{memory::*, Allocating};

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

    nvme.set_page_size(Pagesize::Page2M);

    // CONFIG
    let random = false;
    let write = false;
    const BUFFER_SIZE: usize = PAGESIZE_2MIB * 32;
    const DMA_SIZE: usize = PAGESIZE_4KIB;
    const SINGLE_ADDRESS: bool = false;

    let buffer = nvme.allocate::<u8>(BUFFER_SIZE)?;

    let mut random_buffer_slices = vec![];

    for i in 0..BUFFER_SIZE / DMA_SIZE {
        random_buffer_slices.push(buffer.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB));
    }

    random_buffer_slices.shuffle(&mut thread_rng());

    let mut latencies: Vec<u128> = vec![];

    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let mut lba = 0;

    let dma_single = &random_buffer_slices[0];

    if SINGLE_ADDRESS {
        for _ in 0..random_buffer_slices.len() {
            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            let before = Instant::now();
            if write {
                nvme.write(dma_single, lba * blocks)?;
            } else {
                nvme.read(dma_single, lba * blocks)?;
            }
            let elapsed = before.elapsed();

            latencies.push(elapsed.as_nanos());
        }
        write_nanos_to_file(latencies[1..].to_vec(), write)?;
        return Ok(());
    }

    for dma in random_buffer_slices {
        lba = if random {
            rng.gen_range(0..ns_blocks)
        } else {
            (lba + 1) % ns_blocks
        };

        let before = Instant::now();
        if write {
            nvme.write(&dma, lba * blocks)?;
        } else {
            nvme.read(&dma, lba * blocks)?;
        }
        let elapsed = before.elapsed();

        latencies.push(elapsed.as_nanos());
    }

    write_nanos_to_file(latencies[1..].to_vec(), write)?;

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
