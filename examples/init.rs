use rand::{thread_rng, Rng};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::{env, process, thread};
use vroom::memory::*;
use vroom::{NvmeDevice, QUEUE_LENGTH};

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
    println!("init done");
    // println!("test");
    // let mut nvme = qd32_test(nvme, true)?;
    // let mut nvme = qd32_test(nvme, false)?;
    // let mut nvme = qd32(nvme)?;

    // TODO: make time based?
    // rnadom read/write qd1
    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks - blocks - 1;

    let mut buffer: Dma<u8> = Dma::allocate(HUGE_PAGE_SIZE)?;
    println!("allocate done");

    let n = 100_000;
    let mut read = std::time::Duration::new(0, 0);
    let mut write = std::time::Duration::new(0, 0);

    let mut rng = thread_rng();
    let seq = &(0..n)
        .map(|_| rng.gen_range(0..ns_blocks as u64))
        .collect::<Vec<u64>>()[..];
    let mut ctr = 0;
    for &lba in seq {
        if ctr % (n / 10) == 0 {
            println!("passed iteration {ctr}");
        }
        ctr += 1;
        let rand_block = &(0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()[..];

        buffer[..rand_block.len()].copy_from_slice(rand_block);

        // write
        let before = std::time::Instant::now();
        nvme.write(&buffer.slice(0..bytes as usize), lba)?;
        write += before.elapsed();

        buffer[..rand_block.len()].fill_with(Default::default);

        let before = std::time::Instant::now();
        nvme.read(&buffer.slice(0..bytes as usize), lba)?;
        read += before.elapsed();

        assert_eq!(&buffer[0..rand_block.len()], rand_block);
        // lba += blocks as u64;
    }
    println!("total completions: {}", nvme.stats.completions);
    println!("total submissions: {}", nvme.stats.submissions);
    println!(
        "read iops: {}, write iops: {}",
        (n as f64) / read.as_secs_f64(),
        (n as f64) / write.as_secs_f64()
    );
    println!(
        "read time: {:?}; write time: {:?}; total: {:?}",
        read,
        write,
        read + write
    );
    // let nvme = qd1(nvme)?;

    Ok(())
}

#[allow(unused)]
fn qd32(mut nvme: NvmeDevice) -> Result<NvmeDevice, Box<dyn Error>> {
    let n = 100_000;
    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / 4; // - blocks - 1

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    let before = std::time::Instant::now();
    for i in 0..4 {
        // let max_lba = ns_blocks.clone();
        let mut nvme = Arc::clone(&nvme);
        let range = (i * ns_blocks..((i + 1) * ns_blocks - blocks - 1));

        use std::time::Duration;
        let handle = thread::spawn(move || -> (Duration, Duration) {
            let mut rng = rand::thread_rng();
            let seq = &(0..n)
                .map(|_| rng.gen_range(range.clone()))
                .collect::<Vec<u64>>()[..];

            let blocks = 8;
            let bytes = 512 * blocks;

            let mut read = std::time::Duration::ZERO;
            let mut write = std::time::Duration::ZERO;
            let mut buffer: Dma<u8> = Dma::allocate(HUGE_PAGE_SIZE).unwrap();

            // buggy when completely saturating queue for some reason
            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            // TODO
            for lba in seq.chunks(32) {
                let rand_block = &(0..(32 * bytes))
                    .map(|_| rand::random::<u8>())
                    .collect::<Vec<_>>()[..];
                buffer[..rand_block.len()].copy_from_slice(rand_block);

                let before = std::time::Instant::now();
                for (idx, &lba) in lba.iter().enumerate() {
                    qpair.submit_io(&buffer.slice((idx * bytes)..(idx + 1) * bytes), lba, true);
                }

                if let Some(head) = qpair.complete_io(lba.len()) {
                    qpair.sub_queue.head = head as usize;
                } else {
                    eprintln!("shit");
                    continue;
                }
                write += before.elapsed();

                buffer[..rand_block.len()].fill_with(Default::default);
                let before = std::time::Instant::now();
                for (idx, &lba) in lba.iter().enumerate() {
                    qpair.submit_io(&buffer.slice((idx * bytes)..(idx + 1) * bytes), lba, false);
                }

                if let Some(head) = qpair.complete_io(lba.len()) {
                    qpair.sub_queue.head = head as usize;
                } else {
                    eprintln!("shit");
                    continue;
                }
                read += before.elapsed();
            }
            assert!(qpair.sub_queue.is_empty());

            (read, write)
        });
        threads.push(handle);
    }

    let total = threads.into_iter().fold((0., 0.), |acc, thread| {
        let res = thread
            .join()
            .expect("The thread creation or execution failed!");
        println!("elapsed: {:?}", res);
        (
            acc.0 + (n as f64) / res.0.as_secs_f64(),
            acc.1 + (n as f64) / res.1.as_secs_f64(),
        )
    });
    println!("n: {n}, total iops (read, write): {:?}", total);

    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok(t),
            Err(e) => Err(e.into()),
        },
        Err(arc_mutex_t_again) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}

#[allow(unused)]
fn qd32_test(mut nvme: NvmeDevice, write: bool) -> Result<NvmeDevice, Box<dyn Error>> {
    let n = 100_000;
    let blocks = 8;
    let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / 4; // - blocks - 1

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    let before = std::time::Instant::now();
    for i in 0..4 {
        // let max_lba = ns_blocks.clone();
        let mut nvme = Arc::clone(&nvme);
        let range = (i * ns_blocks..((i + 1) * ns_blocks - blocks - 1));

        use std::time::{Duration, Instant};
        let handle = thread::spawn(move || -> Duration {
            let qd = 32;
            let mut rng = rand::thread_rng();
            let seq = &(0..n)
                .map(|_| rng.gen_range(range.clone()))
                .collect::<Vec<u64>>()[..];

            let blocks = 8;
            let bytes = 512 * blocks;

            let mut elapsed = std::time::Duration::ZERO;
            let mut buffer: Dma<u8> = Dma::allocate(HUGE_PAGE_SIZE).unwrap();

            // buggy when completely saturating queue for some reason
            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            // TODO
            let mut ctr = 0;
            for &lba in seq {
                let before = Instant::now();
                let mut other_shit = Duration::ZERO;
                if write {
                    let rand_block = &(0..(bytes))
                        .map(|_| rand::random::<u8>())
                        .collect::<Vec<_>>()[..];
                    buffer[..bytes].copy_from_slice(rand_block);
                } else {
                    buffer[..bytes].fill_with(Default::default);
                }
                other_shit += before.elapsed();
                if ctr == 32 {
                    qpair.complete_io(1);
                    ctr -= 1;
                }
                if let Some(_) = qpair.quick_poll() {
                    ctr -= 1;
                }
                qpair.submit_io(&buffer.slice((ctr * bytes)..(ctr + 1) * bytes), lba, false);
                elapsed += before.elapsed();
                elapsed -= other_shit;
                ctr += 1;
            }
            qpair.complete_io(ctr);
            assert!(qpair.sub_queue.is_empty());
            elapsed
        });
        threads.push(handle);
    }

    let total = threads.into_iter().fold(0., |acc, thread| {
        let res = thread
            .join()
            .expect("The thread creation or execution failed!");
        println!("elapsed: {:?}", res);
        acc + (n as f64) / res.as_secs_f64()
    });
    println!(
        "n: {n}, total {} iops: {:?}",
        if write { "write" } else { "read" },
        total
    );

    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok(t),
            Err(e) => Err(e.into()),
        },
        Err(arc_mutex_t_again) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}
