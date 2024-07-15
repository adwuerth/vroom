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

    //max alloc size
    const ALLOC_SIZE: usize = 4096 * 2 * 2 * 2 * 2 * 2;

    // unused
    const DMA_SIZE: usize = PAGESIZE_2MIB;
    const SKIP_LATENCIES: usize = 0;
    const ALWAYS_SAME_DMA: bool = false;

    const SAME: bool = false;
    const THRESHOLD: u128 = 10000;

    const SHUFFLE_HALFWAY: bool = false;
    const SHUFFLE_FIRST: bool = false;
    const SHUFFLE_ONCE: bool = false;

    // initialise nvme with default 2mib queues -> 6 2mib iotlb entries
    let mut nvme = vroom::init(&pci_addr)?;

    nvme.set_page_size(page_size.clone());
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let split_size = PAGESIZE_4KIB;
    let dma_size = ALLOC_SIZE * split_size;

    // allocate 1GiB for all page sizes for same conditions
    let mut dma = nvme.allocate::<u8>(PAGESIZE_1GIB)?;
    let rand_block = &(0..dma_size)
        .map(|_| rand::random::<u8>())
        .collect::<Vec<_>>()[..];
    dma[0..dma_size].copy_from_slice(rand_block);
    for i in 0..ALLOC_SIZE {
        previous_dmas.push(dma.slice(i * split_size..(i + 1) * split_size));
    }

    if SHUFFLE_ONCE {
        previous_dmas.shuffle(&mut thread_rng());
    }

    let mut previous_ctr = 0;
    let mut n = 4;
    while n < (ALLOC_SIZE >> 1) {
        n *= 2;

        let mut half1: Vec<u128> = vec![];
        let mut half2: Vec<u128> = vec![];
        let mut half3: Vec<u128> = vec![];

        if SHUFFLE_FIRST {
            previous_dmas.shuffle(&mut thread_rng());
        }

        // reverse slice to prevent any weird buffering
        for previous_dma in previous_dmas[previous_ctr..previous_ctr + n].iter().rev() {
            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            let before = Instant::now();
            if write {
                nvme.write(previous_dma, lba * blocks)?;
            } else {
                nvme.read(previous_dma, lba * blocks)?;
            }
            let elapsed = before.elapsed();

            if elapsed.as_nanos() < THRESHOLD {
                half1.push(elapsed.as_nanos());
            }
        }

        if SHUFFLE_HALFWAY {
            previous_dmas.shuffle(&mut thread_rng());
        }

        for previous_dma in &previous_dmas[previous_ctr..previous_ctr + n] {
            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            let before = Instant::now();
            if write {
                nvme.write(previous_dma, lba * blocks)?;
            } else {
                nvme.read(previous_dma, lba * blocks)?;
            }
            let elapsed = before.elapsed();
            if elapsed.as_nanos() < THRESHOLD {
                half2.push(elapsed.as_nanos());
            }
            if SAME {
                lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + 1) % ns_blocks
                };

                let before = Instant::now();
                if write {
                    nvme.write(previous_dma, lba * blocks)?;
                } else {
                    nvme.read(previous_dma, lba * blocks)?;
                }
                let elapsed = before.elapsed();
                if elapsed.as_nanos() < THRESHOLD {
                    half3.push(elapsed.as_nanos());
                }
            }
        }

        write_nanos_to_file(
            half1[SKIP_LATENCIES..].to_vec(),
            write,
            DMA_SIZE,
            &page_size,
            n,
            false,
            ALWAYS_SAME_DMA,
        )?;

        write_nanos_to_file(
            half2[SKIP_LATENCIES..].to_vec(),
            write,
            DMA_SIZE,
            &page_size,
            n,
            true,
            ALWAYS_SAME_DMA,
        )?;

        if SAME {
            write_nanos_to_file(
                half3[SKIP_LATENCIES..].to_vec(),
                write,
                DMA_SIZE,
                &page_size,
                n,
                true,
                true,
            )?;
        }
        previous_ctr += n;
    }
    Ok(())
}

fn write_nanos_to_file(
    latencies: Vec<u128>,
    write: bool,
    dma_size: usize,
    page_size: &Pagesize,
    buffer_mult: usize,
    second_run: bool,
    same: bool,
) -> Result<(), Box<dyn Error>> {
    const IOMMU: &str = "pt";
    let mut file = File::create(format!(
        "latency_intmap_{}_{}ds_{}ps_{buffer_mult}_{IOMMU}_{}_{}.txt",
        if write { "write" } else { "read" },
        size_to_string(dma_size),
        page_size,
        if second_run { "second" } else { "first" },
        if same { "same" } else { "diff" }
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
