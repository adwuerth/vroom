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
use vroom::NvmeDevice;
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

    let mut is_mmio = false;
    let page_size = match page_size.as_str() {
        "4k" => Pagesize::Page4K,
        "2m" => Pagesize::Page2M,
        "1g" => Pagesize::Page1G,
        "mmio2m" => {
            is_mmio = true;
            Pagesize::Page2M
        }
        "mmio1g" => {
            is_mmio = true;
            Pagesize::Page1G
        }
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

    // Pre-run
    // {
    //     let (nvme_res, _) = pagesizemedians(nvme, &page_size, 128, random)?;
    //     nvme = nvme_res;
    // }
    // nvme.format_namespace(Some(1));

    let mut alloc_size = 1;

    let mut max_alloc = 2048;
    if page_size == Pagesize::Page1G {
        max_alloc = 192;
    }
    let mut data = vec![];

    while alloc_size <= max_alloc {
        let median = {
            let (nvme_res, median_res) = pagesizemedians(nvme, &page_size, alloc_size, random)?;
            nvme = nvme_res;
            median_res
        };
        data.push((alloc_size, median));
        println!("formatting");
        nvme.format_namespace(Some(1));
        println!("formatting done");
        alloc_size *= 2;
    }

    if page_size == Pagesize::Page1G {
        let median = {
            let (nvme_res, median_res) = pagesizemedians(nvme, &page_size, 192, random)?;
            nvme = nvme_res;
            median_res
        };
        data.push((192, median));
        nvme.format_namespace(Some(1));
    }

    let mut file = File::create(format!(
        "pagesizemedians_{}_{}.txt",
        if is_mmio { "mmio" } else { "vfio" },
        page_size
    ))?;
    writeln!(file, "pages,median")?;
    for entry in data {
        writeln!(file, "{},{}", entry.0, entry.1)?;
    }

    // write_nanos_to_file(latencies, write, &page_size, alloc_size, false, "pages")?;
    Ok(())
}

fn pagesizemedians(
    mut nvme: NvmeDevice,
    page_size: &Pagesize,
    alloc_size: usize,
    random: bool,
) -> Result<(NvmeDevice, u128), Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;
    let mut rng = rand::thread_rng();
    let mut lba = 0;
    let mut previous_dmas = vec![];

    let split_size = 1;

    let dma_mult = page_size.size();

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

            latencies.push(elapsed.as_nanos());
        }
    }

    println!("test done");

    let median = median(latencies.clone()).unwrap();

    println!("now deallocating");
    nvme.deallocate(&dma)?;
    println!("dealloc done");
    println!(
        "total: {}, median: {}",
        total.as_micros() / previous_dmas.len() as u128,
        median
    );

    Ok((nvme, median))
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
