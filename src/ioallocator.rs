use crate::init_with_page_size;
use crate::memory::{Dma, Pagesize, DEFAULT_PAGE_SIZE};
use crate::mmio::Mmio;
use crate::vfio::Vfio;
use std::error::Error;

pub trait Allocating {
    /// Allocate Dma<T> with size
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>>;

    /// Deallocate Dma<T>
    fn deallocate<T>(&self, dma: Dma<T>) -> Result<(), Box<dyn Error>>;

    /// Map Resource/Region
    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>>;
}

/// `IOAllocators` UIO and VFIO, is necessary such that trait Allocating can be used as a object
pub enum IOAllocator {
    MmioAllocator(Mmio),
    VfioAllocator(Vfio),
}

impl IOAllocator {
    /// Returns either UIO or VFIO, depending on if vfio is enabled
    pub fn init(pci_addr: &str) -> Result<Self, Box<dyn Error>> {
        Self::init_with_page_size(pci_addr, DEFAULT_PAGE_SIZE)
    }

    pub fn init_with_page_size(
        pci_addr: &str,
        page_size: Pagesize,
    ) -> Result<Self, Box<dyn Error>> {
        Ok(if Vfio::is_enabled(pci_addr) {
            println!("initializing Vfio");
            Self::VfioAllocator(Vfio::init_with_args(pci_addr, page_size, false)?)
        } else {
            println!("initializing Mmio");
            if unsafe { libc::getuid() } != 0 {
                println!("not running as root, this will probably fail");
            }
            Self::MmioAllocator(Mmio::init(pci_addr)?)
        })
    }

    pub fn set_page_size(&mut self, page_size: Pagesize) {
        if let Self::VfioAllocator(vfio) = self {
            vfio.set_page_size(page_size);
        }
    }
}

impl Allocating for IOAllocator {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        match self {
            Self::MmioAllocator(mmio) => mmio.allocate(size),
            Self::VfioAllocator(vfio) => vfio.allocate(size),
        }
    }

    fn deallocate<T>(&self, dma: Dma<T>) -> Result<(), Box<dyn Error>> {
        match self {
            Self::MmioAllocator(mmio) => mmio.deallocate(dma),
            Self::VfioAllocator(vfio) => vfio.deallocate(dma),
        }
    }

    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>> {
        match self {
            Self::MmioAllocator(mmio) => mmio.map_resource(),
            Self::VfioAllocator(vfio) => vfio.map_resource(),
        }
    }
}
