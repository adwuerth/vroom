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

    let lba = 0;

    let mut nvme = vroom::init(&pci_addr)?;

    let bytes: &[u8] = "hello world! vroom test bytes".as_bytes();
    let mut buffer: Dma<u8> = Dma::allocate_nvme(HUGE_PAGE_SIZE, &nvme)?;
    buffer[..bytes.len()].copy_from_slice(bytes);

    nvme.write(&buffer, lba)?;

    buffer[..bytes.len()].fill_with(Default::default);

    nvme.read(&buffer, lba)?;
    let read_buf = &buffer[0..bytes.len()];

    assert_eq!(bytes, read_buf);
    Ok(())
}
