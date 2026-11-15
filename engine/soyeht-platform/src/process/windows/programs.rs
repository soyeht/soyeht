//! The processes of a job, as [`Program`]s in tree order: what a pane on Windows runs, as the
//! pseudo console has no foreground process group to ask.

use super::{Spawned, check, owned};
use crate::process::inspect::{
    Listed, Program, bounded, bounded_executable, split_command_line, tree_order,
};
use std::{io, os::windows::io::AsRawHandle, ptr};
use windows_sys::{
    Wdk::System::Threading::NtQueryInformationProcess,
    Win32::{
        Foundation::{
            ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, FILETIME, HANDLE,
            STATUS_INFO_LENGTH_MISMATCH,
        },
        System::{
            JobObjects::{
                JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList,
                QueryInformationJobObject,
            },
            Threading::{
                GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    },
};

/// `ProcessBasicInformation` of `NtQueryInformationProcess`.
const BASIC_INFORMATION: i32 = 0;
/// `ProcessCommandLineInformation`: the command line, from Windows 8.1.
const COMMAND_LINE_INFORMATION: i32 = 60;
/// The most process ids of a job that are read at all: a job with more (a large parallel build)
/// is read in part, so the cost of a read never grows with the job. Each id is a handle opened
/// and three questions asked.
const MAX_READ: usize = 256;
/// The first room for a file name, in UTF-16 units; a longer name is asked again with more.
const FIRST_PATH: usize = 1024;
/// The longest path the long-path form of a Windows file can have, in UTF-16 units.
const LONGEST_PATH: usize = 32_768;

/// `PROCESS_BASIC_INFORMATION`, as the system lays it out. Only the parent is read.
#[repr(C)]
struct BasicInformation {
    exit_status: i32,
    peb: usize,
    affinity: usize,
    priority: i32,
    unique_process_id: usize,
    parent_process_id: usize,
}

impl Spawned {
    /// The processes of the job, a process before what it started, so the caller finds the
    /// program a person runs among the shells and launchers above it. A process that ends while
    /// the job is read, or that this one may not read (an elevated one), is left out.
    ///
    /// # Errors
    /// The system's error when the job cannot be asked.
    pub fn programs(&self) -> io::Result<Vec<Program>> {
        let listed = self
            .job
            .process_ids()?
            .into_iter()
            .filter_map(read_process)
            .collect();
        Ok(tree_order(listed))
    }
}

impl super::Job {
    /// The ids of the processes the job holds, at most [`MAX_READ`] of them.
    fn process_ids(&self) -> io::Result<Vec<u32>> {
        // The header is two counts; the room is in pointer-sized words, which also aligns it.
        let words = MAX_READ + 2;
        let mut buffer = vec![0usize; words];
        // SAFETY: the job handle is open, and the buffer is `words` pointer-sized words,
        // which the call fills with the structure the class names.
        let queried = unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                u32::try_from(words * size_of::<usize>()).unwrap_or(u32::MAX),
                ptr::null_mut(),
            )
        };
        // A job with more ids than the room answers `ERROR_MORE_DATA` with the ones that fit.
        match check(queried) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(ERROR_MORE_DATA.cast_signed()) => {}
            Err(error) => return Err(error),
        }
        let list = buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        // SAFETY: the buffer holds the header the call wrote, and the ids it counts follow it
        // within the buffer.
        let (count, first) = unsafe {
            (
                (*list).NumberOfProcessIdsInList as usize,
                ptr::addr_of!((*list).ProcessIdList).cast::<usize>(),
            )
        };
        let count = count.min(MAX_READ);
        let mut ids = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: `index` is below the count of ids the call wrote into the buffer.
            let id = unsafe { *first.add(index) };
            if let Ok(id) = u32::try_from(id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }
}

/// One process of the job, or `None` when it ended or may not be read.
fn read_process(pid: u32) -> Option<Listed> {
    // SAFETY: the call returns an owned handle or null; `owned` takes it over.
    let opened = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    // ignored: a process that ended, or that this one may not open (an elevated one), is not
    // one the pane's person started from a shell of this account, so it is left out
    let process = opened.ok()?;
    let handle = process.as_raw_handle();
    let created = creation_time(handle)?;
    Some(Listed {
        program: Program {
            pid,
            parent: None,
            executable: bounded_executable(image_path(handle)),
            arguments: bounded(command_line(handle).unwrap_or_default()),
        },
        parent_pid: parent_of(handle).unwrap_or(0),
        created,
    })
}

fn creation_time(handle: HANDLE) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the process handle is open and the four out pointers live for the call.
    let read = check(unsafe {
        GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
    });
    // ignored: a process that ended while the job was read has no age, so it is left out
    read.ok()?;
    Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn image_path(handle: HANDLE) -> Option<String> {
    let mut room = FIRST_PATH;
    loop {
        let mut path = vec![0u16; room];
        let mut length = u32::try_from(path.len()).ok()?;
        // SAFETY: the buffer is `length` units, which the call updates to the units it wrote.
        let read = check(unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut length)
        });
        match read {
            Ok(()) => return Some(String::from_utf16_lossy(path.get(..length as usize)?)),
            // A name longer than the room: ask again with more, up to the longest there is.
            Err(error)
                if error.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER.cast_signed())
                    && room < LONGEST_PATH =>
            {
                room = (room * 4).min(LONGEST_PATH);
            }
            // ignored: a process whose file the system does not name still has its arguments
            // to go by
            Err(_) => return None,
        }
    }
}

fn parent_of(handle: HANDLE) -> Option<u32> {
    let mut information = BasicInformation {
        exit_status: 0,
        peb: 0,
        affinity: 0,
        priority: 0,
        unique_process_id: 0,
        parent_process_id: 0,
    };
    // SAFETY: the buffer is the structure the class names, of the size passed.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            BASIC_INFORMATION,
            ptr::from_mut(&mut information).cast(),
            size_of::<BasicInformation>() as u32,
            ptr::null_mut(),
        )
    };
    (status >= 0)
        .then(|| u32::try_from(information.parent_process_id).ok())
        .flatten()
}

/// The arguments of the process's command line, split as its own runtime splits them.
fn command_line(handle: HANDLE) -> Option<Vec<String>> {
    let mut needed = 0u32;
    // SAFETY: a zero-length buffer only asks how long the answer is.
    let asked = unsafe {
        NtQueryInformationProcess(
            handle,
            COMMAND_LINE_INFORMATION,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    // Any other status means this system does not give the command line of this process.
    if asked != STATUS_INFO_LENGTH_MISMATCH || needed == 0 {
        return None;
    }
    // Pointer-sized words keep the buffer aligned for the string header at its start.
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0usize; words];
    // SAFETY: the buffer is at least `needed` bytes, which the call fills.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            COMMAND_LINE_INFORMATION,
            buffer.as_mut_ptr().cast(),
            u32::try_from(words * size_of::<usize>()).ok()?,
            &mut needed,
        )
    };
    if status < 0 {
        return None;
    }
    let start = buffer.as_ptr() as usize;
    let end = start + words * size_of::<usize>();
    // SAFETY: the answer is a `UNICODE_STRING` at the start of the buffer.
    let text = unsafe {
        ptr::read(
            buffer
                .as_ptr()
                .cast::<windows_sys::Win32::Foundation::UNICODE_STRING>(),
        )
    };
    let units = usize::from(text.Length) / 2;
    let at = text.Buffer as usize;
    // The string lives in the same buffer; one that points elsewhere is not read.
    if at < start || at + units * 2 > end || !at.is_multiple_of(2) {
        return None;
    }
    // SAFETY: the range was checked to lie inside the buffer, and it is 2-byte aligned.
    let line = unsafe { std::slice::from_raw_parts(text.Buffer, units) };
    Some(split_command_line(line))
}
