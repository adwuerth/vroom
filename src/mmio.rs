use crate::mapping::Mapping;
use crate::memory::{Dma, Pagesize};
use crate::pci::{
    read_io16, write_io16, BUS_MASTER_ENABLE_BIT, COMMAND_REGISTER_OFFSET, INTERRUPT_DISABLE,
};
use std::error::Error;
use std::io::Write;
use std::io::{Read, Seek};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, io, mem, process, ptr};

use crate::{memory, nvme};

static HUGEPAGE_ID: AtomicUsize = AtomicUsize::new(0);

pub struct Mmio {
    pci_addr: String,
}

impl Mmio {
    pub fn init(pci_addr: &str) -> Result<Self, Box<dyn Error>> {
        let pci_addr = pci_addr.to_string();
        let mmio = Self { pci_addr };

        // mmio.bind_to_stub_driver()?;

        // mmio.unbind_driver()?;

        mmio.enable_dma()?;

        mmio.disable_interrupts()?;

        Ok(mmio)
    }

    // todo check if this works
    fn bind_to_stub_driver(&self) -> Result<(), Box<dyn Error>> {
        let vendor_path = format!("/sys/bus/pci/devices/{}/vendor", self.pci_addr);
        let device_path = format!("/sys/bus/pci/devices/{}/device", self.pci_addr);

        let vendor = fs::read_to_string(vendor_path)?;
        let device = fs::read_to_string(device_path)?;

        let nvme_vd = format!("{vendor} {device}");

        println!("now trying to bind to pci-stub");

        let unbind_path = format!("/sys/bus/pci/devices/{}/driver/unbind", self.pci_addr);
        let mut file = fs::OpenOptions::new().write(true).open(unbind_path)?;
        file.write_all(self.pci_addr.as_bytes())?;
        println!("unbound driver");

        let new_id_path = "/sys/bus/pci/drivers/pci-stub/new_id";
        let mut file = fs::OpenOptions::new().write(true).open(new_id_path)?;
        file.write_all(nvme_vd.as_bytes())?;
        println!("set new id");

        let bind_path = "/sys/bus/pci/drivers/pci-stub/bind";
        let mut file = fs::OpenOptions::new().write(true).open(bind_path)?;
        file.write_all(self.pci_addr.as_bytes())?;
        println!("bound to pci-stub");

        Ok(())
    }

    /// Translates a virtual address to its physical counterpart
    fn virt_to_phys(addr: usize) -> Result<usize, Box<dyn Error>> {
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
    pub fn enable_dma(&self) -> Result<(), Box<dyn Error>> {
        let path = format!("/sys/bus/pci/devices/{}/config", self.pci_addr);
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;

        let mut dma = read_io16(&mut file, COMMAND_REGISTER_OFFSET)?;
        dma |= 1 << BUS_MASTER_ENABLE_BIT;
        write_io16(&mut file, dma, COMMAND_REGISTER_OFFSET)?;

        Ok(())
    }

    /// Unbinds kernel driver
    pub fn unbind_driver(&self) -> Result<(), Box<dyn Error>> {
        let path = format!("/sys/bus/pci/devices/{}/driver/unbind", self.pci_addr);

        match fs::OpenOptions::new().write(true).open(path) {
            Ok(mut f) => {
                write!(f, "{}", self.pci_addr)?;
                Ok(())
            }
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Box::new(e)),
        }
    }

    /// Disable `INTx` interrupts for the device.
    pub fn disable_interrupts(&self) -> Result<(), Box<dyn Error>> {
        let path = format!("/sys/bus/pci/devices/{}/config", self.pci_addr);
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;

        let mut dma = read_io16(&mut file, COMMAND_REGISTER_OFFSET)?;
        dma |= 1 << INTERRUPT_DISABLE;
        write_io16(&mut file, dma, COMMAND_REGISTER_OFFSET)?;

        Ok(())
    }
}

impl Mapping for Mmio {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        let size = Pagesize::Page2M.shift_up(size);

        let id = HUGEPAGE_ID.fetch_add(1, Ordering::SeqCst);
        let path = format!("/mnt/huge/nvme-{}-{}", process::id(), id);

        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path.clone())
        {
            Ok(f) => {
                let ptr = unsafe {
                    libc::mmap(
                        ptr::null_mut(),
                        size,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_SHARED | libc::MAP_HUGETLB,
                        f.as_raw_fd(),
                        0,
                    )
                };
                if ptr == libc::MAP_FAILED {
                    Err("failed to mmap huge page - are huge pages enabled and free?".into())
                } else if unsafe { libc::mlock(ptr, size) } == 0 {
                    let memory = Dma {
                        virt: ptr.cast::<T>(),
                        phys: Self::virt_to_phys(ptr as usize)?,
                        size,
                    };
                    Ok(memory)
                } else {
                    Err("failed to memory lock huge page".into())
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => Err(Box::new(io::Error::new(
                e.kind(),
                format!("huge page {path} could not be created - huge pages enabled?"),
            ))),
            Err(e) => Err(Box::new(e)),
        }
    }

    /// Mmaps a pci resource0 and returns a pointer to the mapped memory.
    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>> {
        let path = format!("/sys/bus/pci/devices/{}/resource0", self.pci_addr);

        let file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
        let len = fs::metadata(&path)?.len() as usize;

        // mmap with null ptr to address => kernel chooses address to create mapping
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
            .cast::<u8>()
        };

        if ptr.is_null() || len == 0 {
            Err("pci mapping failed".into())
        } else {
            Ok((ptr, len))
        }
    }

    fn deallocate<T>(&self, dma: Dma<T>) -> Result<(), Box<dyn Error>> {
        todo!()
    }
}
