//! Synchronous Windows byte pipes with bounded, caller-owned write attempts.
//!
//! Rust's ChildStdin uses an overlapped handle. Instead, std::io::pipe creates
//! synchronous CreatePipe endpoints. Change only our writer to PIPE_NOWAIT and
//! pass the blocking reader directly to the child as an owned standard handle.
//! No overlapped buffer, APC callback, relay thread or pending I/O outlives write.

use std::io::{self, PipeReader, PipeWriter, Write};
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::System::Pipes::{PIPE_NOWAIT, PIPE_READMODE_BYTE, SetNamedPipeHandleState};

#[derive(Debug)]
pub(crate) struct NativeStdin(PipeWriter);

pub(crate) fn stdin_pair() -> io::Result<(PipeReader, NativeStdin)> {
    let (reader, writer) = io::pipe()?;
    let mode = PIPE_NOWAIT | PIPE_READMODE_BYTE;
    // SAFETY: writer owns a synchronous, write-capable byte-pipe handle. The
    // call copies the local mode value, retains no pointers and changes no
    // inheritance flags. Null collection arguments are required for local pipes.
    if unsafe {
        SetNamedPipeHandleState(
            writer.as_raw_handle(),
            &mode,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((reader, NativeStdin(writer)))
}

impl Write for NativeStdin {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        super::nonblocking_byte_pipe_result(self.0.write(bytes), bytes.is_empty())
    }

    fn flush(&mut self) -> io::Result<()> {
        // There is no user-space buffer. Do not call FlushFileBuffers: it can
        // wait for the child to consume the kernel pipe buffer.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child_pipe::{WriteBudget, write_child_frame};
    use std::io::Read;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::Pipes::GetNamedPipeHandleStateW;

    fn mode(pipe: &impl AsRawHandle) -> u32 {
        let mut mode = 0;
        // SAFETY: query the mode of a live endpoint into initialized local
        // storage. All unused output buffers are null.
        assert_ne!(
            unsafe {
                GetNamedPipeHandleStateW(
                    pipe.as_raw_handle(),
                    &mut mode,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                )
            },
            0
        );
        mode
    }

    #[test]
    fn only_host_writer_is_nonblocking_and_small_frames_round_trip() {
        let (mut reader, mut writer) = stdin_pair().unwrap();
        assert_ne!(mode(&writer.0) & PIPE_NOWAIT, 0);
        assert_eq!(mode(&reader) & PIPE_NOWAIT, 0);
        write_child_frame(&mut writer, b"frame\n", WriteBudget::Immediate).unwrap();
        let mut bytes = [0; 6];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"frame\n");
    }

    #[test]
    fn full_pipe_exposes_partial_progress_then_zero_progress_without_waiting() {
        let (mut reader, mut writer) = stdin_pair().unwrap();
        let bytes = vec![b'x'; 3 * 1024 * 1024];
        let started = Instant::now();
        let failure = write_child_frame(
            &mut writer,
            &bytes,
            WriteBudget::Until(started + Duration::from_millis(30), None),
        )
        .unwrap_err();
        assert_eq!(failure.kind(), io::ErrorKind::TimedOut);
        assert!(failure.written > 0 && failure.written < bytes.len());
        assert!(started.elapsed() < Duration::from_secs(1));
        let failure_without_space =
            write_child_frame(&mut writer, b"next", WriteBudget::Immediate).unwrap_err();
        assert_eq!(failure_without_space.kind(), io::ErrorKind::TimedOut);
        assert_eq!(failure_without_space.written, 0);
        let mut accepted = vec![0; failure.written];
        reader.read_exact(&mut accepted).unwrap();
        assert_eq!(accepted, bytes[..failure.written]);
        write_child_frame(&mut writer, b"next\n", WriteBudget::Immediate).unwrap();
        let mut next = [0; 5];
        reader.read_exact(&mut next).unwrap();
        assert_eq!(&next, b"next\n");
    }

    #[test]
    fn disconnected_reader_is_broken_pipe_not_retryable_backpressure() {
        let (reader, mut writer) = stdin_pair().unwrap();
        drop(reader);
        assert_eq!(
            write_child_frame(&mut writer, b"frame\n", WriteBudget::Immediate)
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
