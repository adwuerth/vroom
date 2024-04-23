use std::error::Error;
use std::{env, process};

use vroom::memory::Dma;
use vroom::HUGE_PAGE_SIZE;

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

    nvme.identify_controller()?;

    // Logical Block Adress
    // let lba = 0;

    // let buffer: Dma<u8> = Dma::allocate(HUGE_PAGE_SIZE)?;

    // nvme.write(&buffer, lba)?;

    Ok(())
}
