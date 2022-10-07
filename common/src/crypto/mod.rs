use std::fmt::{Formatter, Display};

mod reader;
pub use reader::EncryptedReader;

mod writer;
pub use writer::EncryptedWriter;

pub(crate) const HEADER_SIZE: usize = 1 /*magic*/ + 1 /*version */ + 4 /*blockcounter*/ + 10 /*salt*/; 
pub(crate) const POLY_TAG_SIZE: usize = 16;

pub(crate) const PAYLOAD_SIZE : usize = 512;
pub(crate) const BLOCK_SIZE : usize = HEADER_SIZE + PAYLOAD_SIZE + POLY_TAG_SIZE;

pub(crate) const ARGON2_PARAMS : argon2::Config = argon2::Config {
    variant: argon2::Variant::Argon2i,
    version: argon2::Version::Version13,
    mem_cost: 65536,
    time_cost: 3,
    lanes: 1,
    thread_mode: argon2::ThreadMode::Sequential,
    secret: &[],
    ad: &[],
    hash_length: 32,
};

const VERSION_0 : u8 = 0;
const VARIANT_ARGON_CHACHA20_POLY : u8 = 1;

const COUNTER_HINT : u32 = u32::from_be_bytes([b'5',b'4',b'4',b'b']);

pub(crate) const MAGIC  : &[u8;256] = br#"#toc#stream_____
key=argon2iv13(t=3,m=65536,p=1,salt=SALT:10|'#toc',PLAIN)
nonce=SALT[0:8]|COUNTER
magic=if COUNTER<16 '#toc#stream_____'[COUNTER] else ?
v=1
enc,tag=chacha20-poly1305(nonce,key)
c=COUNTER:4be^'544b'
magic:1|v:1|c:4|SALT:10|enc:512|tag:16

"#;


#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    pub(crate) magic : u8,
    pub(crate) version : u8,
    pub(crate) variant : u8,
    pub(crate) blockcounter : u32,
    pub(crate) salt : [u8;10],
}

impl From<[u8; HEADER_SIZE]> for Header {
    fn from(data: [u8; HEADER_SIZE]) -> Self {
        Header { 
            magic:   data[0],
            version: (data[1] >> 4) & 0x0F,
            variant: (data[1] >> 0) & 0x0F,
            blockcounter: u32::from_be_bytes(data[2..6].try_into().unwrap()) ^ COUNTER_HINT, 
            salt: data[6..].try_into().unwrap(),
        }
    }
}

impl From<Header> for [u8; HEADER_SIZE] {
    fn from(header: Header) -> Self {
        let mut data = [0u8; HEADER_SIZE];
        data[0] = MAGIC[header.blockcounter as usize % MAGIC.len()];
        data[1] = (header.version << 4) | (header.variant << 0);
        data[2..6].copy_from_slice(&(header.blockcounter^COUNTER_HINT).to_be_bytes());
        data[6..].copy_from_slice(&header.salt);
        data
    }
}

impl Header {
    pub(crate) fn to_bytes(self) -> [u8; HEADER_SIZE] {
        self.into()
    }

    fn magic_ok(&self) -> bool {
        self.blockcounter >= 16 || self.magic == MAGIC[self.blockcounter as usize % MAGIC.len()]
    }
}

enum EncryptedFileError {
    Io(std::io::Error),
    InvalidHeader,
    InvalidChunk,
    UnsupportedVariant,
    InvalidBlockCounter,
    KeyError,
}

impl Display for EncryptedFileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EncryptedFileError::Io(e) => write!(f, "IO Error: {}", e),
            EncryptedFileError::InvalidHeader => write!(f, "Invalid Header"),
            EncryptedFileError::UnsupportedVariant => write!(f, "Unsupported Variant"),
            EncryptedFileError::KeyError => write!(f, "Key Error"),
            EncryptedFileError::InvalidChunk => write!(f, "Invalid Chunk"),
            EncryptedFileError::InvalidBlockCounter => write!(f, "Invalid Block Counter"),
        }
    }
}

impl From<EncryptedFileError> for std::io::Error {
    fn from(e : EncryptedFileError) -> std::io::Error {
        match e {
            EncryptedFileError::Io(e) => e,
            EncryptedFileError::InvalidHeader => std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid Header"),
            EncryptedFileError::UnsupportedVariant => std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported Variant"),
            EncryptedFileError::KeyError => std::io::Error::new(std::io::ErrorKind::InvalidData, "Key Error"),
            EncryptedFileError::InvalidChunk => std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid Chunk"),
            EncryptedFileError::InvalidBlockCounter => std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid Block Counter"),
        }
    }
}

impl From<std::io::Error> for EncryptedFileError {
    fn from(e : std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            EncryptedFileError::InvalidChunk
        } else {
            EncryptedFileError::Io(e)
        }
    }
}

impl From<chacha20poly1305::aead::Error> for EncryptedFileError {
    fn from(_: chacha20poly1305::aead::Error) -> Self {
        Self::KeyError
    }
}

pub(crate) fn generate_key(passphrase : &[u8], header : &Header) -> [u8; 32] {
    let mut salt = [0u8; 14];
    salt[0..10].copy_from_slice(&header.salt);
    salt[10..].copy_from_slice(b"#toc");


    let key = argon2::hash_raw(&passphrase, &salt, &ARGON2_PARAMS).unwrap();
    let key : [u8;32] = key.try_into().unwrap();

    eprintln!("key={:?}", &key);
    eprintln!("salt={:?}\n", &salt);

    key
}

pub(crate) fn payload_nonce(h : &Header) -> [u8;12] {
    let mut nonce = [0;12];
    nonce[0..8].copy_from_slice(&h.salt[0..8]);
    nonce[8..12].copy_from_slice(&h.blockcounter.to_be_bytes());
    nonce
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use rand::{RngCore, Rng};
    use std::io::{Read, Write, Seek};

    const TWO_MB : usize = 2 * 1024 * 1024;

    use super::*;

    fn generate_data(len : usize) -> Vec<u8> {
        let mut data = vec![0u8; len];
        rand::thread_rng().fill_bytes(&mut data);
        data
    }

    fn encrypt_all(buffer : &[u8], passphrase : &str) -> Vec<u8> {
        let mut writer = Vec::new();
        let mut enc = EncryptedWriter::new(&mut writer, passphrase.as_bytes());
        enc.write_all(buffer).unwrap();
        drop(enc);
        writer
    }

    fn decrypt_all(buffer : &[u8], passphrase : &str) -> std::io::Result<Vec<u8>> {
        let mut reader = Cursor::new(buffer);
        let mut dec = EncryptedReader::new(&mut reader, passphrase.as_bytes());
        let mut out = Vec::new();
        dec.read_to_end(&mut out)?;
        Ok(out)
    }

    #[test]
    fn test_write_and_read() {
        let original = generate_data(TWO_MB);
  
        let decoded = decrypt_all(&encrypt_all(&original, "test"), "test").unwrap();

        assert_eq!(original.len(), decoded.len());
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_error_on_wrong_passphrase() {
        let original = generate_data(TWO_MB);

        let encoded = encrypt_all(&original, "test");
        let mut dec = EncryptedReader::new(&encoded[..], "wrong".as_bytes());
        let mut out = Vec::new();
        assert!(dec.read_to_end(&mut out).is_err());

        assert_eq!(original, decrypt_all(&encoded, "test").unwrap());
    }

    #[test]
    fn test_encryption_is_salted() {
        let original = generate_data(TWO_MB);

        let encoded1 = encrypt_all(&original, "test");
        let encoded2 = encrypt_all(&original, "test");

        assert_ne!(encoded1, encoded2);
    }

    #[test]
    fn test_concat() {
        let original = generate_data(TWO_MB);

        let chunk_a = encrypt_all(&original[0..1024], "test");
        let chunk_b = encrypt_all(&original[1024..1024*1024], "test");
        let chunk_c = encrypt_all(&original[1024*1024..], "test");

        let all_chunks = [&chunk_a[..], &chunk_b[..], &chunk_c[..]].concat();

        let decoded = decrypt_all(&all_chunks, "test").unwrap();

        assert_eq!(original.len(), decoded.len());
        assert_eq!(original, decoded);
    }

    #[test]
    fn fail_on_ordering_has_been_changed() {
        let original = generate_data(TWO_MB);
        let encryped = encrypt_all(&original, "test");
        let mut modified_enrypted = encryped.clone();

        modified_enrypted[1024..(1024+512)].copy_from_slice(&encryped[1024+512..(1024+1024)]);
        modified_enrypted[1024+512..(1024+1024)].copy_from_slice(&encryped[1024..(1024+512)]);

        assert!(decrypt_all(&modified_enrypted, "test").is_err());
    }

    #[test]
    fn fail_on_write_in_between() {
        let original = generate_data(TWO_MB);
        let mut encryped = encrypt_all(&original, "test");

        encryped[(544*3)..(544*4)].copy_from_slice(&encrypt_all(&original[0..512], "test")[..]);

        assert!(decrypt_all(&encryped, "test").is_err());
    }

    #[test]
    fn test_seek() {
        let mut data = vec![0u8; TWO_MB];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut data);

        let mut encrypted = Vec::new();
        let mut writer = EncryptedWriter::new(&mut encrypted, b"test");
        writer.write_all(&data).unwrap();
        drop(writer);

        let mut decrypted_reader = Cursor::new(data);

        let encrypted_reader = Cursor::new(encrypted);
        let mut reader = EncryptedReader::new(encrypted_reader, b"test");

        for _ in 0..1024 {
            let length = rng.gen_range(0..4000);
            let offset = rng.gen_range(0..TWO_MB - length);

            let mut decrypted = vec![0u8; length];
            let mut encrypted = vec![0u8; length];

            reader.seek(std::io::SeekFrom::Start(offset as u64)).unwrap();
            decrypted_reader.seek(std::io::SeekFrom::Start(offset as u64)).unwrap();

            reader.read_exact(&mut encrypted).unwrap();
            decrypted_reader.read_exact(&mut decrypted).unwrap();
            
            assert_eq!(decrypted, encrypted);
        }
    }

    #[bench]
    fn bench_encrypt(b : &mut test::Bencher) {
        let data = generate_data(10*1024*1024);

        let mut encrypted = Vec::new();
        let writer = EncryptedWriter::new(vec![], b"test");
        b.iter(|| {
            encrypted.clear();
            let mut writer = EncryptedWriter::new_from_salt_and_key(
                &mut encrypted, 
                writer.current_header.salt, 
                writer.key, 
                0
            );
            writer.write_all(&data).unwrap();
            drop(writer);
        });
    }

    #[bench]
    fn bench_decrypt(b : &mut test::Bencher) {
        let data = generate_data(10*1024*1024);
        let encypted = encrypt_all(&data, "test");

        let mut reader = EncryptedReader::new(&encypted[..], b"test");
        reader.read(&mut []).unwrap();

        let mut out = Vec::new();
        b.iter(|| {
            out.clear();
            let mut reader = reader.clone_with(&encypted[..]);
            reader.read_to_end(&mut out).unwrap();
        });
    }

}