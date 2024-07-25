use rand::{thread_rng, Rng};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, process, thread};
use vroom::memory::*;
use vroom::vfio::Vfio;
use vroom::Mapping;
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

    #[allow(clippy::manual_map)]
    let duration = match args.next() {
        Some(secs) => Some(Duration::from_secs(secs.parse().expect(
            "Usage: cargo run --example init <pci bus id> <duration in seconds>",
        ))),
        None => None,
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

    let duration = duration.unwrap();

    let mut nvme = vroom::init(&pci_addr)?;

    nvme.set_page_size(page_size.clone());

    let random = true;
    let write = true;

    // fill_ns(&mut nvme);

    // let nvme = qd_n_multithread(nvme, 32, 4, duration, random, write);

    let dma = nvme.allocate::<u8>(PAGESIZE_2MIB)?;

    let mut qpair = nvme.create_io_queue_pair(QUEUE_LENGTH)?;

    qpair.submit_io(&dma, 0, true);

    qpair.complete_io(1);

    Ok(())
}
