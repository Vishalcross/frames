//! Thread priorities for work the user is not waiting on, after Apple's
//! Energy Efficiency Guide: imports are "utility" (long, with visible
//! progress), thumbnail backfill and sync are "background" (indexing and
//! syncing). The scheduler then favours the UI and, on Apple silicon, runs
//! this work on the efficiency cores. Windows gets the closest equivalents.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkClass {
    /// Long work with visible progress, such as an import.
    Utility,
    /// Maintenance nobody is watching: thumbnail backfill, sync, trash purge.
    Background,
}

thread_local! {
    static CURRENT: std::cell::Cell<Option<WorkClass>> = const { std::cell::Cell::new(None) };
}

/// The class set on this thread, if any; tools it launches run at it too.
pub fn current() -> Option<WorkClass> {
    CURRENT.with(|c| c.get())
}

/// Lower the calling thread's priority for the rest of its life. Call it only
/// on threads this app spawns for the work, never on pooled runtime threads,
/// which would keep the lower priority for unrelated work later.
pub fn set_current_thread(class: WorkClass) {
    CURRENT.with(|c| c.set(Some(class)));
    #[cfg(target_os = "macos")]
    {
        let qos = match class {
            WorkClass::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
            WorkClass::Background => libc::qos_class_t::QOS_CLASS_BACKGROUND,
        };
        // SAFETY: only changes the scheduling class of the calling thread.
        unsafe {
            libc::pthread_set_qos_class_self_np(qos, 0);
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Background mode also lowers the thread's disk and memory priority.
        const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
        const THREAD_MODE_BACKGROUND_BEGIN: i32 = 0x0001_0000;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentThread() -> *mut core::ffi::c_void;
            fn SetThreadPriority(thread: *mut core::ffi::c_void, priority: i32) -> i32;
        }
        let priority = match class {
            WorkClass::Utility => THREAD_PRIORITY_BELOW_NORMAL,
            WorkClass::Background => THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: the pseudo-handle from GetCurrentThread is always valid.
        unsafe {
            SetThreadPriority(GetCurrentThread(), priority);
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = class;
}

/// Run `work` on the calling thread at `class`. For the body of a thread
/// spawned for that work: `s.spawn(|| qos::run_as(WorkClass::Utility, || ...))`.
pub fn run_as<T>(class: WorkClass, work: impl FnOnce() -> T) -> T {
    set_current_thread(class);
    work()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn background_threads_run_at_the_requested_qos() {
        let observed = std::thread::spawn(|| {
            set_current_thread(WorkClass::Background);
            let mut qos = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: reads the calling thread's own scheduling class.
            unsafe {
                libc::pthread_get_qos_class_np(libc::pthread_self(), &mut qos, &mut relative);
            }
            qos
        })
        .join()
        .unwrap();
        assert_eq!(observed as u32, libc::qos_class_t::QOS_CLASS_BACKGROUND as u32);
    }
}
