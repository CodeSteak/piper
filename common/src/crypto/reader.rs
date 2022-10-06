use std::{collections::{BTreeMap}, io::{Read, SeekFrom, Seek}};
use chacha20poly1305::{aead::generic_array::GenericArray, ChaCha20Poly1305, KeyInit, AeadInPlace};

use super::{HEADER_SIZE, BLOCK_SIZE, PAYLOAD_SIZE, POLY_TAG_SIZE, MAGIC, EncryptedFileError, Header};
pub struct EncryptedReader<R> {
    inner : R,
    passphrase: Vec<u8>,
    stream_state : BTreeMap<[u8;8], StreamState>,
    last_stream : Option<[u8;8]>,
    
    current_chunk_position : usize,
    current_chunk: Box<[u8; BLOCK_SIZE]>,

    global_position : u64,
}

#[derive(Clone, Copy)]
struct StreamState {
    key : [u8;32],
    first_stream_chunk : i64,
    next_stream_block : Option<i64>,
}


impl<R> EncryptedReader<R> {
    pub fn new(inner : R, passphrase : &[u8]) -> Self {
        Self {
            inner,
            passphrase: passphrase.to_vec(),
            stream_state : BTreeMap::new(),
            last_stream: None,
            current_chunk_position : PAYLOAD_SIZE,
            current_chunk: Box::new([0; BLOCK_SIZE]),
            global_position : 0,
        }
    }

    fn payload_bytes(&self) -> &[u8; PAYLOAD_SIZE] {
        self.current_chunk[HEADER_SIZE..][..PAYLOAD_SIZE].try_into().unwrap()
    }

    fn header_bytes(&self) -> &[u8; HEADER_SIZE] {
        self.current_chunk[..HEADER_SIZE].try_into().unwrap()
    }

    fn poly_tag_bytes(&self) -> &[u8; POLY_TAG_SIZE] {
        self.current_chunk[HEADER_SIZE + PAYLOAD_SIZE..].try_into().unwrap()
    }

    fn payload_bytes_mut(&mut self) -> &mut [u8; PAYLOAD_SIZE] {
        (&mut self.current_chunk[HEADER_SIZE..][..PAYLOAD_SIZE]).try_into().unwrap()
    }

    fn get_state(&mut self, header : &Header) -> Result<StreamState, EncryptedFileError> {
        let current_block = self.global_position as i64 / PAYLOAD_SIZE as i64;

        // Update last block
        if self.last_stream.is_some() && self.last_stream != Some(header.salt) {
            dbg!("Updating last block");
            let mut last_state = self.stream_state.get_mut(&self.last_stream.unwrap()).unwrap();
            last_state.next_stream_block = Some(current_block);
        }
        // Remember last stream
        self.last_stream = Some(header.salt);

        if let Some(state) = self.stream_state.get(&header.salt) {
            // Check last block validity
            if let Some(next_stream_chunk) = state.next_stream_block {
                if next_stream_chunk <= current_block {
                    return Err(EncryptedFileError::InvalidBlockCounter);
                }
            }

            return if current_block == state.first_stream_chunk + header.blockcounter as i64 {
                 Ok(state.clone())
            } else {
                Err(EncryptedFileError::InvalidBlockCounter)
            }
        }

        let key = super::generate_key(&self.passphrase, header);
        let start_chunk_position = current_block - header.blockcounter as i64;
        if start_chunk_position < 0 {
            return Err(EncryptedFileError::InvalidBlockCounter);
        }

        let state = StreamState { 
            key:  key, 
            first_stream_chunk: start_chunk_position,
            next_stream_block: None,
        };
        self.stream_state.insert(header.salt, state);
        Ok(state)
    }
}

impl < R: Read > EncryptedReader<R> {

    fn read_chunk(&mut self) -> Result<bool, EncryptedFileError> {
        self.current_chunk_position = PAYLOAD_SIZE;
        match self.inner.read(&mut self.current_chunk[..])? {
            0 => {return Ok(false);},
            BLOCK_SIZE => (),
            n => {
                self.inner.read_exact(&mut self.current_chunk[n..])?;
            },
        }

        let header = Header::from(*self.header_bytes());
        if header.magic != *MAGIC {
            return Err(EncryptedFileError::InvalidHeader);
        }
        if header.version != 0 || header.variant != 1 {
            return Err(EncryptedFileError::UnsupportedVariant);
        }

        let key = self.get_state(&header)?;
        let nonce = super::payload_nonce(&header);

        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice((&key.key[..]).into()));
        let tag = 
            GenericArray::from_slice(self.poly_tag_bytes())
            .to_owned();
       
        cipher.decrypt_in_place_detached(
            &GenericArray::from_slice(&nonce),
            &[], // no additional data
            &mut self.payload_bytes_mut()[..],
            &tag
        )?;
        self.current_chunk_position = 0;
        Ok(true)
    }
}

impl<R : Read> Read for EncryptedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        if self.current_chunk_position == PAYLOAD_SIZE {
            if !self.read_chunk()? {
                return Ok(0);
            }
        }

        let to_read = std::cmp::min(buf.len(), PAYLOAD_SIZE-self.current_chunk_position);
        buf[..to_read].copy_from_slice(&self.payload_bytes()[self.current_chunk_position..][..to_read]);
        self.current_chunk_position += to_read;
        self.global_position += to_read as u64;
        Ok(to_read)
    }
}

impl<R : Read+Seek> Seek for EncryptedReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match pos {
            SeekFrom::Start(n) => {
                let block = n / PAYLOAD_SIZE as u64;
                let offset = n % PAYLOAD_SIZE as u64;
                self.inner.seek(SeekFrom::Start(block * BLOCK_SIZE as u64))?;

                self.last_stream = None;
                self.global_position = block * PAYLOAD_SIZE as u64;

                if self.read_chunk()? { // if not at EOF
                    self.current_chunk_position = offset as usize;
                    self.global_position += offset;
                }
                Ok(n)
            },
            SeekFrom::Current(n) => {
                let new_pos = self.global_position as i64 + n;
                if new_pos < 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Seek before start of file"));
                }
                self.seek(SeekFrom::Start(new_pos as u64))
            },
            SeekFrom::End(n) => {
                let end = self.inner.seek(SeekFrom::End(0))?;
                let blocks = end / BLOCK_SIZE as u64;

                let new_pos = blocks as i64 * PAYLOAD_SIZE as i64 + n;
                if new_pos < 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Seek before start of file"));
                }
                self.seek(SeekFrom::Start(new_pos as u64))
            },
        }
    }
}