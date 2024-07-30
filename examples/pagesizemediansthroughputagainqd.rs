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

    let nvme = vroom::init_with_page_size(&pci_addr, page_size.clone())?;

    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let dma_mult = PAGESIZE_4KIB;

    let dma_size = alloc_size * dma_mult;

    let nvme = Arc::new(Mutex::new(nvme));

    let latencies = Arc::new(Mutex::new(vec![]));

    let reqs = 16;

    let duration = duration / reqs;

    let mut total = 0;
    let mut iops_vec: Vec<f64> = vec![];

    for _ in 0..reqs {
        let mut qpairs = Vec::new();
        for _ in 0..thread_count {
            let qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(vroom::QUEUE_LENGTH)
                .unwrap();
            qpairs.push(qpair);
        }
        let mut threads = Vec::new();
        for (_i, mut qpair) in qpairs.into_iter().enumerate() {
            let nvme = Arc::clone(&nvme);
            let latencies = Arc::clone(&latencies);

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

                        let elapsed = before.elapsed();
                        total += elapsed;
                        latencies.lock().unwrap().push(elapsed.as_nanos());
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

                nvme.lock().unwrap().deallocate(dma).unwrap();

                (ios, ios as f64 / total.as_secs_f64())
            });

            threads.push(thread);
        }

        let temp_total = threads.into_iter().fold((0, 0.), |acc, thread| {
            let res = thread
                .join()
                .expect("The thread creation or execution failed!");
            (acc.0 + res.0, acc.1 + res.1)
        });

        total += temp_total.0;
        iops_vec.push(temp_total.1);
    }

    println!("alloc done");

    let median = median(latencies.lock().unwrap().clone()).unwrap();

    let average = average(latencies.lock().unwrap().clone()).unwrap();

    println!(
        "n: {}, total {} iops: {:?} median {} average {} latlen {}",
        total,
        if write { "write" } else { "read" },
        iops_vec.iter().sum::<f64>() / iops_vec.len() as f64,
        median,
        average,
        latencies.lock().unwrap().len()
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

fn average(latencies: Vec<u128>) -> Option<f64> {
    let len = latencies.len();
    if len == 0 {
        return None;
    }
    let sum: u128 = latencies.iter().sum();
    Some(sum as f64 / len as f64)
}
