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

    // CONFIG
    let random = false;
    let write = false;
    const BUFFER_SIZE: usize = PAGESIZE_2MIB * 64;
    const DMA_SIZE: usize = PAGESIZE_2MIB;
    const BUFFER_FRAG: usize = PAGESIZE_4KIB; // DO NOT CHANGE
    const PAGE_SIZE: usize = PAGESIZE_2MIB;

    Vfio::set_pagesize(PAGE_SIZE);
    let mut latencies: Vec<u128> = vec![];

    let blocks = 8;
    // let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let mut lba = 0;

    if DMA_SIZE == PAGESIZE_4KIB {
        for _ in 0..BUFFER_SIZE / BUFFER_FRAG {
            let dma = &nvme.allocate::<u8>(DMA_SIZE)?;

            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            let before = Instant::now();
            if write {
                nvme.write(dma, lba * blocks)?;
            } else {
                nvme.read(dma, lba * blocks)?;
            }
            let elapsed = before.elapsed();

            latencies.push(elapsed.as_nanos());
        }

        write_nanos_to_file(latencies[1..].to_vec(), write, DMA_SIZE, PAGE_SIZE)?;
    } else if DMA_SIZE == PAGESIZE_2MIB {
        let mut current_dma = None;
        for i in 0..BUFFER_SIZE / BUFFER_FRAG {
            if i % 512 == 0 {
                current_dma = Some(nvme.allocate::<u8>(DMA_SIZE)?);
            }

            if let Some(ref dma) = current_dma {
                let dma_slice = dma.slice(BUFFER_FRAG * (i % 512)..BUFFER_FRAG * ((i % 512) + 1));

                lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + 1) % ns_blocks
                };

                let before = Instant::now();
                if write {
                    nvme.write(&dma_slice, lba * blocks)?;
                } else {
                    nvme.read(&dma_slice, lba * blocks)?;
                }
                let elapsed = before.elapsed();

                latencies.push(elapsed.as_nanos());
            }
        }

        write_nanos_to_file(latencies[1..].to_vec(), write, DMA_SIZE, PAGE_SIZE)?;
    }

    Ok(())
}

fn write_nanos_to_file(
    latencies: Vec<u128>,
    write: bool,
    dma_size: usize,
    page_size: usize,
) -> Result<(), Box<dyn Error>> {
    const IOMMU: &str = "tn";
    let mut file = File::create(format!(
        "latency_intmap_{}_{}ds_{}ps_{IOMMU}.txt",
        if write { "write" } else { "read" },
        size_to_string(dma_size),
        size_to_string(page_size),
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}

fn size_to_string(size: usize) -> String {
    let s = if size == PAGESIZE_4KIB {
        "4kib"
    } else if size == PAGESIZE_2MIB {
        "2mib"
    } else {
        "unknown"
    };
    s.to_owned()
}
