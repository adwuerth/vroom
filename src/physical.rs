use crate::mapping::Mapping;
use crate::memory::{self, Dma, Pagesize};
use crate::pci::{
    read_io16, write_io16, BUS_MASTER_ENABLE_BIT, COMMAND_REGISTER_OFFSET, INTERRUPT_DISABLE,
};
use std::io::Write;
use std::io::{Read, Seek};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{default, fs, io, mem, process, ptr};

static HUGEPAGE_ID: AtomicUsize = AtomicUsize::new(0);

use crate::{mlock_unsafe, mmap_fd_unsafe, munlock_unsafe, munmap_unsafe, Result};
use crate::{mmap_unsafe, Error};

/// Using Physical, DMA is performed with physical addresses on hugepages, access to the device is done through sysfs
pub struct Physical {
    pci_addr: String,
    page_size: Pagesize,
}

impl Physical {
    pub fn init(pci_addr: &str) -> Result<Self> {
        Self::init_with_args(pci_addr, memory::DEFAULT_PAGE_SIZE)
    }

    pub fn init_with_args(pci_addr: &str, page_size: Pagesize) -> Result<Self> {
        let pci_addr = pci_addr.to_string();

        if page_size == Pagesize::Page4K {
            return Err(Error::Mmio(
                "Pagesize 4K not supported for non-Vfio!".to_string(),
            ));
        }

        let mmio = Self {
            pci_addr,
            page_size,
        };

        if !mmio.is_bound_to_pci_stub()? {
            return Err(Error::Mmio(
                "The device is not bound to Pci-Stub!".to_string(),
            ));
        }

        mmio.enable_dma()?;

        mmio.disable_interrupts()?;

        Ok(mmio)
    }

    fn is_bound_to_pci_stub(&self) -> Result<bool> {
        let pci_stub_dir = Path::new("/sys/bus/pci/drivers/pci-stub");

        if !pci_stub_dir.exists() {
            return Ok(false);
        }

        for entry in fs::read_dir(pci_stub_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_symlink() {
                continue;
            }

            if let Some(filename) = path.file_name() {
                if let Some(filename_str) = filename.to_str() {
                    if filename_str == self.pci_addr {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Translates a virtual address to its physical counterpart
    fn virt_to_phys(addr: usize) -> Result<usize> {
        let pagesize = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        let mut file = fs::OpenOptions::new()
            .read(true)
            .open("/proc/self/pagemap")?;

        file.seek(io::SeekFrom::Start(
            (addr / pagesize * mem::size_of::<usize>()) as u64,
        ))?;

        let mut buffer = [0; mem::size_of::<usize>()];
        file.read_exact(&mut buffer)?;

        let phys = unsafe { mem::transmute::<[u8; mem::size_of::<usize>()], usize>(buffer) };
        Ok((phys & 0x007F_FFFF_FFFF_FFFF) * pagesize + addr % pagesize)
    }

    /// Enables direct memory access for the device at `pci_addr`.
    pub fn enable_dma(&self) -> Result<()> {
        let path = format!("/sys/bus/pci/devices/{}/config", self.pci_addr);
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;

        let mut dma = read_io16(&mut file, COMMAND_REGISTER_OFFSET)?;
        dma |= 1 << BUS_MASTER_ENABLE_BIT;
        write_io16(&mut file, dma, COMMAND_REGISTER_OFFSET)?;

        Ok(())
    }

    /// Disable `INTx` interrupts for the device.
    pub fn disable_interrupts(&self) -> Result<()> {
        let path = format!("/sys/bus/pci/devices/{}/config", self.pci_addr);
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;

        let mut dma = read_io16(&mut file, COMMAND_REGISTER_OFFSET)?;
        dma |= 1 << INTERRUPT_DISABLE;
        write_io16(&mut file, dma, COMMAND_REGISTER_OFFSET)?;

        Ok(())
    }
}

impl Mapping for Physical {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>> {
        let size = self.page_size.shift_up(size);

        let id = HUGEPAGE_ID.fetch_add(1, Ordering::SeqCst);

        let path = format!("/mnt/huge/nvme-{}-{}", process::id(), id);
        // println!("allocating: {path}");
        let res = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path);

        let file = match res {
            Ok(file) => file,
            Err(error) => {
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("huge page {path} could not be created - huge pages enabled?, {error}"),
                )))
            }
        };

        let ptr = mmap_fd_unsafe!(size, self.page_size.mmap_flags(), file.as_raw_fd())?;

        mlock_unsafe!(ptr, size)?;

        Ok(Dma {
            virt: ptr.cast::<T>(),
            phys: Self::virt_to_phys(ptr as usize)?,
            size,
        })
    }

    /// Mmaps a pci resource0 and returns a pointer to the mapped memory.
    fn map_resource(&self) -> Result<(*mut u8, usize)> {
        let path = format!("/sys/bus/pci/devices/{}/resource0", self.pci_addr);

        let file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
        let len = fs::metadata(&path)?.len() as usize;

        if len == 0 {
            return Err(Error::Vroom("Resource0 len is 0".to_string()));
        }

        // mmap with null ptr to address => kernel chooses address to create mapping
        let ptr = mmap_fd_unsafe!(len, file.as_raw_fd())?;

        Ok((ptr.cast::<u8>(), len))
    }

    fn deallocate<T>(&self, dma: &Dma<T>) -> Result<()> {
        let addr = dma.virt.cast::<libc::c_void>();
        let len = dma.size;

        munlock_unsafe!(addr, len)?;

        munmap_unsafe!(addr, len)?;

        Ok(())
    }
}
