use std::io::Read;

// TODO: Optimize this
pub fn create_pipe() -> (PipeWriter, PipeReader) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    (
        PipeWriter { written: 0, sender },
        PipeReader {
            buffer: vec![],
            receiver,
        },
    )
}

pub struct PipeReader {
    buffer: Vec<u8>,
    receiver: std::sync::mpsc::Receiver<Vec<u8>>,
}

pub struct PipeWriter {
    written: u64,
    sender: std::sync::mpsc::SyncSender<Vec<u8>>,
}

impl PipeWriter {
    pub fn written(&self) -> u64 {
        self.written
    }
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.buffer.is_empty() {
            match self.receiver.recv() {
                Ok(v) => {
                    self.buffer = v;
                }
                Err(_) => return Ok(0),
            };
        }
        let n = std::cmp::min(buf.len(), self.buffer.len());
        buf[..n].copy_from_slice(&self.buffer[..n]);
        self.buffer.drain(..n);
        Ok(n)
    }
}

impl std::io::Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sender
            .send(buf.to_vec())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "send error"))?;
        self.written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
