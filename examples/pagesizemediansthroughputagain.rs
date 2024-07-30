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
use vroom::{memory::*, Mapping};
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

    let duration = match args.next() {
        Some(secs) => Duration::from_secs(
            secs.parse()
                .expect("Usage: cargo run --example init <pci bus id> <duration in seconds>"),
        ),
        None => process::exit(1),
    };

    // CONFIG
    //nvme
    let random = true;
    let write = true;

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let dma_mult = PAGESIZE_4KIB;

    let dma_size = alloc_size * dma_mult;

    let mut dma = nvme.allocate::<u8>(dma_size)?;

    let rand_block = &(0..dma_size)
        .map(|_| rand::random::<u8>())
        .collect::<Vec<_>>()[..];
    dma[0..dma_size].copy_from_slice(rand_block);

    // for i in 0..alloc_size / split_mult {
    //     // let rand_block = &(i * dma_mult..(i * dma_mult) + PAGESIZE_4KIB)
    //     //     .map(|_| rand::random::<u8>())
    //     //     .collect::<Vec<_>>()[..];
    //     // dma[i * dma_mult..(i * dma_mult) + PAGESIZE_4KIB].copy_from_slice(rand_block);
    //     previous_dmas.push(dma.slice(i * dma_mult..(i * dma_mult) + split_size));
    // }

    let unit_size = PAGESIZE_4KIB;

    // for i in 0..dma_size / unit_size {
    //     previous_dmas.push(dma.slice(i * unit_size..(i + 1) * unit_size));
    // }
    for i in 0..dma_size / unit_size {
        previous_dmas.push(dma.slice(i * unit_size..(i * unit_size) + 1));
    }

    println!("alloc done");

    let mut total = Duration::ZERO;

    let mut ios = 0;

    let mut latencies: Vec<u128> = vec![];
    while total < duration {
        for previous_dma in &previous_dmas {
            lba = if random {
                rng.gen_range(0..ns_blocks)
            } else {
                (lba + 1) % ns_blocks
            };

            let before = Instant::now();

            nvme.write(previous_dma, lba * blocks)?;

            let elapsed = before.elapsed();

            total += elapsed;
            ios += 1;

            latencies.push(elapsed.as_nanos());

            if total > duration {
                break;
            }
        }
    }

    let median = median(latencies.clone()).unwrap();

    println!(
        "total: {}, median: {}",
        total.as_micros() / previous_dmas.len() as u128,
        median
    );
    let iops = ios as f64 / total.as_secs_f64();
    write_latency(&page_size, &duration, dma_size, 1, 1, median, iops);

    // nvme.deallocate(dma)?;

    write_nanos_to_file(latencies, write, &page_size, alloc_size, "")?;
    Ok(())
}

fn write_latency(
    page_size: &Pagesize,
    duration: &Duration,
    dma_size: usize,
    queue_depth: usize,
    thread_count: usize,
    latency: u128,
    iops: f64,
) {
    let fname = format!(
        "write_{}_qd{queue_depth}_t{thread_count}_{}s_{dma_size}alloc",
        page_size,
        duration.as_secs()
    );

    let mut file = File::create(fname).unwrap();

    writeln!(file, "{}", latency).unwrap();

    writeln!(file, "{}", iops).unwrap();
}

fn median(mut latencies: Vec<u128>) -> Option<u128> {
    let len = latencies.len();
    if len == 0 {
        return None;
    }
    latencies.sort_unstable();
    if len % 2 == 1 {
        Some(latencies[len / 2])
    } else {
        Some((latencies[len / 2 - 1] + latencies[len / 2]) / 2)
    }
}

fn write_nanos_to_file(
    latencies: Vec<u128>,
    write: bool,
    page_size: &Pagesize,
    buffer_mult: usize,
    extra_param: &str,
) -> Result<(), Box<dyn Error>> {
    const IOMMU: &str = "vfio";
    let mut file = File::create(format!(
        "latenciesqd1t1_{}_{}ps_{buffer_mult}_{IOMMU}_{extra_param}.txt",
        if write { "write" } else { "read" },
        page_size,
    ))?;
    for lat in latencies {
        writeln!(file, "{}", lat)?;
    }
    Ok(())
}
