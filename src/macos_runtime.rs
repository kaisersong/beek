//! Native resources used by long-lived Rust threads on macOS.
use cocoa::base::{id, nil};
use cocoa::foundation::NSAutoreleasePool;
use std::{fs::File, marker::PhantomData, rc::Rc};

/// Drain on every exit from a polling iteration, including `continue` and panic.
/// A pool belongs to its creating thread and must never cross an async suspension.
pub struct AutoreleasePool {
    pool: id,
    _thread_bound: PhantomData<Rc<()>>,
}

impl AutoreleasePool {
    pub fn new() -> Self {
        Self {
            pool: unsafe { NSAutoreleasePool::new(nil) },
            _thread_bound: PhantomData,
        }
    }
}

impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        unsafe { self.pool.drain() }
    }
}

/// The open file owns the OS lock; exiting or crashing releases it automatically.
/// Keep the file on disk: unlinking it would let another process lock a new inode.
pub fn lock_instance(path: &std::path::Path) -> std::io::Result<File> {
    let file = File::options().create(true).truncate(false).write(true).open(path)?;
    file.try_lock().map_err(std::io::Error::from)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::{base::TCFType, string::CFString};
    use objc::{msg_send, sel, sel_impl};

    fn add_autoreleased_reference(value: &CFString) {
        unsafe {
            let object = value.as_CFTypeRef() as id;
            let retained: id = msg_send![object, retain];
            let _: id = msg_send![retained, autorelease];
        }
    }

    #[test]
    fn polling_pool_drains_on_continue_and_early_return() {
        std::thread::spawn(|| {
            let value = CFString::new(&"BeeK pool regression fixture ".repeat(20));
            let baseline = value.retain_count();
            for _ in 0..1000 {
                let _pool = AutoreleasePool::new();
                add_autoreleased_reference(&value);
                assert_eq!(value.retain_count(), baseline + 1);
                continue;
            }
            assert_eq!(value.retain_count(), baseline);
            let early_return = || -> Option<()> {
                let _pool = AutoreleasePool::new();
                add_autoreleased_reference(&value);
                None?;
                Some(())
            };
            assert_eq!(early_return(), None);
            assert_eq!(value.retain_count(), baseline);
        }).join().unwrap();
    }

    #[test]
    fn instance_lock_rejects_duplicate_and_releases_on_exit() {
        let path = std::env::temp_dir().join(format!("beek-instance-test-{}.lock", std::process::id()));
        let first = lock_instance(&path).unwrap();
        assert!(lock_instance(&path).is_err());
        drop(first);
        assert!(lock_instance(&path).is_ok());
        std::fs::remove_file(path).unwrap();
    }
}
