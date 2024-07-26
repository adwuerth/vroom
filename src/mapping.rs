use crate::memory::{Dma, Pagesize, DEFAULT_PAGE_SIZE};
use crate::mmio::Mmio;
use crate::vfio::Vfio;
use crate::Result;
pub trait Mapping {
    /// Allocate memory in host memory and map it for use for `NVMe`
    /// # Errors
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>>;

    /// Deallocate memory in host memory and unmap it
    /// # Errors
    fn deallocate<T>(&self, dma: Dma<T>) -> Result<()>;

    /// Map a device region into host memory
    /// # Errors
    fn map_resource(&self) -> Result<(*mut u8, usize)>;
}

/// `IOAllocators` UIO and VFIO, is necessary such that trait Allocating can be used as a object
pub enum MemoryMapping {
    Mmio(Mmio),
    Vfio(Vfio),
}

impl MemoryMapping {
    /// Returns either UIO or VFIO, depending on if vfio is enabled
    /// # Errors
    pub fn init(pci_addr: &str) -> Result<Self> {
        Self::init_with_page_size(pci_addr, DEFAULT_PAGE_SIZE)
    }

    /// Initialize Vfio with given Pagesize, if Mmio, use default 2MiB hugepages
    /// # Errors
    pub fn init_with_page_size(pci_addr: &str, page_size: Pagesize) -> Result<Self> {
        Ok(if Vfio::is_enabled(pci_addr) {
            println!("initializing Vfio");
            Self::Vfio(Vfio::init_with_args(pci_addr, page_size, false)?)
        } else {
            println!("initializing Mmio");
            if unsafe { libc::getuid() } != 0 {
                println!("not running as root, this will probably fail");
            }
            Self::Mmio(Mmio::init(pci_addr)?)
        })
    }

    pub fn set_page_size(&mut self, page_size: Pagesize) {
        if let Self::Vfio(vfio) = self {
            vfio.set_page_size(page_size);
        }
    }
}

impl Mapping for MemoryMapping {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>> {
        match self {
            Self::Mmio(mmio) => mmio.allocate(size),
            Self::Vfio(vfio) => vfio.allocate(size),
        }
    }

    fn deallocate<T>(&self, dma: Dma<T>) -> Result<()> {
        match self {
            Self::Mmio(mmio) => mmio.deallocate(dma),
            Self::Vfio(vfio) => vfio.deallocate(dma),
        }
    }

    fn map_resource(&self) -> Result<(*mut u8, usize)> {
        match self {
            Self::Mmio(mmio) => mmio.map_resource(),
            Self::Vfio(vfio) => vfio.map_resource(),
        }
    }
}
