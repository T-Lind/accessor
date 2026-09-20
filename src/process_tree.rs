//! Scope harness children and their tools to the lifetime of their owner.
use anyhow::{Context, Result};
use tokio::process::{Child, Command};

pub fn spawn(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    #[cfg(unix)]
    command.process_group(0);
    command.spawn()
}

pub struct ProcessTree {
    handle: isize,
}

impl ProcessTree {
    pub fn attach(child: &Child) -> Result<Self> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::JobObjects::*;
            // SAFETY: the job is owned here; pointers refer to a live C structure
            // and the live child's process handle for the duration of each call.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                anyhow::ensure!(
                    !job.is_null(),
                    "Cannot create harness process group: {}",
                    std::io::Error::last_os_error()
                );
                let tree = Self {
                    handle: job as isize,
                };
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                anyhow::ensure!(
                    SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const _,
                        std::mem::size_of_val(&info) as u32
                    ) != 0,
                    "Cannot configure harness process group: {}",
                    std::io::Error::last_os_error()
                );
                let process = child
                    .raw_handle()
                    .context("Harness process handle unavailable")?;
                anyhow::ensure!(
                    AssignProcessToJobObject(job, process) != 0,
                    "Cannot attach harness process group: {}",
                    std::io::Error::last_os_error()
                );
                Ok(tree)
            }
        }
        #[cfg(unix)]
        {
            Ok(Self {
                handle: child.id().context("Harness process ID unavailable")? as isize,
            })
        }
    }
    pub fn stop(&mut self) {
        if self.handle == 0 {
            return;
        }
        #[cfg(windows)]
        // SAFETY: this is our uniquely owned job handle, closed once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle as _);
        }
        #[cfg(unix)]
        // SAFETY: this group was created for our child by spawn above.
        unsafe {
            libc::kill(-(self.handle as i32), libc::SIGKILL);
        }
        self.handle = 0;
    }
}
impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.stop();
    }
}
