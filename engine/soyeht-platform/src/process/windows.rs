//! Starting a process on Windows: jobs, pseudo consoles and the one `CreateProcessW` call that
//! every child the engine starts goes through, and ending a child with everything it started.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fmt, io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::Command,
    ptr,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_ACCESS_DENIED, HANDLE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::{
        Console::{
            COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
            SetConsoleCtrlHandler,
        },
        JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Pipes::CreatePipe,
        Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
            GetExitCodeProcess, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
            STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

/// How often an ending job is asked whether it is empty.
const EMPTY_POLL: Duration = Duration::from_millis(5);

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Takes ownership of a handle a Windows call returned, or reports its failure.
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call returned a new handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// A pipe: `(read end, write end)`, neither inheritable.
pub fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write): (HANDLE, HANDLE) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: both out pointers live for the call; default security and buffer size.
    check(unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) })?;
    Ok((owned(read)?, owned(write)?))
}

/// A copy of `handle` that a child started with an explicit handle list can inherit. Only the
/// copy is inheritable, and only until it is dropped after the launch: the original, and every
/// other handle of the supervisor, never becomes inheritable.
fn inheritable_copy(handle: &impl AsRawHandle) -> io::Result<OwnedHandle> {
    let mut copy: HANDLE = ptr::null_mut();
    // SAFETY: the source is open for the call, both process handles are the current process's
    // pseudo handle, and `copy` is an out pointer.
    check(unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle.as_raw_handle(),
            GetCurrentProcess(),
            &mut copy,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    })?;
    owned(copy)
}

/// A job object: everything started in it ends when the job is terminated or when the last
/// handle to it closes, which is when whoever owns the job ends or drops it, unless
/// [`Spawned::release`] lifted that. Each child has a job of its own.
pub struct Job(OwnedHandle);

impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: null attributes and name create an anonymous job.
        let job = Self(owned(unsafe {
            CreateJobObjectW(ptr::null(), ptr::null())
        })?);
        job.set_limit_flags(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)?;
        Ok(job)
    }

    fn set_limit_flags(&self, flags: u32) -> io::Result<()> {
        // SAFETY: the structure is plain data and zero is a valid value for every field.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = flags;
        // SAFETY: the job handle is open and `limits` is the structure the class names.
        check(unsafe {
            SetInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        })
    }

    /// Ends every process in the job. Windows refuses with `ERROR_ACCESS_DENIED` when the
    /// job's processes are already ending; any other failure means they may still run.
    fn terminate(&self) -> io::Result<()> {
        // SAFETY: the job handle is open for the call.
        check(unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) })
    }

    fn active_processes(&self) -> io::Result<u32> {
        // SAFETY: the structure is plain data and zero is a valid value for every field.
        let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the job handle is open and `accounting` is the structure the class names.
        check(unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                ptr::from_mut(&mut accounting).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        })?;
        Ok(accounting.ActiveProcesses)
    }

    /// Waits up to `limit` for the job to hold no process. Ending a job is asynchronous: its
    /// processes still hold their working directories and files for a moment afterwards.
    fn wait_empty(&self, limit: Duration) -> io::Result<bool> {
        let deadline = Instant::now() + limit;
        loop {
            if self.active_processes()? == 0 {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            thread::sleep(EMPTY_POLL);
        }
    }
}

/// A process started inside its own job: the process handle together with the job that holds
/// it and everything it starts. The one place that knows how to wait for the process and how
/// to end it with its tree, for every child the engine starts.
pub struct Spawned {
    process: OwnedHandle,
    job: Job,
}

impl Spawned {
    pub fn new(process: OwnedHandle, job: Job) -> Self {
        Self { process, job }
    }

    /// Waits up to `milliseconds` (`INFINITE` for no limit) for the process to end. Its exit
    /// code, or `None` while it still runs.
    pub fn wait_exit(&self, milliseconds: u32) -> io::Result<Option<u32>> {
        // SAFETY: the process handle is open for the whole call.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), milliseconds) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Ok(None),
            _ => return Err(io::Error::last_os_error()),
        }
        let mut code = 0;
        // SAFETY: the process has ended and `code` is an out pointer.
        check(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) })?;
        Ok(Some(code))
    }

    /// Ends the process and everything it started, the Windows form of killing a process
    /// group, and waits up to `limit` until none of it is left. When the job refuses (its
    /// processes are already ending, or none is left) the process itself is ended, and one
    /// that already ended is the outcome that was wanted.
    pub fn kill_tree(&self, limit: Duration) -> io::Result<()> {
        match self.job.terminate() {
            Ok(()) => {}
            Err(error) if is_access_denied(&error) => end_process(&self.process)?,
            Err(error) => {
                end_process(&self.process)?;
                return Err(error);
            }
        }
        if self.job.wait_empty(limit)? {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "processes of the job were still running after being ended",
            ))
        }
    }

    /// Lets what is left in the job outlive the job's last handle. A bounded command that
    /// finished normally calls it, so a background process it deliberately left running (an
    /// agent a tool started) survives, as it does on Unix.
    pub fn release(&self) -> io::Result<()> {
        self.job.set_limit_flags(0)
    }
}

fn end_process(process: &OwnedHandle) -> io::Result<()> {
    // SAFETY: the process handle is open for the call.
    match check(unsafe { TerminateProcess(process.as_raw_handle(), 1) }) {
        Err(error) if !is_access_denied(&error) => Err(error),
        _ => Ok(()),
    }
}

fn is_access_denied(error: &io::Error) -> bool {
    error.raw_os_error().map(i32::cast_unsigned) == Some(ERROR_ACCESS_DENIED)
}

/// A pseudo console: the terminal a child on Windows writes to and reads from.
pub struct Console(Mutex<Option<HPCON>>);

// SAFETY: a pseudo console handle is an opaque token the system lets any thread use; the mutex
// serialises resizing against closing.
unsafe impl Send for Console {}
// SAFETY: as above.
unsafe impl Sync for Console {}

impl Console {
    /// Creates a console of `rows` x `cols` that reads `input` and writes `output`.
    pub fn new(
        rows: u16,
        cols: u16,
        input: &OwnedHandle,
        output: &OwnedHandle,
    ) -> io::Result<Self> {
        let mut console: HPCON = 0;
        // SAFETY: both handles are open pipe ends for the call and `console` is an out pointer.
        let result = unsafe {
            CreatePseudoConsole(
                size(cols, rows),
                input.as_raw_handle(),
                output.as_raw_handle(),
                0,
                &mut console,
            )
        };
        if result < 0 {
            return Err(io::Error::other(format!(
                "pseudo console error {result:#x}"
            )));
        }
        Ok(Self(Mutex::new(Some(console))))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let guard = self
            .0
            .lock()
            .map_err(|_| io::Error::other("console lock poisoned"))?;
        let Some(console) = *guard else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "terminal is closed",
            ));
        };
        // SAFETY: the console handle stays open while the lock is held.
        let result = unsafe { ResizePseudoConsole(console, size(cols, rows)) };
        if result < 0 {
            return Err(io::Error::other(format!(
                "pseudo console error {result:#x}"
            )));
        }
        Ok(())
    }

    /// Hangs up the terminal: the program sees its console close and the output pipe ends.
    /// Closing waits until the pending output has been read, so the caller must not hold a lock
    /// that whoever reads the output needs; see [`Console::close_in_background`].
    pub fn close(&self) {
        // The handle leaves the mutex first, so a resize is never blocked behind the close.
        let console = match self.0.lock() {
            Ok(mut guard) => guard.take(),
            // A resize that panicked while holding the lock leaves the handle in place; take it
            // so it is still closed.
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(console) = console {
            // SAFETY: taken out of the option, so it is closed exactly once.
            unsafe { ClosePseudoConsole(console) };
        }
    }

    /// As [`Console::close`], on a thread of its own, for callers that hold a lock.
    pub fn close_in_background(self: &Arc<Self>) {
        let console = Arc::clone(self);
        let started = thread::Builder::new()
            .name("console-close".into())
            .spawn(move || console.close());
        if started.is_err() {
            // No thread to spare: closing here is slower for the caller but still correct.
            self.close();
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.close();
    }
}

fn size(cols: u16, rows: u16) -> COORD {
    COORD {
        X: i16::try_from(cols).unwrap_or(i16::MAX),
        Y: i16::try_from(rows).unwrap_or(i16::MAX),
    }
}

/// How the child's standard handles are connected when it has no pseudo console.
pub struct Stdio<'a> {
    pub input: &'a OwnedHandle,
    pub output: &'a OwnedHandle,
    pub error: &'a OwnedHandle,
}

/// What `CreateProcessW` needs besides the command.
pub struct Launch<'a> {
    pub command: &'a Command,
    pub job: &'a Job,
    pub console: Option<&'a Console>,
    pub stdio: Option<Stdio<'a>>,
    /// Whether the child also gets this process's environment, with the command's variables
    /// applied over it. Otherwise it gets exactly the command's variables.
    pub inherit_environment: bool,
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

/// Why a command line could not be made safely.
#[derive(Debug, Eq, PartialEq)]
pub struct UnsafeBatchArgument;

impl fmt::Display for UnsafeBatchArgument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "an argument for a .cmd or .bat file may not contain a quote, a percent sign or a line break",
        )
    }
}

impl std::error::Error for UnsafeBatchArgument {}

impl From<UnsafeBatchArgument> for io::Error {
    fn from(error: UnsafeBatchArgument) -> Self {
        Self::new(io::ErrorKind::InvalidInput, error)
    }
}

/// Appends `argument` quoted so `CommandLineToArgvW` reads it back unchanged.
fn push_argument(line: &mut Vec<u16>, argument: &OsStr) {
    let argument: Vec<u16> = argument.encode_wide().collect();
    let plain = !argument.is_empty()
        && !argument
            .iter()
            .any(|unit| matches!(*unit, 0x20 | 0x09 | 0x0A | 0x0B | 0x22));
    if plain {
        line.extend(argument);
        return;
    }
    line.push(u16::from(b'"'));
    let mut backslashes = 0;
    for unit in argument {
        if unit == u16::from(b'\\') {
            backslashes += 1;
            continue;
        }
        let escaped = if unit == u16::from(b'"') {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        line.extend(std::iter::repeat_n(u16::from(b'\\'), escaped));
        backslashes = 0;
        line.push(unit);
    }
    line.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
    line.push(u16::from(b'"'));
}

/// Appends `argument` for a `.cmd` or `.bat` program, which `cmd.exe` reads, not
/// `CommandLineToArgvW`. Inside double quotes `& | < > ^ ( )` are plain text to it, so the
/// argument is always quoted; a quote would end the quoting, a percent sign is expanded even
/// inside quotes, and a line break ends the command, so those arguments are refused.
fn push_batch_argument(line: &mut Vec<u16>, argument: &OsStr) -> Result<(), UnsafeBatchArgument> {
    let argument: Vec<u16> = argument.encode_wide().collect();
    if argument
        .iter()
        .any(|unit| matches!(*unit, 0x00 | 0x0A | 0x0D | 0x22 | 0x25))
    {
        return Err(UnsafeBatchArgument);
    }
    line.push(u16::from(b'"'));
    line.extend(argument);
    line.push(u16::from(b'"'));
    Ok(())
}

fn is_batch(program: &Path) -> bool {
    program.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
    })
}

/// The NUL-terminated command line that runs `program` with `arguments`.
fn command_line<'a>(
    program: &Path,
    arguments: impl Iterator<Item = &'a OsStr>,
) -> Result<Vec<u16>, UnsafeBatchArgument> {
    let batch = is_batch(program);
    let mut line = vec![u16::from(b'"')];
    line.extend(program.as_os_str().encode_wide());
    line.push(u16::from(b'"'));
    for argument in arguments {
        line.push(u16::from(b' '));
        if batch {
            push_batch_argument(&mut line, argument)?;
        } else {
            push_argument(&mut line, argument);
        }
    }
    line.push(0);
    Ok(line)
}

/// The executable to run: a path as given, or the first match for the name along the child's
/// own `PATH` (the supervisor's `PATH` when the command sets none).
fn resolve(command: &Command) -> PathBuf {
    let program = command.get_program();
    let given = Path::new(program);
    if given.components().count() > 1 || given.is_absolute() {
        return with_extension(given).unwrap_or_else(|| given.to_owned());
    }
    let path = command
        .get_envs()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .and_then(|(_, value)| value.map(OsStr::to_owned))
        .or_else(|| std::env::var_os("PATH"));
    path.iter()
        .flat_map(std::env::split_paths)
        .find_map(|directory| with_extension(&directory.join(program)))
        .unwrap_or_else(|| given.to_owned())
}

fn with_extension(path: &Path) -> Option<PathBuf> {
    if path.extension().is_some() {
        return path.is_file().then(|| path.to_owned());
    }
    ["exe", "com", "cmd", "bat"].iter().find_map(|extension| {
        let candidate = path.with_extension(extension);
        candidate.is_file().then_some(candidate)
    })
}

/// The environment block for a child: the command's explicit variables, over this process's
/// own when `inherit` is set, sorted the way Windows expects (names compare ignoring case).
fn environment(command: &Command, inherit: bool) -> Vec<u16> {
    let mut entries: BTreeMap<String, OsString> = BTreeMap::new();
    let mut put = |name: &OsStr, value: Option<&OsStr>| {
        let key = name.to_string_lossy().to_uppercase();
        match value {
            Some(value) => {
                let mut entry = name.to_owned();
                entry.push("=");
                entry.push(value);
                entries.insert(key, entry);
            }
            None => {
                entries.remove(&key);
            }
        }
    };
    if inherit {
        for (name, value) in std::env::vars_os() {
            put(&name, Some(&value));
        }
    }
    for (name, value) in command.get_envs() {
        put(name, value);
    }
    let mut block = Vec::new();
    for entry in entries.into_values() {
        block.extend(entry.encode_wide());
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    block
}

/// Makes this process, and so every process it starts after, handle Ctrl+C in the normal way.
///
/// A process started in a process group of its own, as a launcher does, ignores Ctrl+C, and a
/// child inherits that: a shell in a terminal would then end nothing on Ctrl+C. Unix has the
/// same trap with the signals a background job ignores, and a terminal's child resets them.
pub(crate) fn restore_ctrl_c() -> io::Result<()> {
    // SAFETY: a null handler with FALSE clears the flag that ignores Ctrl+C; nothing is passed
    // or kept.
    check(unsafe { SetConsoleCtrlHandler(None, 0) })
}

/// Starts the command inside the job, on the console or with the given standard handles, and
/// returns the process handle and id.
pub fn create_process(launch: &Launch<'_>) -> io::Result<(OwnedHandle, u32)> {
    let program = resolve(launch.command);
    let mut line = command_line(&program, launch.command.get_args())?;
    let mut environment = environment(launch.command, launch.inherit_environment);
    let directory = launch
        .command
        .get_current_dir()
        .map(|path| wide(path.as_os_str()));

    let job = launch.job.0.as_raw_handle();
    // Copies the child may inherit; they close when this function returns, after the launch.
    let copies = launch
        .stdio
        .as_ref()
        .map(|stdio| -> io::Result<[OwnedHandle; 3]> {
            Ok([
                inheritable_copy(stdio.input)?,
                inheritable_copy(stdio.output)?,
                inheritable_copy(stdio.error)?,
            ])
        })
        .transpose()?;
    let mut inherited: Vec<HANDLE> = copies
        .iter()
        .flatten()
        .map(AsRawHandle::as_raw_handle)
        .collect();
    let mut startup: STARTUPINFOEXW = unsafe_zeroed();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    if launch.console.is_some() {
        // A terminal's child reads Ctrl+C as the terminal's key, whatever this process was told.
        if let Err(error) = restore_ctrl_c() {
            crate::log::failure("process.ctrl_c_not_restored", &error);
        }
        // Without this flag a child whose parent has redirected standard handles writes to
        // them instead of to the pseudo console. Null handles hand it the console's own.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    }
    if let Some([input, output, error]) = &copies {
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = input.as_raw_handle();
        startup.StartupInfo.hStdOutput = output.as_raw_handle();
        startup.StartupInfo.hStdError = error.as_raw_handle();
    }

    let attributes = AttributeList::new(2 + usize::from(launch.stdio.is_some()))?;
    startup.lpAttributeList = attributes.pointer();
    attributes.set(
        PROC_THREAD_ATTRIBUTE_JOB_LIST,
        ptr::from_ref(&job).cast(),
        size_of::<HANDLE>(),
    )?;
    if let Some(console) = launch.console {
        let guard = console
            .0
            .lock()
            .map_err(|_| io::Error::other("console lock poisoned"))?;
        let handle = guard.ok_or_else(|| io::Error::other("terminal is closed"))?;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            handle as *const _,
            size_of::<HPCON>(),
        )?;
    }
    if copies.is_some() {
        attributes.set(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            inherited.as_mut_ptr().cast_const().cast(),
            inherited.len() * size_of::<HANDLE>(),
        )?;
    }

    let mut flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
    if launch.stdio.is_some() {
        flags |= CREATE_NO_WINDOW;
    }
    // SAFETY: PROCESS_INFORMATION is plain data that the call fills.
    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer is to a live, NUL-terminated or sized buffer owned by this
    // function; the command line is writable as the call requires; the attribute list names
    // the job, console and handles that all outlive the call.
    check(unsafe {
        CreateProcessW(
            ptr::null(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            i32::from(launch.stdio.is_some()),
            flags,
            environment.as_mut_ptr().cast(),
            directory.as_ref().map_or(ptr::null(), |path| path.as_ptr()),
            &startup.StartupInfo,
            &mut information,
        )
    })?;
    let _thread = owned(information.hThread)?;
    Ok((owned(information.hProcess)?, information.dwProcessId))
}

fn unsafe_zeroed<T>() -> T {
    // SAFETY: only used for plain-data Windows structures, for which all-zero is a valid value.
    unsafe { std::mem::zeroed() }
}

/// A process/thread attribute list, deleted when dropped.
struct AttributeList {
    storage: Vec<u64>,
}

impl AttributeList {
    fn new(count: usize) -> io::Result<Self> {
        let mut bytes = 0;
        // SAFETY: a null list only asks for the size; the call "fails" with the size set.
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), count as u32, 0, &mut bytes);
        }
        let mut storage = vec![0_u64; bytes.div_ceil(8).max(1)];
        // SAFETY: `storage` has the `bytes` the first call asked for, 8-byte aligned.
        check(unsafe {
            InitializeProcThreadAttributeList(
                storage.as_mut_ptr().cast(),
                count as u32,
                0,
                &mut bytes,
            )
        })?;
        Ok(Self { storage })
    }

    fn pointer(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_ptr().cast_mut().cast()
    }

    fn set(&self, attribute: u32, value: *const std::ffi::c_void, size: usize) -> io::Result<()> {
        // SAFETY: the list is initialised for at least this many attributes; the caller keeps
        // `value` alive until the process is created.
        check(unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                attribute as usize,
                value,
                size,
                ptr::null_mut(),
                ptr::null(),
            )
        })
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: the list was initialised in `new` and is deleted once.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(program: &str, arguments: &[&str]) -> Result<String, UnsafeBatchArgument> {
        let line = command_line(Path::new(program), arguments.iter().map(OsStr::new))?;
        Ok(String::from_utf16_lossy(&line[..line.len() - 1]))
    }

    #[test]
    fn batch_arguments_with_command_characters_are_quoted_whole() {
        assert_eq!(
            line(r"C:\t\a.cmd", &["a&b", "x|y", "c d", "<>^()", ""]).unwrap(),
            r#""C:\t\a.cmd" "a&b" "x|y" "c d" "<>^()" """#
        );
        assert!(line(r"C:\t\A.BAT", &["a&b"]).unwrap().ends_with(r#""a&b""#));
    }

    #[test]
    fn a_batch_argument_that_could_end_the_quoting_or_expand_is_refused() {
        for unsafe_argument in ["a\"b", "50%", "%PATH%", "a\nb", "a\rb", "a\0b"] {
            assert_eq!(
                line(r"C:\t\a.cmd", &[unsafe_argument]),
                Err(UnsafeBatchArgument),
                "{unsafe_argument:?}"
            );
        }
        // The same text is a normal argument for a program that reads its own command line.
        assert_eq!(
            line(r"C:\t\a.exe", &["50%", "a b", "q\"t"]).unwrap(),
            r#""C:\t\a.exe" 50% "a b" "q\"t""#
        );
    }
}
