#![allow(dead_code)]

use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions};
use std::mem;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::path::Path;
use std::ptr;
use crate::HUGE_PAGE_SIZE;

use crate::vfio_constants::*;

use crate::memory::{get_vfio, set_vfio, IOVA_WIDTH, VFIO_GROUP_FILE_DESCRIPTORS, Dma};
use crate::pci::{pci_open_resource_ro, read_hex, BUS_MASTER_ENABLE_BIT, COMMAND_REGISTER_OFFSET};



/// struct vfio_iommu_type1_dma_map, grabbed from linux/vfio.h
#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
struct vfio_iommu_type1_dma_map {
    argsz: u32,
    flags: u32,
    vaddr: *mut u8,
    iova: *mut u8,
    size: usize,
}

/// struct vfio_group_status, grabbed from linux/vfio.h
#[allow(non_camel_case_types)]
#[repr(C)]
struct vfio_group_status {
    argsz: u32,
    flags: u32,
}

/// struct vfio_region_info, grabbed from linux/vfio.h
#[allow(non_camel_case_types)]
#[repr(C)]
struct vfio_region_info {
    argsz: u32,
    flags: u32,
    index: u32,
    cap_offset: u32,
    size: u64,
    offset: u64,
}

/// struct vfio_irq_set, grabbed from linux/vfio.h
///
/// As this is a dynamically sized struct (has an array at the end) we need to use
/// Dynamically Sized Types (DSTs) which can be found at
/// https://doc.rust-lang.org/nomicon/exotic-sizes.html#dynamically-sized-types-dsts
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct vfio_irq_set<T: ?Sized> {
    pub argsz: u32,
    pub flags: u32,
    pub index: u32,
    pub start: u32,
    pub count: u32,
    pub data: T,
}

/// struct vfio_irq_info, grabbed from linux/vfio.h
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct vfio_irq_info {
    pub argsz: u32,
    pub flags: u32,
    pub index: u32, /* IRQ index */
    pub count: u32, /* Number of IRQs within this index */
}

/// 'libc::epoll_event' equivalent.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Event {
    pub events: u32,
    pub data: u64,
}

pub struct Vfio {
    pci_addr: String,
    device_fd: RawFd,
    container_fd: RawFd,
}

impl Vfio {
    /// Initializes the IOMMU for a given PCI device. The device must be bound to the VFIO driver.
    pub fn init(pci_addr: &str) -> Result<Self, Box<dyn Error>> {
        let group_file: File;
        let group_fd: RawFd;

        if Self::is_intel_iommu(pci_addr) {
            let mgaw = Self::get_intel_iommu_gaw(pci_addr);

            if mgaw < IOVA_WIDTH {
                println!("IOMMU supports only {} bit wide IOVAs, reduce IOVA_WIDTH in src/memory.rs if DMA mappings fail!", mgaw);
            }
        } else {
            println!("Cannot determine IOVA width on non-Intel IOMMU, reduce IOVA_WIDTH in src/memory.rs if DMA mappings fail!");
        }

        // we also have to build this vfio struct...
        let mut group_status: vfio_group_status = vfio_group_status {
            argsz: mem::size_of::<vfio_group_status>() as u32,
            flags: 0,
        };

        // need to set up the container exactly once
        let mut first_time_setup = false;

        let container_fd = if let Some(vfio) = get_vfio() {
            vfio.container_fd
        } else {
            first_time_setup = true;
            // open vfio file to create new vfio container
            let container_file = OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/vfio/vfio")?;
            let container_fd = container_file.into_raw_fd();
            set_vfio(container_fd);

            // check if the container's API version is the same as the VFIO API's
            if unsafe { libc::ioctl(container_fd, VFIO_GET_API_VERSION) } != VFIO_API_VERSION {
                return Err("unknown VFIO API Version".into());
            }

            // check if type1 is supported
            if unsafe { libc::ioctl(container_fd, VFIO_CHECK_EXTENSION, VFIO_TYPE1_IOMMU) } != 1 {
                return Err("container doesn't support Type1 IOMMU".into());
            }

            container_fd
        };

        // find vfio group for device
        let link = fs::read_link(format!("/sys/bus/pci/devices/{}/iommu_group", pci_addr)).unwrap();
        let group = link
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();

        let mut vfio_gfds = VFIO_GROUP_FILE_DESCRIPTORS.lock().unwrap();

        #[allow(clippy::map_entry)]
        if !vfio_gfds.contains_key(&group) {
            // open the devices' group
            group_file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/vfio/{}", group))?;
            // group file descriptor
            group_fd = group_file.into_raw_fd();

            // Test the group is viable and available
            if unsafe { libc::ioctl(group_fd, VFIO_GROUP_GET_STATUS, &mut group_status) } == -1 {
                return Err(format!(
                    "failed to VFIO_GROUP_GET_STATUS. Errno: {}",
                    std::io::Error::last_os_error()
                )
                .into());
            }
            if (group_status.flags & VFIO_GROUP_FLAGS_VIABLE) != 1 {
                return Err(
                    "group is not viable (ie, not all devices in this group are bound to vfio)"
                        .into(),
                );
            }

            // Add the group to the container
            if unsafe { libc::ioctl(group_fd, VFIO_GROUP_SET_CONTAINER, &container_fd) } == -1 {
                return Err(format!(
                    "failed to VFIO_GROUP_SET_CONTAINER. Errno: {}",
                    std::io::Error::last_os_error()
                )
                .into());
            }

            vfio_gfds.insert(group, group_fd);
        } else {
            group_fd = *vfio_gfds.get(&group).unwrap();
        }

        if first_time_setup {
            //    Enable the IOMMU model we want
            if unsafe { libc::ioctl(container_fd, VFIO_SET_IOMMU, VFIO_TYPE1_IOMMU) } == -1 {
                return Err(format!(
                    "failed to VFIO_SET_IOMMU to VFIO_TYPE1_IOMMU. Errno: {}",
                    std::io::Error::last_os_error()
                )
                .into());
            }
        }

        // Get a file descriptor for the device
        let device_fd: RawFd = unsafe { libc::ioctl(group_fd, VFIO_GROUP_GET_DEVICE_FD, pci_addr) };
        if device_fd == -1 {
            return Err(format!(
                "failed to VFIO_GROUP_GET_DEVICE_FD. Errno: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        let vfio = Self {
            pci_addr: pci_addr.to_string(),
            device_fd,
            container_fd,
        };

        vfio.enable_dma()?;

        Ok(vfio)
    }

    pub fn is_enabled(pci_addr: &str) -> bool {
        Path::new(&format!("/sys/bus/pci/devices/{}/iommu_group", pci_addr)).exists()
    }
    pub(crate) fn allocate<T>(size: usize) -> Result<Dma<T>, Box<dyn Error>> {
        let ptr = if IOVA_WIDTH < crate::memory::X86_VA_WIDTH {
            // To support IOMMUs capable of 39 bit wide IOVAs only, we use
            // 32 bit addresses. Since mmap() ignores libc::MAP_32BIT when
            // using libc::MAP_HUGETLB, we create a 32 bit address with the
            // right alignment (huge page size, e.g. 2 MB) on our own.

            // first allocate memory of size (needed size + 1 huge page) to
            // get a mapping containing the huge page size aligned address
            let addr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    size + HUGE_PAGE_SIZE,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_32BIT,
                    -1,
                    0,
                )
            };

            // calculate the huge page size aligned address by rounding up
            let aligned_addr = ((addr as isize + HUGE_PAGE_SIZE as isize - 1)
                & -(HUGE_PAGE_SIZE as isize)) as *mut libc::c_void;

            let free_chunk_size = aligned_addr as usize - addr as usize;

            // free unneeded pages (i.e. all chunks of the additionally mapped huge page)
            unsafe {
                libc::munmap(addr, free_chunk_size);
                libc::munmap(aligned_addr.add(size), HUGE_PAGE_SIZE - free_chunk_size);
            }

            // finally map huge pages at the huge page size aligned 32 bit address
            unsafe {
                libc::mmap(
                    aligned_addr as *mut libc::c_void,
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED
                        | libc::MAP_ANONYMOUS
                        | libc::MAP_HUGETLB
                        | crate::memory::MAP_HUGE_2MB
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
                    libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_HUGETLB | crate::memory::MAP_HUGE_2MB,
                    -1,
                    0,
                )
            }
        };

        // This is the main IOMMU work: IOMMU DMA MAP the memory...
        if ptr == libc::MAP_FAILED {
            Err(format!(
                "failed to memory map DMA-memory. Errno: {}",
                std::io::Error::last_os_error()
            )
                .into())
        } else {
            let iova = Vfio::map_dma(ptr as usize, size)?;

            let memory = Dma {
                virt: ptr as *mut T,
                phys: iova,
                size,
            };

            Ok(memory)
        }
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

        let mut dma: u16 = 0;
        if unsafe {
            libc::pread(
                self.device_fd,
                &mut dma as *mut _ as *mut libc::c_void,
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

        dma |= 1 << BUS_MASTER_ENABLE_BIT;

        if unsafe {
            libc::pwrite(
                self.device_fd,
                &mut dma as *mut _ as *mut libc::c_void,
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

    /// mmap a VFIO resource and returns a pointer to the mapped memory.
    pub fn map_region(&self, index: u32) -> Result<(*mut u8, usize), Box<dyn Error>> {
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
        let addr = ptr as *mut u8;

        Ok((addr, len))
    }

    pub fn map_dma(ptr: usize, size: usize) -> Result<usize, Box<dyn Error>> {
        let mut iommu_dma_map: vfio_iommu_type1_dma_map = vfio_iommu_type1_dma_map {
            argsz: mem::size_of::<vfio_iommu_type1_dma_map>() as u32,
            vaddr: ptr as *mut u8,
            size,
            iova: ptr as *mut u8,
            flags: VFIO_DMA_MAP_FLAG_READ | VFIO_DMA_MAP_FLAG_WRITE,
        };
        let ioctl_result = unsafe {
            libc::ioctl(
                get_vfio().unwrap(),
                VFIO_IOMMU_MAP_DMA,
                &mut iommu_dma_map,
            )
        };
        if ioctl_result != -1 {
            Ok(iommu_dma_map.iova as usize)
        } else {
            Err(format!(
                "failed to map the DMA memory (ulimit set?). Errno: {}",
                std::io::Error::last_os_error()
            )
            .into())
        }
    }

    /// Checks if the IOMMU is from Intel.
    pub fn is_intel_iommu(pci_addr: &str) -> bool {
        Path::new(&format!(
            "/sys/bus/pci/devices/{}/iommu/intel-iommu",
            pci_addr
        ))
        .exists()
    }

    /// Returns the IOMMU guest address width.
    pub fn get_intel_iommu_gaw(pci_addr: &str) -> u8 {
        let mut iommu_cap_file = pci_open_resource_ro(pci_addr, "iommu/intel-iommu/cap")
            .expect("failed to read IOMMU capabilities");

        let iommu_cap = read_hex(&mut iommu_cap_file)
            .expect("failed to convert IOMMU capabilities hex string to u64");

        let mgaw = ((iommu_cap & VTD_CAP_MGAW_MASK) >> VTD_CAP_MGAW_SHIFT) + 1;

        mgaw as u8
    }
}
