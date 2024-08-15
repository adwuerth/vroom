#![allow(non_camel_case_types, clippy::identity_op)]

use std::fmt;

#[derive(Debug)]
pub enum IoctlOp {
    // VFIO constants
    VFIO_GET_API_VERSION,
    VFIO_CHECK_EXTENSION,
    VFIO_SET_IOMMU,
    VFIO_GROUP_GET_STATUS,
    VFIO_GROUP_SET_CONTAINER,
    VFIO_GROUP_GET_DEVICE_FD,
    VFIO_DEVICE_GET_REGION_INFO,
    VFIO_IOMMU_GET_INFO,
    VFIO_IOMMU_MAP_DMA,
    VFIO_IOMMU_UNMAP_DMA,

    // constants needed for IOMMU Interrupts.
    #[allow(unused)]
    VFIO_DEVICE_GET_IRQ_INFO,

    #[allow(unused)]
    VFIO_DEVICE_SET_IRQS,

    // VFIO IOMMUFD constants
    VFIO_DEVICE_BIND_IOMMUFD,
    VFIO_DEVICE_ATTACH_IOMMUFD_PT,

    // IOMMUFD constants
    IOMMU_IOAS_ALLOC,
    IOMMU_IOAS_MAP,
    IOMMU_IOAS_UNMAP,
}

impl IoctlOp {
    const _IOC_TYPESHIFT: u64 = 8;
    const _IOC_NRSHIFT: u64 = 0;

    // _IO macro from ioctl.h, _IOC is inlined, as only _IO is needed
    #[allow(non_snake_case)]
    const fn _IO(type_: u64, nr: u64) -> u64 {
        type_ << Self::_IOC_TYPESHIFT | nr << Self::_IOC_NRSHIFT
    }

    // constants needed for IOMMU. Grabbed from linux/vfio.h
    const VFIO_TYPE: u64 = b';' as u64;
    const VFIO_BASE: u64 = 100;

    const IOMMUFD_TYPE: u64 = b';' as u64;

    // these are enum values in iommufd.h
    const IOMMUFD_CMD_IOAS_ALLOC: u64 = 0x81;
    const IOMMUFD_CMD_IOAS_MAP: u64 = 0x85;
    const IOMMUFD_CMD_IOAS_UNMAP: u64 = 0x86;

    pub const fn op(&self) -> u64 {
        let (type_, nr) = match self {
            Self::VFIO_GET_API_VERSION => (Self::VFIO_TYPE, Self::VFIO_BASE + 0),
            Self::VFIO_CHECK_EXTENSION => (Self::VFIO_TYPE, Self::VFIO_BASE + 1),
            Self::VFIO_SET_IOMMU => (Self::VFIO_TYPE, Self::VFIO_BASE + 2),
            Self::VFIO_GROUP_GET_STATUS => (Self::VFIO_TYPE, Self::VFIO_BASE + 3),
            Self::VFIO_GROUP_SET_CONTAINER => (Self::VFIO_TYPE, Self::VFIO_BASE + 4),
            Self::VFIO_GROUP_GET_DEVICE_FD => (Self::VFIO_TYPE, Self::VFIO_BASE + 6),
            Self::VFIO_DEVICE_GET_REGION_INFO => (Self::VFIO_TYPE, Self::VFIO_BASE + 8),
            Self::VFIO_IOMMU_GET_INFO => (Self::VFIO_TYPE, Self::VFIO_BASE + 12),
            Self::VFIO_IOMMU_MAP_DMA => (Self::VFIO_TYPE, Self::VFIO_BASE + 13),
            Self::VFIO_IOMMU_UNMAP_DMA => (Self::VFIO_TYPE, Self::VFIO_BASE + 14),
            Self::VFIO_DEVICE_BIND_IOMMUFD => (Self::VFIO_TYPE, Self::VFIO_BASE + 18),
            Self::VFIO_DEVICE_ATTACH_IOMMUFD_PT => (Self::VFIO_TYPE, Self::VFIO_BASE + 19),
            Self::VFIO_DEVICE_GET_IRQ_INFO => (Self::VFIO_TYPE, Self::VFIO_BASE + 9),
            Self::VFIO_DEVICE_SET_IRQS => (Self::VFIO_TYPE, Self::VFIO_BASE + 10),

            Self::IOMMU_IOAS_ALLOC => (Self::IOMMUFD_TYPE, Self::IOMMUFD_CMD_IOAS_ALLOC),
            Self::IOMMU_IOAS_MAP => (Self::IOMMUFD_TYPE, Self::IOMMUFD_CMD_IOAS_MAP),
            Self::IOMMU_IOAS_UNMAP => (Self::IOMMUFD_TYPE, Self::IOMMUFD_CMD_IOAS_UNMAP),
        };

        Self::_IO(type_, nr)
    }
}

impl fmt::Display for IoctlOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub enum IoctlFlag {}

impl IoctlFlag {
    pub const VFIO_GROUP_FLAGS_VIABLE: u32 = 1 << 0;

    pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
    pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;

    #[allow(unused)]
    pub const VFIO_IRQ_SET_DATA_NONE: u32 = 1 << 0; /* Data not present */

    #[allow(unused)]
    pub const VFIO_IRQ_SET_DATA_EVENTFD: u32 = 1 << 2; /* Data is eventfd (s32) */

    #[allow(unused)]
    pub const VFIO_IRQ_SET_ACTION_TRIGGER: u32 = 1 << 5; /* Trigger interrupt */

    #[allow(unused)]
    pub const IOMMU_IOAS_MAP_FIXED_IOVA: u32 = 1 << 0;
    pub const IOMMU_IOAS_MAP_WRITEABLE: u32 = 1 << 1;
    pub const IOMMU_IOAS_MAP_READABLE: u32 = 1 << 1;
}
