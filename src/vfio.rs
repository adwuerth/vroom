#![allow(dead_code)]
#![allow(clippy::must_use_candidate)]

use crate::ioallocator::Allocating;
use crate::PAGESIZE_2MIB;
use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions};
use std::mem;

use std::os::unix::io::{IntoRawFd, RawFd};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

#[allow(clippy::wildcard_imports)]
use crate::vfio_constants::*;

#[allow(clippy::wildcard_imports)]
use crate::vfio_structs::*;
use lazy_static::lazy_static;
use libc::ioctl;

use crate::memory::{Dma, Pagesize};
use crate::pci::{pci_open_resource_ro, read_hex, BUS_MASTER_ENABLE_BIT, COMMAND_REGISTER_OFFSET};
use std::collections::HashMap;
use std::sync::Mutex;

lazy_static! {
    pub(crate) static ref VFIO_GROUP_FILE_DESCRIPTORS: Mutex<HashMap<i32, RawFd>> =
        Mutex::new(HashMap::new());
}

// from https://www.kernel.org/doc/Documentation/x86/x86_64/mm.txt
pub(crate) const X86_VA_WIDTH: u8 = 47;
pub(crate) const USE_CDEV: bool = false;

lazy_static! {
    /// IOVA_WIDTH is usually greater or equals to 47, e.g. in VMs only 39
    static ref IOVA_WIDTH: AtomicU8 = AtomicU8::new(X86_VA_WIDTH);
    static ref IOVA_CTR: AtomicUsize = AtomicUsize::new(0);
}

macro_rules! ioctl {
    ($fd:expr, $request:expr, $arg:expr, $error:expr) => {{
        let result = unsafe { ioctl($fd, $request, $arg) };
        if result < 0 {
            Err(format!(
                "{} Errno: {}",
                $error,
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(result)
        }
    }};
    ($fd:expr, $request:expr, $error:expr) => {{
        let result = unsafe { ioctl($fd, $request) };
        if result < 0 {
            Err(format!(
                "{} Errno: {}",
                $error,
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(result)
        }
    }};
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vfio {
    pci_addr: String,
    device_fd: RawFd,
    container_fd: RawFd,
    ioas_id: u32,
    iommufd: i32,
    page_size: Pagesize,
}

/// Implementation of Linux VFIO framework for direct device access.
impl Vfio {
    /// Initializes the IOMMU for a given PCI device. The device must be bound to the VFIO driver.
    /// # Panics
    /// # Errors
    #[allow(clippy::too_many_lines)]
    pub fn init(pci_addr: &str) -> Result<Self, Box<dyn Error>> {
        Self::init_with_args(pci_addr, Pagesize::Page2M, false)
    }

    #[allow(clippy::too_many_lines)]
    pub fn init_with_args(
        pci_addr: &str,
        page_size: Pagesize,
        use_cdev: bool,
    ) -> Result<Self, Box<dyn Error>> {
        if use_cdev {
            return Self::init_cdev(pci_addr, page_size);
        }

        let group_file: File;
        let group_fd: RawFd;

        println!(
            "initializing vfio with IOVA WIDTH = {}",
            IOVA_WIDTH.load(Ordering::Relaxed)
        );

        Self::check_intel_iommu(pci_addr);

        // we also have to build this vfio struct...
        let mut group_status: vfio_group_status = vfio_group_status {
            argsz: mem::size_of::<vfio_group_status>() as u32,
            flags: 0,
        };

        // open vfio file to create new vfio container
        let container_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/vfio/vfio")?;
        let container_fd = container_file.into_raw_fd();

        // check if the container's API version is the same as the VFIO API's
        if ioctl!(
            container_fd,
            VFIO_GET_API_VERSION,
            "failed to VFIO_GET_API_VERSION"
        )? != VFIO_API_VERSION
        {
            return Err("unknown VFIO API Version".into());
        }

        // check if type1 is supported
        if ioctl!(
            container_fd,
            VFIO_CHECK_EXTENSION,
            VFIO_TYPE1_IOMMU,
            "failed to VFIO_CHECK_EXTENSION"
        )? != 1
        {
            return Err("container doesn't support Type1 IOMMU".into());
        }

        // find vfio group for device
        let link = fs::read_link(format!("/sys/bus/pci/devices/{pci_addr}/iommu_group")).unwrap();
        let group = link
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();

        let mut vfio_gfds = VFIO_GROUP_FILE_DESCRIPTORS.lock().unwrap();

        #[allow(clippy::map_entry)]
        if vfio_gfds.contains_key(&group) {
            group_fd = *vfio_gfds.get(&group).unwrap();
        } else {
            // open the devices' group
            group_file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/vfio/{group}"))?;
            // group file descriptor
            group_fd = group_file.into_raw_fd();

            // Test the group is viable and available
            ioctl!(
                group_fd,
                VFIO_GROUP_GET_STATUS,
                &mut group_status,
                "failed to VFIO_GROUP_GET_STATUS"
            )?;

            if (group_status.flags & VFIO_GROUP_FLAGS_VIABLE) != 1 {
                return Err(
                    "group is not viable (ie, not all devices in this group are bound to vfio)"
                        .into(),
                );
            }

            // Add the group to the container
            ioctl!(
                group_fd,
                VFIO_GROUP_SET_CONTAINER,
                &container_fd,
                "failed to VFIO_GROUP_SET_CONTAINER"
            )?;

            vfio_gfds.insert(group, group_fd);
        }

        //    Enable the IOMMU model we want
        ioctl!(
            container_fd,
            VFIO_SET_IOMMU,
            VFIO_TYPE1_IOMMU,
            "failed to VFIO_SET_IOMMU to VFIO_TYPE1_IOMMU"
        )?;

        // Get a file descriptor for the device
        let device_fd = ioctl!(
            group_fd,
            VFIO_GROUP_GET_DEVICE_FD,
            pci_addr,
            "failed to VFIO_GROUP_GET_DEVICE_FD"
        )?;

        let mut iommu_info: vfio_iommu_type1_info = vfio_iommu_type1_info {
            argsz: mem::size_of::<vfio_iommu_type1_info>() as u32,
            flags: 0,
            iova_pgsizes: 0,
            cap_offset: 0,
            pad: 0,
        };

        ioctl!(
            container_fd,
            VFIO_IOMMU_GET_INFO,
            &mut iommu_info,
            "failed to VFIO_IOMMU_GET_INFO"
        )?;

        println!(
            "IOMMU page sizes: {:b} {:x} {}",
            iommu_info.iova_pgsizes, iommu_info.iova_pgsizes, iommu_info.iova_pgsizes
        );

        let vfio = Self {
            pci_addr: pci_addr.to_string(),
            device_fd,
            container_fd,
            ioas_id: 0,
            iommufd: 0,
            page_size,
        };

        vfio.enable_dma()?;

        Ok(vfio)
    }

    fn init_cdev(pci_addr: &str, page_size: Pagesize) -> Result<Self, Box<dyn Error>> {
        let iommufd = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/iommu")?;
        let iommufd = iommufd.into_raw_fd();

        let cdev_fd = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/vfio/devices/vfio0")?;
        let cdev_fd = cdev_fd.into_raw_fd();

        let mut bind = vfio_device_bind_iommufd {
            argsz: mem::size_of::<vfio_device_bind_iommufd>() as u32,
            flags: 0,
            iommufd,
            out_devid: 0,
        };

        let mut alloc_data = iommu_ioas_alloc {
            size: mem::size_of::<iommu_ioas_alloc>() as u32,
            flags: 0,
            out_ioas_id: 0,
        };

        let mut attach_data = vfio_device_attach_iommufd_pt {
            argsz: mem::size_of::<vfio_device_attach_iommufd_pt>() as u32,
            flags: 0,
            pt_id: 0,
        };

        ioctl!(
            cdev_fd,
            VFIO_DEVICE_BIND_IOMMUFD,
            &mut bind,
            "failed to bind iommufd to cdev"
        )?;

        ioctl!(
            iommufd,
            IOMMU_IOAS_ALLOC,
            &mut alloc_data,
            "failed to allocate IOAS"
        )?;

        attach_data.pt_id = alloc_data.out_ioas_id;

        ioctl!(
            cdev_fd,
            VFIO_DEVICE_ATTACH_IOMMUFD_PT,
            &mut attach_data,
            "failed to attach iommufd to cdev"
        )?;

        let vfio = Self {
            pci_addr: pci_addr.to_string(),
            device_fd: cdev_fd,
            container_fd: -1,
            ioas_id: alloc_data.out_ioas_id,
            iommufd,
            page_size,
        };

        vfio.enable_dma()?;
        Ok(vfio)
    }

    fn check_intel_iommu(pci_addr: &str) {
        if Self::is_intel_iommu(pci_addr) {
            let mgaw = Self::get_intel_iommu_gaw(pci_addr);

            if mgaw < IOVA_WIDTH.load(Ordering::Relaxed) {
                println!(
                    "IOMMU only supports {mgaw} bit wide IOVAs. Setting IOVA_WIDTH to {mgaw}!"
                );
            }

            IOVA_WIDTH.store(mgaw, Ordering::Relaxed);
        } else {
            println!("Cannot determine IOVA width on non-Intel IOMMU, reduce IOVA_WIDTH in src/memory.rs if DMA mappings fail!");
        }
    }

    #[must_use]
    pub fn is_enabled(pci_addr: &str) -> bool {
        Path::new(&format!("/sys/bus/pci/devices/{pci_addr}/iommu_group")).exists()
    }

    /// Enables DMA Bit for VFIO device
    fn enable_dma(&self) -> Result<(), Box<dyn Error>> {
        // Get region info for config region
        let mut conf_reg: vfio_region_info = vfio_region_info {
            argsz: mem::size_of::<vfio_region_info>() as u32,
            flags: 0,
            index: VFIO_PCI_CONFIG_REGION_INDEX,
            cap_offset: 0,
            size: 0,
            offset: 0,
        };
        if unsafe { libc::ioctl(self.device_fd, VFIO_DEVICE_GET_REGION_INFO, &mut conf_reg) } == -1
        {
            return Err(format!(
                "failed to VFIO_DEVICE_GET_REGION_INFO for index VFIO_PCI_CONFIG_REGION_INDEX. Errno: {}",
                std::io::Error::last_os_error()
            ).into());
        }

        // Read current value of command register
        let mut dma: u16 = 0;
        if unsafe {
            libc::pread(
                self.device_fd,
                std::ptr::addr_of_mut!(dma).cast::<libc::c_void>(),
                2,
                (conf_reg.offset + COMMAND_REGISTER_OFFSET) as i64,
            )
        } == -1
        {
            return Err(format!(
                "failed to pread DMA bit. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        // Set the bus master enable bit
        dma |= 1 << BUS_MASTER_ENABLE_BIT;

        if unsafe {
            libc::pwrite(
                self.device_fd,
                std::ptr::addr_of_mut!(dma).cast::<libc::c_void>(),
                2,
                (conf_reg.offset + COMMAND_REGISTER_OFFSET) as i64,
            )
        } == -1
        {
            return Err(format!(
                "failed to pwrite DMA bit. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }
        Ok(())
    }

    /// mmap the io device into host memory, and return a pointer to the mapped memory.
    /// This enables direct access to the device's memory.
    /// # Errors
    pub fn map_resource_index(&self, index: u32) -> Result<(*mut u8, usize), Box<dyn Error>> {
        let mut region_info: vfio_region_info = vfio_region_info {
            argsz: mem::size_of::<vfio_region_info>() as u32,
            flags: 0,
            index,
            cap_offset: 0,
            size: 0,
            offset: 0,
        };
        if unsafe {
            libc::ioctl(
                self.device_fd,
                VFIO_DEVICE_GET_REGION_INFO,
                &mut region_info,
            )
        } == -1
        {
            return Err(format!(
                "failed to VFIO_DEVICE_GET_REGION_INFO. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        let len = region_info.size as usize;

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                self.device_fd,
                region_info.offset as i64,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(format!(
                "failed to mmap region. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }
        let addr = ptr.cast::<u8>();

        Ok((addr, len))
    }
    // Maps a memory region for DMA.
    /// # Errors
    pub fn map_dma<T>(
        &self,
        ptr: *mut libc::c_void,
        size: usize,
    ) -> Result<Dma<T>, Box<dyn Error>> {
        // This is the main IOMMU work: IOMMU DMA MAP the memory...
        if ptr == libc::MAP_FAILED {
            return Err(format!(
                "failed to memory map DMA-memory. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        if USE_CDEV {
            return self.map_dma_cdev(ptr, size);
        }

        // a direct mapping of the user-space virtual address space and the io virtual address space is used => iova = vaddr
        let mut iommu_dma_map: vfio_iommu_type1_dma_map = vfio_iommu_type1_dma_map {
            argsz: mem::size_of::<vfio_iommu_type1_dma_map>() as u32,
            flags: VFIO_DMA_MAP_FLAG_READ | VFIO_DMA_MAP_FLAG_WRITE,
            vaddr: ptr as u64,
            iova: ptr as u64, // TEST THIS PERFORMANCE
            size,
        };

        ioctl!(
            self.container_fd,
            VFIO_IOMMU_MAP_DMA,
            &mut iommu_dma_map,
            "failed to map the DMA memory"
        )?;

        let iova = iommu_dma_map.iova as usize;

        let memory = Dma {
            virt: ptr.cast::<T>(),
            phys: iova,
            size,
        };

        Ok(memory)
    }

    fn map_dma_cdev<T>(
        &self,
        ptr: *mut libc::c_void,
        size: usize,
    ) -> Result<Dma<T>, Box<dyn Error>> {
        println!("mapping DMA with cdev");
        // let mut map = iommu_ioas_map {
        //     size: mem::size_of::<iommu_ioas_map>() as u32,
        //     flags: IOMMU_IOAS_MAP_FIXED_IOVA | IOMMU_IOAS_MAP_WRITEABLE | IOMMU_IOAS_MAP_READABLE,
        //     ioas_id: self.ioas_id,
        //     __reserved: 0,
        //     user_va: ptr as u64,
        //     length: size as u64,
        //     iova: ptr as u64,
        // };
        let mut map = iommu_ioas_map {
            size: mem::size_of::<iommu_ioas_map>() as u32,
            flags: IOMMU_IOAS_MAP_WRITEABLE | IOMMU_IOAS_MAP_READABLE,
            ioas_id: self.ioas_id,
            __reserved: 0,
            user_va: ptr as u64,
            length: size as u64,
            iova: 0,
        };

        ioctl!(
            self.iommufd,
            IOMMU_IOAS_MAP,
            &mut map,
            "failed to map IOAS memory"
        )?;

        Ok(Dma {
            virt: ptr.cast::<T>(),
            phys: map.iova as usize,
            size,
        })
    }

    /// Maps a memory region for DMA.
    /// # Errors
    pub fn unmap_dma<T>(&self, dma: &Dma<T>) -> Result<(), Box<dyn Error>> {
        let mut dma_unmap = vfio_iommu_type1_dma_unmap {
            argsz: mem::size_of::<vfio_iommu_type1_dma_unmap>() as u32,
            iova: dma.phys as *mut u8,
            size: dma.size,
            flags: 0,
            data: ptr::null_mut(),
        };

        if unsafe { libc::ioctl(self.container_fd, VFIO_IOMMU_UNMAP_DMA, &mut dma_unmap) } < 0 {
            return Err(format!(
                "failed to unmap the DMA memory. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        Ok(())
    }

    /// Checks if the IOMMU is from Intel.
    #[allow(clippy::must_use_candidate)]
    pub fn is_intel_iommu(pci_addr: &str) -> bool {
        Path::new(&format!(
            "/sys/bus/pci/devices/{pci_addr}/iommu/intel-iommu"
        ))
        .exists()
    }

    /// Returns the IOMMU guest address width.
    /// # Panics
    /// Panics when the IOMMU capabilities file cannot be read, or when the hex string cannot be converted to a u64.
    #[allow(clippy::must_use_candidate)]
    pub fn get_intel_iommu_gaw(pci_addr: &str) -> u8 {
        let mut iommu_cap_file = pci_open_resource_ro(pci_addr, "iommu/intel-iommu/cap")
            .expect("failed to read IOMMU capabilities");

        let iommu_cap = read_hex(&mut iommu_cap_file)
            .expect("failed to convert IOMMU capabilities hex string to u64");

        let mgaw = ((iommu_cap & VTD_CAP_MGAW_MASK) >> VTD_CAP_MGAW_SHIFT) + 1;

        mgaw as u8
    }

    /// Allocate `size` bytes memory with 1 GiB page size on the host device. Returns pointer to allocated memory
    pub fn allocate_1gib(size: usize) -> *mut libc::c_void {
        unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_HUGETLB | libc::MAP_HUGE_1GB,
                -1,
                0,
            )
        }
    }

    /// Allocate `size` bytes memory with 2 MiB page size on the host device. Returns pointer to allocated memory
    pub fn allocate_2mib(size: usize) -> *mut libc::c_void {
        if IOVA_WIDTH.load(Ordering::Relaxed) < X86_VA_WIDTH {
            // To support IOMMUs capable of 39 bit wide IOVAs only, we use
            // 32 bit addresses. Since mmap() ignores libc::MAP_32BIT when
            // using libc::MAP_HUGETLB, we create a 32-bit address with the
            // right alignment (huge page size, e.g. 2 MB) on our own.

            // first allocate memory of size (needed size + 1 huge page) to
            // get a mapping containing the huge page size aligned address
            let addr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    size + PAGESIZE_2MIB,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_32BIT,
                    -1,
                    0,
                )
            };

            // calculate the huge page size aligned address by rounding up
            let aligned_addr = ((addr as isize + PAGESIZE_2MIB as isize - 1)
                & -(PAGESIZE_2MIB as isize)) as *mut libc::c_void;

            let free_chunk_size = aligned_addr as usize - addr as usize;

            // free unneeded pages (i.e. all chunks of the additionally mapped huge page)
            unsafe {
                libc::munmap(addr, free_chunk_size);
                libc::munmap(aligned_addr.add(size), PAGESIZE_2MIB - free_chunk_size);
            }

            // finally map huge pages at the huge page size aligned 32-bit address
            unsafe {
                libc::mmap(
                    aligned_addr.cast::<libc::c_void>(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED
                        | libc::MAP_ANONYMOUS
                        | libc::MAP_HUGETLB
                        | libc::MAP_HUGE_2MB
                        | libc::MAP_FIXED,
                    -1,
                    0,
                )
            }
        } else {
            unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_HUGETLB | libc::MAP_HUGE_2MB,
                    -1,
                    0,
                )
            }
        }
    }

    /// Allocate `size` bytes memory with 4 KiB page size on the host device. Returns pointer to allocated memory
    pub fn allocate_4kib(size: usize) -> *mut libc::c_void {
        if IOVA_WIDTH.load(Ordering::Relaxed) < X86_VA_WIDTH {
            // To support IOMMUs capable of 39 bit wide IOVAs only, we use
            // 32 bit addresses.

            // Allocate memory of the needed size with 32-bit address space
            unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_32BIT,
                    -1,
                    0,
                )
            }
        } else {
            // Allocate memory of the needed size
            unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_POPULATE, // todo remove MAP_POPULATE
                    -1,
                    0,
                )
            }
        }
    }

    pub fn allocate_with_pagesize(&self, size: usize) -> *mut libc::c_void {
        let page_size = &self.page_size;
        match page_size {
            Pagesize::Page4K => Self::allocate_4kib(size),
            Pagesize::Page1G => Self::allocate_1gib(size),
            Pagesize::Page2M => Self::allocate_2mib(size),
        }
    }

    pub fn set_page_size(&mut self, page_size: Pagesize) {
        self.page_size = page_size;
    }
}

impl Allocating for Vfio {
    fn allocate<T>(&self, size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        let ptr = self.allocate_with_pagesize(size);
        self.map_dma(ptr, size)
    }

    fn map_resource(&self) -> Result<(*mut u8, usize), Box<dyn Error>> {
        self.map_resource_index(VFIO_PCI_BAR0_REGION_INDEX)
    }

    fn deallocate<T>(&self, dma: Dma<T>) -> Result<(), Box<dyn Error>> {
        self.unmap_dma(&dma)?;

        let size = self.page_size.shift_up(dma.size);

        match unsafe { libc::munmap(dma.virt.cast::<libc::c_void>(), size) } {
            0 => {
                println!("deallocated memory");
                Ok(())
            }
            _ => Err("failed to munmap memory".into()),
        }
    }
}

struct VfioGroupMapping {}

struct VfioCDevMapping {}
