use std::io::Write;

use chacha20poly1305::{ChaCha20Poly1305, aead::{generic_array::GenericArray, AeadMutInPlace}, KeyInit};
use rand::{SeedableRng, RngCore};

use crate::{BLOCK_SIZE, Header, MAGIC, HEADER_SIZE, PAYLOAD_SIZE, VERSION_0, VARIANT_ARGON_CHACHA20_POLY};

pub struct EncryptedWriter<W : Write> {
    inner : W,
    
    pub(crate) key : [u8;32],
    pub(crate) current_header : Header,

    current_chunk_position : usize,
    current_chunk: Box<[u8; BLOCK_SIZE]>,
}

impl<W : Write> EncryptedWriter<W> {
    pub fn new(inner : W, passphrase : &[u8]) -> Self {
        let mut salt = [0;8];
        let mut rng = rand::rngs::StdRng::from_entropy();
        rng.fill_bytes(&mut salt);

        let header = Header {
            magic : *MAGIC,
            version: 0,
            variant: 1,
            blockcounter: 0,
            salt,
        };

        let key = super::generate_key(passphrase, &header);

        Self {
            inner,
            
            key,
            current_header : header,
            current_chunk_position: 0,
            current_chunk: Box::new([0; BLOCK_SIZE]),
        }
    }

    #[allow(dead_code)] // used in tests
    pub(crate) fn new_from_salt_and_key(inner : W, salt : [u8; 8], key : [u8; 32], blockcounter: u32) -> Self {
        let header = Header {
            magic : *MAGIC,
            version: VERSION_0,
            variant: VARIANT_ARGON_CHACHA20_POLY,
            blockcounter,
            salt,
        };

        Self {
            inner,
            
            key,
            current_header : header,
            current_chunk_position: 0,
            current_chunk: Box::new([0; BLOCK_SIZE]),
        }
    } 

    fn write_chunk(&mut self) -> std::io::Result<()> {
        self.current_chunk[0..HEADER_SIZE].copy_from_slice(&self.current_header.to_bytes());
        for i in self.current_chunk_position..PAYLOAD_SIZE {
            self.current_chunk[i] = 0;
        }
        let nonce = super::payload_nonce(&self.current_header);
        let mut cipher = ChaCha20Poly1305::new(GenericArray::from_slice((&self.key[..]).into()));
        let poly_tag = cipher.encrypt_in_place_detached(
            GenericArray::from_slice(&nonce[..]), 
            b"", 
            &mut self.current_chunk[HEADER_SIZE..][..PAYLOAD_SIZE]
        ).unwrap();
        self.current_chunk[HEADER_SIZE + PAYLOAD_SIZE..].copy_from_slice(&poly_tag[..]);

        self.inner.write_all(&self.current_chunk[..])?;

        self.current_header.blockcounter += 1;
        Ok(())
    }
}

impl< W : Write > Write for EncryptedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let left = PAYLOAD_SIZE - self.current_chunk_position;

        let to_write = std::cmp::min(left, buf.len());
        self.current_chunk[HEADER_SIZE + self.current_chunk_position..][..to_write].copy_from_slice(&buf[..to_write]);
        self.current_chunk_position += to_write;

        if self.current_chunk_position == PAYLOAD_SIZE {
            self.write_chunk()?;
            self.current_chunk_position = 0;
        }

        Ok(to_write)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<W : Write> Drop for EncryptedWriter<W> {
    fn drop(&mut self) {
        if self.current_chunk_position > 0 {
            self.write_chunk().unwrap();
        }
    }
}