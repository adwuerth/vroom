use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use std::{env, process, vec};
use vroom::QUEUE_LENGTH;
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

    let duration = match args.next() {
        Some(secs) => Duration::from_secs(
            secs.parse()
                .expect("Usage: cargo run --example init <pci bus id> <duration in seconds>"),
        ),
        None => process::exit(1),
    };

    let queue_depth = match args.next() {
        Some(arg) => arg.parse::<usize>().unwrap(),
        None => {
            eprintln!("no alloc size");
            process::exit(1);
        }
    };

    let thread_count = match args.next() {
        Some(arg) => arg.parse::<usize>().unwrap(),
        None => {
            eprintln!("no thread count");
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

    let mut nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let dma_mult = PAGESIZE_2MIB;

    let dma_size = alloc_size * dma_mult;

    println!("calling allocate with dma_size {dma_size}");

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    let mut qpairs = Vec::new();
    for _ in 0..thread_count {
        let qpair = nvme
            .lock()
            .unwrap()
            .create_io_queue_pair(QUEUE_LENGTH)
            .unwrap();
        qpairs.push(qpair);
    }

    for (i, mut qpair) in qpairs.into_iter().enumerate() {
        let nvme = Arc::clone(&nvme);

        let thread = thread::spawn(move || -> (u64, f64) {
            let mut dma = nvme.lock().unwrap().allocate::<u8>(dma_size).unwrap();
            let mut rng = rand::thread_rng();
            let mut lba = 0;
            let mut previous_dmas = vec![];
            let rand_block = &(0..dma_size)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            dma[0..dma_size].copy_from_slice(rand_block);
            let mut total = Duration::ZERO;
            let mut ios = 0;

            let mut outstanding_ops = 0;
            let unit_size = PAGESIZE_4KIB;

            for i in 0..dma_size / unit_size {
                previous_dmas.push(dma.slice(i * unit_size..(i * unit_size) + 1));
            }
            while total < duration {
                for previous_dma in &previous_dmas {
                    lba = if random {
                        rng.gen_range(0..ns_blocks)
                    } else {
                        (lba + 1) % ns_blocks
                    };
                    let before = Instant::now();
                    while qpair.quick_poll().is_some() {
                        outstanding_ops -= 1;
                        ios += 1;
                    }
                    if outstanding_ops == queue_depth {
                        qpair.complete_io(1);
                        outstanding_ops -= 1;
                        ios += 1;
                    }
                    qpair.submit_io(previous_dma, lba * blocks, write);
                    total += before.elapsed();
                    outstanding_ops += 1;
                    if total > duration {
                        break;
                    }
                }
            }
            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                total += before.elapsed();
            }
            ios += outstanding_ops as u64;
            assert!(qpair.sub_queue.is_empty());

            (ios, ios as f64 / total.as_secs_f64())
        });

        threads.push(thread);
    }
    let total = threads.into_iter().fold((0, 0.), |acc, thread| {
        let res = thread
            .join()
            .expect("The thread creation or execution failed!");
        (acc.0 + res.0, acc.1 + res.1)
    });

    write_throughput(
        &page_size,
        &duration,
        dma_size,
        queue_depth,
        thread_count,
        total.1,
    );

    println!(
        "n: {}, total {} iops: {:?}",
        total.0,
        if write { "write" } else { "read" },
        total.1
    );

    Ok(())
}

fn write_throughput(
    page_size: &Pagesize,
    duration: &Duration,
    dma_size: usize,
    queue_depth: usize,
    thread_count: usize,
    throughput: f64,
) {
    let fname = format!(
        "write_{}_qd{queue_depth}_t{thread_count}_{}s_{dma_size}alloc",
        page_size,
        duration.as_secs()
    );

    let mut file = File::create(fname).unwrap();

    writeln!(file, "{}", throughput).unwrap();
}
