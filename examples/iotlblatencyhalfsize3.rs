use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::time::Instant;
use std::{env, process, vec};
use vroom::vfio::Vfio;
use vroom::NvmeDevice;
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

    let do_dealloc = match args.next() {
        Some(arg) => arg == "dealloc",
        None => false,
    };

    let alloc_size_given = match args.next() {
        Some(arg) => Some(arg.parse::<usize>().unwrap()),
        None => None,
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

    // let mut nvme = vroom::init(&pci_addr)?;

    // CONFIG
    let random = true;
    let write = true;
    const ALLOC_SIZE: usize = 4096 * 2 * 2 * 2 * 2 * 2;
    //CURRENTLY ONLY SUPPORTS 4KIB
    const DMA_SIZE: usize = PAGESIZE_2MIB;
    const SAME: bool = false;
    const ALWAYS_SAME_DMA: bool = false;
    const SKIP_LATENCIES: usize = 0;
    const THRESHOLD: u128 = 10000;
    const SHUFFLE_HALFWAY: bool = false;
    const SHUFFLE_FIRST: bool = false;
    const SHUFFLE_ONCE: bool = false;
    let mut nvme = vroom::init(&pci_addr)?;
    nvme.set_page_size(page_size.clone());
    let mut nvme = pre_run(nvme)?;
    // let mut n = 4096 * 2 * 2 * 2 * 2 * 2;
    nvme = inv_iotlb(nvme, page_size.clone())?;

    let blocks = 8;
    // let bytes = 512 * blocks;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let mut lba = 0;

    let mut previous_dmas = vec![];

    let split_size = PAGESIZE_4KIB;

    let dma_size = ALLOC_SIZE * split_size;

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

        if let Some(l) = alloc_size_given {
            n = l;
        }
        let mut half1: Vec<u128> = vec![];
        let mut half2: Vec<u128> = vec![];
        let mut half3: Vec<u128> = vec![];

        if SHUFFLE_FIRST {
            previous_dmas.shuffle(&mut thread_rng());
        }

        // let same_dma = &previous_dmas[0];
        // for previous_dma in &previous_dmas[0..n] {
        //     lba = if random {
        //         rng.gen_range(0..ns_blocks)
        //     } else {
        //         (lba + 1) % ns_blocks
        //     };

        //     if write {
        //         nvme.write(same_dma, lba * blocks)?;
        //     } else {
        //         nvme.read(same_dma, lba * blocks)?;
        //     }
        // }

        // let dma_arr: &[Dma<u8>] = &previous_dmas[previous_ctr..previous_ctr + n];

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

        // if do_dealloc {
        //     nvme.deallocate(dma)?;
        // }

        // nvme = inv_iotlb(nvme, page_size.clone())?;

        previous_ctr += n;

        if alloc_size_given.is_some() {
            return Ok(());
        }
    }
    Ok(())
}

fn pre_run(mut nvme: NvmeDevice) -> Result<NvmeDevice, Box<dyn Error>> {
    let blocks = 8;
    let ns_blocks = nvme.namespaces.get(&1).unwrap().blocks / blocks - 1;

    let mut rng = rand::thread_rng();

    let mut lba;

    let mut previous_dmas = vec![];

    let n = 2048;

    let dma = nvme.allocate::<u8>(n * PAGESIZE_4KIB)?;
    for i in 0..n {
        previous_dmas.push(dma.slice(i * PAGESIZE_4KIB..(i + 1) * PAGESIZE_4KIB));
    }

    for previous_dma in &previous_dmas {
        lba = rng.gen_range(0..ns_blocks);

        nvme.write(previous_dma, lba * blocks)?;
    }

    nvme.deallocate(dma)?;

    Ok(nvme)
}

fn inv_iotlb(mut nvme: NvmeDevice, page_size: Pagesize) -> Result<NvmeDevice, Box<dyn Error>> {
    nvme.set_page_size(Pagesize::Page4K);
    let dma = nvme.allocate::<u8>(PAGESIZE_4KIB)?;
    nvme.deallocate(dma)?;
    nvme.set_page_size(page_size);
    Ok(nvme)
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
