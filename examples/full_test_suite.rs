use rand::Rng;
use std::io::{self, Write};
use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::File,
    process,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use vroom::{
    memory::{DmaSlice, Pagesize},
    vfio::Vfio,
    Mapping, NvmeDevice, PAGESIZE_4KIB, QUEUE_LENGTH,
};

use lazy_static::lazy_static;

#[derive(Debug, Default, Clone)]
struct TestResult {
    qd1tn: HashMap<usize, Vec<TestOperation>>,
    psmedians: HashMap<usize, Vec<TestOperation>>,
}

#[derive(Debug, Default, Clone)]
struct TestOperation {
    ios: u64,
    iops: f64,
    latencies: Vec<u128>,
}
lazy_static! {
    static ref TEST_RESULT: Arc<Mutex<TestResult>> = Arc::new(Mutex::new(TestResult::default()));
}
pub fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    args.next();

    let page_size = match args.next() {
        Some(arg) => Pagesize::from_str(&arg)?,
        None => {
            eprintln!("Usage: cargo run --example full_test_suite <page size>");
            process::exit(1);
        }
    };

    let nvme_addresses = [
        "0000:01:00.0",
        // "0000:02:00.0",
        // "0000:03:00.0",
        // "0000:04:00.0",
        // "0000:21:00.0",
        // "0000:22:00.0",
        // "0000:23:00.0",
        // "0000:24:00.0",
    ];

    for nvme_address in nvme_addresses {
        vroom::check_nvme(nvme_address);
    }

    let mut threads = vec![];

    for &nvme_address in &nvme_addresses {
        let nvme_address = nvme_address.to_string();
        let page_size = page_size.clone();
        threads.push(thread::spawn(move || {
            single_nvme(&nvme_address, page_size).unwrap();
        }));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    let is_vfio = Vfio::is_enabled(nvme_addresses[0]);

    write_result_to_csv(&page_size, is_vfio)?;

    Ok(())
}

fn write_result_to_csv(page_size: &Pagesize, is_vfio: bool) -> io::Result<()> {
    let mut map = TEST_RESULT.lock().unwrap().psmedians.clone();
    let mut fname = "psmeds";
    if map.is_empty() {
        map = TEST_RESULT.lock().unwrap().qd1tn.clone();
        fname = "qd1tn";
    }

    let mode = if is_vfio { "vfio" } else { "mmio" };

    let mut file = File::create(format!("{fname}_{page_size}_{mode}.csv"))?;
    writeln!(file, "key,ios,iops,median")?;

    // Collect and sort the keys
    let mut keys: Vec<&usize> = map.keys().collect();
    keys.sort();

    for &key in &keys {
        let operations = &map[key];
        let sum_ios: u64 = operations.iter().map(|op| op.ios).sum();
        let avg_iops: f64 =
            operations.iter().map(|op| op.iops).sum::<f64>() / operations.len() as f64;
        let mut all_latencies: Vec<u128> = operations
            .iter()
            .flat_map(|op| op.latencies.clone())
            .collect();

        // Compute the median of latencies
        let median = if all_latencies.is_empty() {
            0
        } else {
            all_latencies.sort();
            let mid = all_latencies.len() / 2;
            if all_latencies.len() % 2 == 0 {
                (all_latencies[mid - 1] + all_latencies[mid]) / 2
            } else {
                all_latencies[mid]
            }
        };

        // Write the computed values to the file
        writeln!(file, "{},{},{},{}", key, sum_ios, avg_iops, median)?;
    }

    Ok(())
}

fn single_nvme(pci_addr: &str, page_size: Pagesize) -> Result<(), Box<dyn Error>> {
    let nvme = vroom::init_with_page_size(pci_addr, page_size.clone())?;

    // let nvme = qd1tn(nvme)?;

    let nvme = psmeds(nvme, &page_size);

    Ok(())
}

fn psmeds(mut nvme: NvmeDevice, page_size: &Pagesize) -> Result<NvmeDevice, Box<dyn Error>> {
    let mut max_alloc = 2048;
    let mut alloc_size = 8;
    if page_size == &Pagesize::Page1G {
        max_alloc = 256;
        // max_alloc = 192;
    }

    while alloc_size <= max_alloc {
        println!("running psmeds {alloc_size}");
        let test_op = {
            let (nvme_res, test_res) = pagesizemedians(nvme, &page_size, alloc_size, true)?;
            nvme = nvme_res;
            test_res
        };
        println!("formatting");
        nvme.format_namespace(Some(1));
        println!("formatting done");

        TEST_RESULT
            .lock()
            .unwrap()
            .psmedians
            .entry(alloc_size)
            .or_insert_with(Vec::new)
            .push(test_op);

        // if alloc_size != max_alloc && alloc_size * 2 > max_alloc && page_size == &Pagesize::Page1G {
        //     alloc_size = max_alloc;
        //     continue;
        // }
        alloc_size *= 2;
    }

    Ok(nvme)
}

fn qd1tn(mut nvme: NvmeDevice) -> Result<NvmeDevice, Box<dyn Error>> {
    let mut pages: usize = 1;
    while pages <= 64 {
        let test_op = {
            println!("starting qd1tn {pages}");
            let (nvme_result, test_op_result) = qd_n_multithread(
                nvme,
                1,
                pages as u64,
                Duration::from_secs(10),
                true,
                true,
                512,
            )?;
            nvme = nvme_result;
            test_op_result
        };

        println!("formatting ns 1");
        nvme.format_namespace(Some(1));

        TEST_RESULT
            .lock()
            .unwrap()
            .qd1tn
            .entry(pages)
            .or_insert_with(Vec::new)
            .push(test_op);

        pages *= 2;
    }

    // {
    //     let (nvme_result, _) =
    //         qd_n_multithread(nvme, 1, 1, Duration::from_secs(30), true, true, 512)?;
    //     nvme = nvme_result;
    // }

    // println!("formatting ns 1");
    // nvme.format_namespace(Some(1));
    Ok(nvme)
}

fn pagesizemedians(
    mut nvme: NvmeDevice,
    page_size: &Pagesize,
    alloc_size: usize,
    random: bool,
) -> Result<(NvmeDevice, TestOperation), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let split_size = 1;

    let dma_mult = page_size.size();

    // let dma_mult = PAGESIZE_4KIB;
    let dma_size = alloc_size * dma_mult;

    println!("allocating");
    let mut dma = nvme.allocate::<u8>(dma_size)?;
    println!("allocate done");

    // let rand_block = &(0..dma_size)
    //     .map(|_| rand::random::<u8>())
    //     .collect::<Vec<_>>()[..];
    // dma[0..dma_size].copy_from_slice(rand_block);

    for i in 0..alloc_size {
        let rand_block = &(i * dma_mult..(i * dma_mult) + PAGESIZE_4KIB)
            .map(|_| rand::random::<u8>())
            .collect::<Vec<_>>()[..];
        dma[i * dma_mult..(i * dma_mult) + PAGESIZE_4KIB].copy_from_slice(rand_block);
        previous_dmas.push(dma.slice(i * dma_mult..(i * dma_mult) + split_size));
    }

    println!("now running test");

    let mut total = Duration::ZERO;
    let mut ios = 0;

    let mut latencies: Vec<u128> = vec![];
    for _i in 0..512 {
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
        }
    }

    println!("test done");

    let median = median(&mut latencies).unwrap();

    println!("now deallocating");
    nvme.deallocate(&dma)?;
    println!("dealloc done");
    println!(
        "total: {}, median: {}",
        total.as_micros() / previous_dmas.len() as u128,
        median
    );

    let test_operation = TestOperation {
        ios,
        iops: ios as f64 / total.as_secs_f64(),
        latencies,
    };

    Ok((nvme, test_operation))
}

fn qd_n_multithread(
    nvme: NvmeDevice,
    queue_depth: usize,
    thread_count: u64,
    duration: Duration,
    random: bool,
    write: bool,
    ps4k_alloc: usize,
) -> Result<(NvmeDevice, TestOperation), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks;

    let nvme = Arc::new(Mutex::new(nvme));
    let mut threads = Vec::new();

    for thread_num in 0..thread_count {
        let nvme = Arc::clone(&nvme);

        let handle = thread::spawn(move || -> (u64, f64, Vec<u128>) {
            let mut rng = rand::thread_rng();
            let bytes = 512 * blocks as usize; // 4kib
            let mut total = std::time::Duration::ZERO;

            let buffer_size = PAGESIZE_4KIB * ps4k_alloc;

            let mut buffer = nvme.lock().unwrap().allocate::<u8>(buffer_size).unwrap();

            let mut qpair = nvme
                .lock()
                .unwrap()
                .create_io_queue_pair(QUEUE_LENGTH)
                .unwrap();

            let rand_block = &(0..buffer_size)
                .map(|_| rand::random::<u8>())
                .collect::<Vec<_>>()[..];
            buffer[0..buffer_size].copy_from_slice(rand_block);

            let mut buffer_slices = vec![];
            for i in 0..ps4k_alloc {
                buffer_slices.push(buffer.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB));
            }
            // buffer_slices.shuffle(&mut thread_rng());
            let mut buffer_slices_it = buffer_slices.iter().cycle();

            let mut outstanding_ops = 0;
            let mut total_io_ops = 0;
            let lba = thread_num;

            let mut latencies = vec![];

            while total < duration {
                // let lba = rng.gen_range(range.0..range.1);
                let lba = if random {
                    rng.gen_range(0..ns_blocks)
                } else {
                    (lba + thread_count) % ns_blocks
                };
                let before = Instant::now();

                while qpair.quick_poll().is_some() {
                    outstanding_ops -= 1;
                    total_io_ops += 1;
                }
                if outstanding_ops == queue_depth {
                    qpair.complete_io(1);
                    outstanding_ops -= 1;
                    total_io_ops += 1;
                }
                qpair.submit_io(buffer_slices_it.next().unwrap(), lba * blocks, write);

                let elapsed = before.elapsed();
                total += elapsed;
                outstanding_ops += 1;
                latencies.push(elapsed.as_nanos());
            }
            if outstanding_ops != 0 {
                let before = Instant::now();
                qpair.complete_io(outstanding_ops);
                total += before.elapsed();
            }
            total_io_ops += outstanding_ops as u64;
            assert!(qpair.sub_queue.is_empty());
            nvme.lock().unwrap().delete_io_queue_pair(&qpair).unwrap();
            nvme.lock().unwrap().deallocate(&buffer).unwrap();

            (
                total_io_ops,
                total_io_ops as f64 / total.as_secs_f64(),
                latencies,
            )
        });
        threads.push(handle);
    }

    let (ios, iops, mut latencies) = threads.into_iter().fold(
        (0, 0., vec![]),
        |mut acc: (u64, f64, Vec<u128>), thread: thread::JoinHandle<(u64, f64, Vec<u128>)>| {
            let res = thread
                .join()
                .expect("The thread creation or execution failed!");
            acc.2.extend(res.2);
            (acc.0 + res.0, acc.1 + res.1, acc.2)
        },
    );

    let median = median(&mut latencies).unwrap();
    let average = average(&latencies).unwrap();
    println!("median: {median}");
    println!("average: {average}");

    println!(
        "n: {}, total {} iops: {:?}",
        ios,
        if write { "write" } else { "read" },
        iops
    );

    let test_operation = TestOperation {
        ios,
        iops,
        latencies,
    };

    match Arc::try_unwrap(nvme) {
        Ok(mutex) => match mutex.into_inner() {
            Ok(t) => Ok((t, test_operation)),
            Err(e) => Err(e.into()),
        },
        Err(_) => Err("Arc::try_unwrap failed, not the last reference.".into()),
    }
}
fn median(latencies: &mut [u128]) -> Option<u128> {
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

fn average(latencies: &[u128]) -> Option<f64> {
    let len = latencies.len();
    if len == 0 {
        return None;
    }
    let sum: u128 = latencies.iter().sum();
    Some(sum as f64 / len as f64)
}
