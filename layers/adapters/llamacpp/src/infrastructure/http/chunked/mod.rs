//! HTTP chunked response decoding.

use std::io::{BufRead, Read};

pub(crate) struct ChunkedReader<R: BufRead> {
    inner: R,
    remaining: usize,
    done: bool,
}
impl<R: BufRead> ChunkedReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
            done: false,
        }
    }
}
impl<R: BufRead> Read for ChunkedReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.done || out.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut size = String::new();
            self.inner.read_line(&mut size)?;
            let hex = size.trim().split(';').next().unwrap_or("");
            self.remaining = usize::from_str_radix(hex, 16).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
            })?;
            if self.remaining == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let count = self.remaining.min(out.len());
        self.inner.read_exact(&mut out[..count])?;
        self.remaining -= count;
        if self.remaining == 0 {
            let mut ending = [0; 2];
            self.inner.read_exact(&mut ending)?;
        }
        Ok(count)
    }
}
