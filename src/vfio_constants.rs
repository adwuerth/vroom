const _IOC_TYPESHIFT: u64 = 8;
const _IOC_NRSHIFT: u64 = 0;


// _IO macro from ioctl.h, _IOC is inlined, as only _IO is needed
#[allow(non_snake_case)]
const fn _IO(type_: u64, nr: u64) -> u64 {
    type_ << _IOC_TYPESHIFT | nr << _IOC_NRSHIFT
}

// constants needed for IOMMU. Grabbed from linux/vfio.h
const VFIO_TYPE: u64 = ';' as u8 as u64;
const VFIO_BASE: u64 = 100;

pub const VFIO_GET_API_VERSION: u64 = _IO(VFIO_TYPE, VFIO_BASE + 0);
pub const VFIO_CHECK_EXTENSION: u64 = _IO(VFIO_TYPE, VFIO_BASE + 1);
pub const VFIO_SET_IOMMU: u64 = _IO(VFIO_TYPE, VFIO_BASE + 2);
pub const VFIO_GROUP_GET_STATUS: u64 = _IO(VFIO_TYPE, VFIO_BASE + 3);
pub const VFIO_GROUP_SET_CONTAINER: u64 = _IO(VFIO_TYPE, VFIO_BASE + 4);
pub const VFIO_GROUP_GET_DEVICE_FD: u64 = _IO(VFIO_TYPE, VFIO_BASE + 6);
pub const VFIO_DEVICE_GET_REGION_INFO: u64 = _IO(VFIO_TYPE, VFIO_BASE + 8);

pub const VFIO_API_VERSION: i32 = 0;
pub const VFIO_TYPE1_IOMMU: u64 = 1;
pub const VFIO_GROUP_FLAGS_VIABLE: u32 = 1 << 0;


pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;
pub const VFIO_IOMMU_MAP_DMA: u64 = _IO(VFIO_TYPE, VFIO_BASE + 13);

// constants needed for IOMMU Interrupts. Grabbed from linux/vfio.h
pub const VFIO_DEVICE_GET_IRQ_INFO: u64 = _IO(VFIO_TYPE, VFIO_BASE + 9);
pub const VFIO_DEVICE_SET_IRQS: u64 = _IO(VFIO_TYPE, VFIO_BASE + 10);
pub const VFIO_IRQ_SET_DATA_NONE: u32 = 1 << 0; /* Data not present */
pub const VFIO_IRQ_SET_DATA_EVENTFD: u32 = 1 << 2; /* Data is eventfd (s32) */
pub const VFIO_IRQ_SET_ACTION_TRIGGER: u32 = 1 << 5; /* Trigger interrupt */

// from enum in vfio.h
pub const VFIO_PCI_MSI_IRQ_INDEX: u64 = 1;
pub const VFIO_PCI_MSIX_IRQ_INDEX: u64 = 2;

// from enum in vfio.h
pub const VFIO_PCI_CONFIG_REGION_INDEX: u32 = 7;
pub const VFIO_PCI_BAR0_REGION_INDEX: u32 = 0;

pub const VFIO_IRQ_INFO_EVENTFD: u32 = 1 << 0;

// constants to determine IOMMU (guest) address width
pub const VTD_CAP_MGAW_SHIFT: u8 = 16;
pub const VTD_CAP_MGAW_MASK: u64 = 0x3f << VTD_CAP_MGAW_SHIFT;