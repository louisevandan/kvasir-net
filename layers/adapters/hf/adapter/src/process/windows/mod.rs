//! A suspended launcher joins its owned job before any Python child can start.
use std::{
    io,
    mem::{size_of, zeroed},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
};

pub struct Group(OwnedHandle);
fn error() -> String {
    io::Error::last_os_error().to_string()
}
impl Group {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if raw.is_null() {
                return Err(error());
            }
            let owned = Self(OwnedHandle::from_raw_handle(raw));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                owned.raw(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(error());
            }
            Ok(owned)
        }
    }
    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }
    pub fn attach_and_resume(&self, child: &tokio::process::Child) -> Result<(), String> {
        let pid = child.id().ok_or("missing suspended child")?;
        unsafe {
            if AssignProcessToJobObject(
                self.raw(),
                child.raw_handle().ok_or("missing child handle")?,
            ) == 0
            {
                return Err(error());
            }
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(error());
            }
            let snapshot = OwnedHandle::from_raw_handle(snapshot);
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut found = Thread32First(snapshot.as_raw_handle(), &mut entry);
            while found != 0 {
                if entry.th32OwnerProcessID == pid {
                    let raw = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if raw.is_null() {
                        return Err(error());
                    }
                    let thread = OwnedHandle::from_raw_handle(raw);
                    if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                        return Err(error());
                    }
                    return Ok(());
                }
                found = Thread32Next(snapshot.as_raw_handle(), &mut entry);
            }
            Err("suspended child has no main thread".into())
        }
    }
    pub fn kill(&self) -> Result<(), String> {
        if unsafe { TerminateJobObject(self.raw(), 1) } == 0 {
            Err(error())
        } else {
            Ok(())
        }
    }
    pub async fn drained(&self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let active = unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
                if QueryInformationJobObject(
                    self.raw(),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as _,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                ) == 0
                {
                    return Err(error());
                }
                info.ActiveProcesses
            };
            if active == 0 {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("owned process tree cleanup timeout".into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}
