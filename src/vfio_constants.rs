const _IOC_TYPESHIFT: u64 = 8;
const _IOC_NRSHIFT: u64 = 0;

// _IO macro from ioctl.h, _IOC is inlined, as only _IO is needed
#[allow(non_snake_case)]
const fn _IO(type_: u64, nr: u64) -> u64 {
    type_ << _IOC_TYPESHIFT | nr << _IOC_NRSHIFT
}

// constants needed for IOMMU. Grabbed from linux/vfio.h
const VFIO_TYPE: u64 = b';' as u64;
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
pub const VFIO_IOMMU_GET_INFO: u64 = _IO(VFIO_TYPE, VFIO_BASE + 12);
pub const VFIO_IOMMU_MAP_DMA: u64 = _IO(VFIO_TYPE, VFIO_BASE + 13);
pub const VFIO_IOMMU_UNMAP_DMA: u64 = _IO(VFIO_TYPE, VFIO_BASE + 14);

// iommufd vfio constants
pub const VFIO_DEVICE_BIND_IOMMUFD: u64 = _IO(VFIO_TYPE, VFIO_BASE + 18);
pub const VFIO_DEVICE_ATTACH_IOMMUFD_PT: u64 = _IO(VFIO_TYPE, VFIO_BASE + 19);

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

// Intel VTd consts
// constants to determine IOMMU (guest) address width
pub const VTD_CAP_MGAW_SHIFT: u8 = 16;
pub const VTD_CAP_MGAW_MASK: u64 = 0x3f << VTD_CAP_MGAW_SHIFT;

// IOMMUFD constants, grabbed from include/uapi/linux/iommufd.h
// enum IOMMUFD_CMD {
//     IOMMUFD_CMD_BASE = 0x80,
//     IOMMUFD_CMD_DESTROY = IOMMUFD_CMD_BASE,
//     IOMMUFD_CMD_IOAS_ALLOC,
//     IOMMUFD_CMD_IOAS_ALLOW_IOVAS,
//     IOMMUFD_CMD_IOAS_COPY,
//     IOMMUFD_CMD_IOAS_IOVA_RANGES,
//     IOMMUFD_CMD_IOAS_MAP,
//     IOMMUFD_CMD_IOAS_UNMAP,
//     IOMMUFD_CMD_OPTION,
//     IOMMUFD_CMD_VFIO_IOAS,
//     IOMMUFD_CMD_HWPT_ALLOC,
//     IOMMUFD_CMD_GET_HW_INFO,
//     IOMMUFD_CMD_HWPT_SET_DIRTY_TRACKING,
//     IOMMUFD_CMD_HWPT_GET_DIRTY_BITMAP,
//     IOMMUFD_CMD_HWPT_INVALIDATE,
// }

const IOMMUFD_TYPE: u64 = b';' as u64;

const IOMMUFD_CMD_IOAS_ALLOC: u64 = 0x81;
const IOMMUFD_CMD_IOAS_MAP: u64 = 0x85;

pub const IOMMU_IOAS_ALLOC: u64 = _IO(IOMMUFD_TYPE, IOMMUFD_CMD_IOAS_ALLOC);
pub const IOMMU_IOAS_MAP: u64 = _IO(IOMMUFD_TYPE, IOMMUFD_CMD_IOAS_MAP);

pub const IOMMU_IOAS_MAP_FIXED_IOVA: u32 = 1 << 0;
pub const IOMMU_IOAS_MAP_WRITEABLE: u32 = 1 << 1;
pub const IOMMU_IOAS_MAP_READABLE: u32 = 1 << 1;
