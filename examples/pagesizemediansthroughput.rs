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

    // const THRESHOLD: u128 = 10000000;

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    // let mut nvme = vroom::init(&pci_addr)?;
    // nvme.set_page_size(page_size.clone());

    // let dma = nvme.allocate::<u8>(page_size.size())?;
    // nvme.write(&dma, 0)?;
    // nvme.deallocate(dma)?;

    // nvme.set_page_size(page_size.clone());
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    println!("ns_blocks: {ns_blocks}");
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let dma_mult = PAGESIZE_4KIB;

    let dma_size = alloc_size * dma_mult;
    let mut total = Duration::ZERO;
    let mut ios = 0;

    let reps = 60;

    let mut latencies = vec![];
    let mut dma = nvme.allocate::<u8>(dma_size)?;

    let rand_block = &(0..dma_size)
        .map(|_| rand::random::<u8>())
        .collect::<Vec<_>>()[..];
    dma[0..dma_size].copy_from_slice(rand_block);

    // for i in 0..dma_size / PAGESIZE_4KIB {
    //     previous_dmas.push(dma.slice(i * PAGESIZE_4KIB..(i * PAGESIZE_4KIB) + 1));
    // }

    let split_size = 512;

    let bytes_len = 1;

    for i in 0..dma_size / split_size {
        previous_dmas.push(dma.slice(i * split_size..(i * split_size) + bytes_len));
    }
    for _ in 0..reps {
        println!("calling allocate with dma_size {dma_size}");

        let mut in_loop = Duration::ZERO;
        previous_dmas.shuffle(&mut thread_rng());

        while in_loop < duration / reps {
            for previous_dma in &previous_dmas {
                lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + 1) % ns_blocks
                };

                let before = Instant::now();

                nvme.write(previous_dma, lba)?;
                // nvme.read(previous_dma, lba)?;

                let elapsed = before.elapsed();

                latencies.push(elapsed.as_nanos());

                in_loop += elapsed;
                ios += 1;

                if in_loop > duration / reps {
                    break;
                }
            }
            print!("I");
        }

        let cur_med = median(latencies.clone());
        println!("current median: {}", cur_med.unwrap());

        total += in_loop;
    }

    println!();

    let median = median(latencies).unwrap();

    println!(
        "ios: {ios}, iops: {}, avg latency: {}, median lat: {}, median accumulated: {}",
        ios as f64 / total.as_secs_f64(),
        total.as_nanos() / (ios) as u128,
        median,
        ios * median
    );
    // nvme.deallocate(dma)?;

    Ok(())
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
