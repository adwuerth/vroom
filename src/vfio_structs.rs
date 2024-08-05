#![allow(non_camel_case_types, unused)]

/// struct `vfio_iommu_type1_dma_map`, grabbed from linux/vfio.h

#[derive(Debug)]
#[repr(C)]
pub struct vfio_iommu_type1_dma_map {
    pub argsz: u32,
    pub flags: u32,
    pub vaddr: u64,
    pub iova: u64,
    pub size: usize,
}

/// struct `vfio_iommu_type1_dma_unmap`, grabbed from linux/vfio.h
#[derive(Debug)]
#[repr(C)]
pub struct vfio_iommu_type1_dma_unmap {
    pub argsz: u32,
    pub flags: u32,
    pub iova: *mut u8,
    pub size: usize,
    pub data: *mut libc::c_void,
}

/// struct `vfio_group_status`, grabbed from linux/vfio.h
#[repr(C)]
pub struct vfio_group_status {
    pub argsz: u32,
    pub flags: u32,
}

/// struct `vfio_region_info`, grabbed from linux/vfio.h
#[repr(C)]
pub struct vfio_region_info {
    pub argsz: u32,
    pub flags: u32,
    pub index: u32,
    pub cap_offset: u32,
    pub size: u64,
    pub offset: u64,
}

/// struct `vfio_iommu_type1_info`, grabbed from linux/vfio.h
#[repr(C)]
pub struct vfio_iommu_type1_info {
    pub argsz: u32,
    pub flags: u32,
    pub iova_pgsizes: u64,
    pub cap_offset: u32,
    pub pad: u32,
}

/// struct `vfio_irq_set`, grabbed from linux/vfio.h
///
/// As this is a dynamically sized struct (has an array at the end) we need to use
/// Dynamically Sized Types (DSTs) which can be found at
/// https://doc.rust-lang.org/nomicon/exotic-sizes.html#dynamically-sized-types-dsts
#[repr(C)]
pub struct vfio_irq_set<T: ?Sized> {
    pub argsz: u32,
    pub flags: u32,
    pub index: u32,
    pub start: u32,
    pub count: u32,
    pub data: T,
}

/// struct `vfio_irq_info`, grabbed from linux/vfio.h
#[repr(C)]
pub struct vfio_irq_info {
    pub argsz: u32,
    pub flags: u32,
    pub index: u32,
    /* IRQ index */
    pub count: u32,
    /* Number of IRQs within this index */
}

#[repr(C)]
pub struct iommu_ioas_map {
    pub size: u32,
    pub flags: u32,
    pub ioas_id: u32,
    pub __reserved: u32,
    pub user_va: u64,
    pub length: u64,
    pub iova: u64,
}

#[repr(C)]
pub struct iommu_ioas_unmap {
    pub size: u32,
    pub ioas_id: u32,
    pub iova: u64,
    pub length: u64,
}

#[repr(C)]
pub struct vfio_device_bind_iommufd {
    pub argsz: u32,
    pub flags: u32,
    pub iommufd: i32,
    pub out_devid: u32,
}

#[repr(C)]
pub struct iommu_ioas_alloc {
    pub size: u32,
    pub flags: u32,
    pub out_ioas_id: u32,
}

#[repr(C)]
pub struct vfio_device_attach_iommufd_pt {
    pub argsz: u32,
    pub flags: u32,
    pub pt_id: u32,
}
