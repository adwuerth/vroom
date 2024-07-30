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

    // let dma = nvme.allocate::<u8>(page_size.size())?;
    // nvme.write(&dma, 0)?;
    // nvme.deallocate(dma)?;

    // nvme.set_page_size(page_size.clone());
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let dma_mult = PAGESIZE_2MIB;

    let dma_size = alloc_size * dma_mult;

    let reps = 16;
    let duration = duration / reps;
    let mut latencies = vec![];
    let mut ios = 0;
    let mut total = Duration::ZERO;
    for _ in 0..reps {
        let mut in_loop = Duration::ZERO;
        println!("calling allocate with dma_size {dma_size}");
        let mut dma = nvme.allocate::<u8>(dma_size)?;
        println!("allocate done");
        // let rand_block = &(0..dma_size)
        //     .map(|_| rand::random::<u8>())
        //     .collect::<Vec<_>>()[..];
        // dma[0..dma_size].copy_from_slice(rand_block);

        let unit_mult = 1;

        let unit_size = PAGESIZE_4KIB * unit_mult;

        // for i in 0..dma_size / unit_size {
        //     previous_dmas.push(dma.slice(i * unit_size..(i + 1) * unit_size));
        // }
        for i in 0..dma_size / unit_size {
            previous_dmas.push(dma.slice(i * unit_size..((i + 1) * unit_size)));
        }

        // previous_dmas.shuffle(&mut thread_rng());

        // println!(
        //     "write prp starting, previous_dmas len {}",
        //     previous_dmas.len()
        // );
        let mut prp_list_chunks = vec![];
        for prevdma in &previous_dmas {
            prp_list_chunks.push(prevdma.phys)
        }

        while total < duration {
            // prp_list_chunks.shuffle(&mut thread_rng());
            let mut prp_list_chunks = prp_list_chunks.chunks(128);
            for chunk in prp_list_chunks {
                lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + 1) % ns_blocks
                };

                // let dma_u8_slice =
                //     unsafe { std::slice::from_raw_parts(previous_dma.virt, previous_dma.size) };

                // let elapsed = nvme.write_prp(previous_dma, lba * blocks, true)?;

                let elapsed = nvme.write_prp_raw(Vec::from(chunk), lba * blocks, true)?;

                // let start = Instant::now();
                // nvme.read(previous_dma, lba * blocks)?;
                // let elapsed = start.elapsed();

                total += elapsed;
                ios += 1;

                latencies.push(elapsed.as_nanos());

                if total > duration {
                    break;
                }
            }
        }

        total += in_loop;
    }

    let median = median(latencies.clone()).unwrap();

    println!(
        "total: {}, median: {}",
        ios as f64 / total.as_secs_f64(),
        median
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
