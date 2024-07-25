use std::{env, process};
use vroom::memory::Dma;
use vroom::{self, Mapping, NvmeDevice};

pub fn get_pci_addr() -> String {
    env::var("NVME_ADDR").unwrap_or_else(|_| {
        eprintln!("Please set the NVME_ADDR environment variable.");
        process::exit(1);
    })
}

pub fn init_nvme(pci_addr: &str) -> NvmeDevice {
    vroom::init(pci_addr).unwrap_or_else(|e| {
        eprintln!("Initialization failed: {}", e);
        process::exit(1);
    })
}

pub fn allocate_dma_buffer(nvme: &NvmeDevice, size: usize) -> Dma<u8> {
    nvme.allocate::<u8>(size).unwrap_or_else(|e| {
        eprintln!("DMA allocation failed: {}", e);
        process::exit(1);
    })
}

pub fn nvme_write(nvme: &mut NvmeDevice, buffer: &Dma<u8>, lba: u64) {
    nvme.write(buffer, lba).unwrap_or_else(|e| {
        eprintln!("NVMe write failed: {}", e);
        process::exit(1);
    });
}

pub fn nvme_read(nvme: &mut NvmeDevice, buffer: &mut Dma<u8>, lba: u64) {
    nvme.read(buffer, lba).unwrap_or_else(|e| {
        eprintln!("NVMe read failed: {}", e);
        process::exit(1);
    });
}
