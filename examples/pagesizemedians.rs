use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use std::{env, process, vec};
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

    let page_size = match args.next() {
        Some(arg) => arg,
        None => {
            eprintln!("Usage: cargo run --example init <pci bus id> <page size>");
            process::exit(1);
        }
    };

    let alloc_size = match args.next() {
        Some(arg) => arg.parse::<usize>().unwrap(),
        None => {
            eprintln!("no alloc size");
            process::exit(1);
        }
    };

    let page_size = match page_size.as_str() {
        "4k" => Pagesize::Page4K,
        "2m" => Pagesize::Page2M,
        "1g" => Pagesize::Page1G,
        _ => {
            eprintln!("Invalid page size");
            process::exit(1);
        }
    };

    // CONFIG
    //nvme
    let random = true;
    let write = true;

    const THRESHOLD: u128 = 10000000;

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    // let dma = nvme.allocate::<u8>(page_size.size())?;
    // nvme.write(&dma, 0)?;
    // nvme.deallocate(dma)?;

    // nvme.set_page_size(page_size.clone());
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let split_size = 1;
    let dma_size = alloc_size * page_size.size();

    let mut dma = nvme.allocate::<u8>(dma_size)?;

    let rand_block = &(0..dma_size)
        .map(|_| rand::random::<u8>())
        .collect::<Vec<_>>()[..];
    dma[0..dma_size].copy_from_slice(rand_block);

    for i in 0..alloc_size {
        previous_dmas.push(dma.slice(i * page_size.size()..(i * page_size.size()) + split_size));
    }

    println!("alloc done");

    let mut latencies: Vec<u128> = vec![];
    for _i in 0..64 {
        for previous_dma in &previous_dmas {
            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            let before = Instant::now();

            nvme.write(previous_dma, lba * blocks)?;

            let elapsed = before.elapsed();

            if elapsed.as_nanos() < THRESHOLD {
                latencies.push(elapsed.as_nanos());
            }
        }
    }
    write_nanos_to_file(latencies, write, &page_size, alloc_size, false, "pages")?;
    Ok(())
}

fn write_nanos_to_file(
    latencies: Vec<u128>,
    write: bool,
    page_size: &Pagesize,
    buffer_mult: usize,
    second_run: bool,
    extra_param: &str,
) -> Result<(), Box<dyn Error>> {
    const IOMMU: &str = "vfio";
    let mut file = File::create(format!(
        "latency_intmap_{}_{}ps_{buffer_mult}_{IOMMU}_{}_{extra_param}.txt",
        if write { "write" } else { "read" },
        page_size,
        if second_run { "second" } else { "first" },
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}
