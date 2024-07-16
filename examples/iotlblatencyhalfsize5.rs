use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
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

    const THRESHOLD: u128 = 10000;

    // initialise nvme with default 2mib queues -> 6 2mib iotlb entries
    let mut nvme = vroom::init(&pci_addr)?;

    nvme.set_page_size(page_size.clone());
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let split_size = PAGESIZE_4KIB * 4;

    let mut dma = nvme.allocate::<u8>(PAGESIZE_1GIB)?;
    let rand_block = &(0..PAGESIZE_1GIB)
        .map(|_| rand::random::<u8>())
        .collect::<Vec<_>>()[..];
    dma[0..PAGESIZE_1GIB].copy_from_slice(rand_block);
    for i in 0..PAGESIZE_1GIB / split_size {
        previous_dmas.push(dma.slice(i * split_size..(i + 1) * split_size));
    }

    let mut latencies: Vec<u128> = vec![];

    // reverse slice to prevent any weird buffering
    for previous_dma in &previous_dmas {
        lba = if random {
            rng.gen_range(0..ns_blocks)
        } else {
            (lba + 1) % ns_blocks
        };
        // println!("submitting write to: {}", previous_dma.phys);
        let before = Instant::now();

        let res = nvme.write(previous_dma, lba * blocks);

        let elapsed = before.elapsed();
        if res.is_err() {
            println!("error!");
            return Ok(());
        }

        // println!("write done");
        if elapsed.as_nanos() < THRESHOLD {
            latencies.push(elapsed.as_nanos());
        }
    }

    write_nanos_to_file(latencies, write, &page_size, 0, false, "a")?;

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
    const IOMMU: &str = "pt";
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
