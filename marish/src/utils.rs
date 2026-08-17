use std::sync::Arc;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;

use rustix::io::Errno;

// Wrapper for BorrowedFd<'_> that implements Read + Write with rustix
pub struct FdRw(pub Arc<OwnedFd>);

impl Read for FdRw {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match rustix::io::read(&self.0, &mut *buf) {
                Ok(n) => return Ok(n),
                Err(Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Write for FdRw {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match rustix::io::write(&self.0, buf) {
                Ok(n) => return Ok(n),
                Err(Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }

    // Raw fd writes are unbuffered -- no flushing
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
