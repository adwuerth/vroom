#[macro_export]
macro_rules! ioctl_unsafe {
    ($fd:expr, $op:expr, $arg:expr) => {{
        let op = $op.op();
        let result = unsafe { libc::ioctl($fd, op, $arg) };
        if result == -1 {
            Err(Error::Ioctl {
                error: (format!("{}", $op)),
                io_error: (std::io::Error::last_os_error()),
            })
        } else {
            Ok(result)
        }
    }};
    ($fd:expr, $op:expr) => {{
        let op = $op.op();
        let result = unsafe { libc::ioctl($fd, op) };
        if result == -1 {
            Err(Error::Ioctl {
                error: (format!("{}", $op)),
                io_error: (std::io::Error::last_os_error()),
            })
        } else {
            Ok(result)
        }
    }};
}

#[macro_export]
macro_rules! mmap_unsafe {
    ($addr:expr, $len:expr, $prot:expr, $flags:expr, $fd:expr, $offset:expr) => {{
        let ptr = unsafe { libc::mmap($addr, $len, $prot, $flags, $fd, $offset) };
        if ptr == libc::MAP_FAILED {
            Err(Error::Mmap {
                error: (format!("mmap with len {} failed", $len)),
                io_error: (std::io::Error::last_os_error()),
            })
        } else {
            Ok(ptr)
        }
    }};
}

#[macro_export]
macro_rules! munmap_unsafe {
    ($addr:expr, $len:expr) => {{
        let result = unsafe { libc::munmap($addr, $len) };
        if result == -1 {
            Err(Error::Mmap {
                error: (format!("munmap with len {} failed", $len)),
                io_error: (std::io::Error::last_os_error()),
            })
        } else {
            Ok(result)
        }
    }};
}

#[macro_export]
macro_rules! mmap_anonymous_unsafe {
    ($len:expr, $flags:expr) => {
        mmap_unsafe!(
            ptr::null_mut(),
            $len,
            libc::PROT_READ | libc::PROT_WRITE,
            $flags | libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_POPULATE,
            -1,
            0
        )
    };
    ($len:expr) => {
        mmap_anonymous_unsafe!($len, 0)
    };
}

#[macro_export]
macro_rules! mmap_fd_unsafe {
    ($len:expr, $flags:expr, $fd:expr) => {
        mmap_unsafe!(
            ptr::null_mut(),
            $len,
            libc::PROT_READ | libc::PROT_WRITE,
            $flags | libc::MAP_SHARED,
            $fd,
            0
        )
    };
    ($len:expr, $fd:expr) => {
        mmap_fd_unsafe!($len, 0, $fd)
    };
}

#[macro_export]
macro_rules! mlock_unsafe {
    ($addr:expr, $len:expr) => {{
        let result = unsafe { libc::mlock($addr, $len) };
        if result == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }};
}

#[macro_export]
macro_rules! munlock_unsafe {
    ($addr:expr, $len:expr) => {{
        let result = unsafe { libc::munlock($addr, $len) };
        if result == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }};
}

#[macro_export]
macro_rules! pread_unsafe {
    ($fd:expr, $buf:expr, $count:expr, $offset:expr) => {{
        if unsafe { libc::pread($fd, $buf, $count, $offset) } == -1 {
            Err(Error::Io(std::io::Error::new(
                std::io::Error::last_os_error().kind(),
                format!("pread failed {}", std::io::Error::last_os_error()),
            )))
        } else {
            Ok(())
        }
    }};
}

#[macro_export]
macro_rules! pwrite_unsafe {
    ($fd:expr, $buf:expr, $count:expr, $offset:expr) => {{
        if unsafe { libc::pwrite($fd, $buf, $count, $offset) } == -1 {
            Err(Error::Io(std::io::Error::new(
                std::io::Error::last_os_error().kind(),
                format!("pwrite failed {}", std::io::Error::last_os_error()),
            )))
        } else {
            Ok(())
        }
    }};
}
