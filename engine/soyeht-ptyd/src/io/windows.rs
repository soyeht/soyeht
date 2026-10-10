use std::{
    fs::File,
    io::{self, Read},
    os::windows::io::AsRawHandle,
    ptr,
    sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, sync_channel},
    thread::{self, JoinHandle},
    time::Duration,
};
use windows_sys::Win32::System::{
    IO::CancelSynchronousIo,
    Pipes::{PIPE_NOWAIT, SetNamedPipeHandleState},
};

/// How many chunks the reader may have ready before it waits for the supervisor.
const READY_CHUNKS: usize = 16;
const CHUNK_BYTES: usize = 16384;

pub(super) fn input_without_waiting(file: &File) -> io::Result<()> {
    let mode = PIPE_NOWAIT;
    // SAFETY: the handle is an open pipe end and `mode` lives for the call.
    let changed =
        unsafe { SetNamedPipeHandleState(file.as_raw_handle(), &mode, ptr::null(), ptr::null()) };
    if changed == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The read end of a terminal or pipe. A pipe has no readiness to wait on, so a thread of its
/// own blocks in the read and passes what it gets on; the supervisor then waits on a channel,
/// which costs nothing while the child is quiet.
pub(super) struct Source {
    chunks: Receiver<io::Result<Vec<u8>>>,
    held: Vec<u8>,
    taken: usize,
    failure: Option<io::Error>,
    reader: JoinHandle<()>,
}

impl Source {
    pub(super) fn new(mut file: File) -> io::Result<Self> {
        let (sender, chunks) = sync_channel(READY_CHUNKS);
        let reader = thread::Builder::new()
            .name("pipe-reader".into())
            .spawn(move || {
                let mut bytes = [0; CHUNK_BYTES];
                loop {
                    let chunk = match file.read(&mut bytes) {
                        // The writer closed its end.
                        Ok(0) => return,
                        Ok(count) => Ok(bytes[..count].to_vec()),
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => Err(error),
                    };
                    let failed = chunk.is_err();
                    // Nobody is reading any more when this fails.
                    if sender.send(chunk).is_err() || failed {
                        return;
                    }
                }
            })?;
        Ok(Self {
            chunks,
            held: Vec::new(),
            taken: 0,
            failure: None,
            reader,
        })
    }

    fn has_bytes(&self) -> bool {
        self.taken < self.held.len()
    }

    fn receive(&mut self, chunk: io::Result<Vec<u8>>) {
        match chunk {
            Ok(bytes) => (self.held, self.taken) = (bytes, 0),
            Err(error) => self.failure = Some(error),
        }
    }

    pub(super) fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.has_bytes() {
            if let Some(error) = self.failure.take() {
                return Err(error);
            }
            match self.chunks.try_recv() {
                Ok(chunk) => self.receive(chunk),
                Err(TryRecvError::Empty) => return Err(io::ErrorKind::WouldBlock.into()),
                Err(TryRecvError::Disconnected) => return Ok(0),
            }
            if let Some(error) = self.failure.take() {
                return Err(error);
            }
        }
        let count = buffer.len().min(self.held.len() - self.taken);
        buffer[..count].copy_from_slice(&self.held[self.taken..self.taken + count]);
        self.taken += count;
        Ok(count)
    }

    pub(super) fn wait_readable(&mut self, timeout: Duration) -> io::Result<()> {
        if self.has_bytes() || self.failure.is_some() {
            return Ok(());
        }
        match self.chunks.recv_timeout(timeout) {
            Ok(chunk) => self.receive(chunk),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
        Ok(())
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if !self.reader.is_finished() {
            // SAFETY: the thread handle stays open while the join handle lives. The call only
            // interrupts the read the thread is blocked in, so it ends instead of lingering
            // until the pipe closes.
            unsafe { CancelSynchronousIo(self.reader.as_raw_handle()) };
        }
    }
}
