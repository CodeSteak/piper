use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use argon2::{Config, ThreadMode, Variant, Version};

use crate::tar_password::TarPassword;

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct TarHash {
    hash: [u8; 32],
}

impl TarHash {
    pub fn from_tarid(id: &TarPassword, salt: &str) -> Self {
        let password = id.to_string();
        let config = Config {
            variant: Variant::Argon2i,
            version: Version::Version13,
            mem_cost: 65536,
            time_cost: 3,
            lanes: 1,
            thread_mode: ThreadMode::Sequential,
            secret: &[],
            ad: &[],
            hash_length: 32,
        };

        let hash = argon2::hash_raw(password.as_bytes(), salt.as_bytes(), &config).unwrap();
        assert!(hash.len() == 32);

        Self {
            hash: hash.try_into().unwrap(),
        }
    }
}

impl Display for TarHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for b in self.hash.iter() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl FromStr for TarHash {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(());
        }

        let mut hash = [0u8; 32];
        for (i, c) in s.chars().enumerate() {
            let val = match c {
                '0'..='9' => c as u8 - b'0',
                'a'..='f' => c as u8 - b'a' + 10,
                'A'..='F' => c as u8 - b'A' + 10,
                _ => return Err(()),
            };
            hash[i / 2] |= val << (4 * (1 - i % 2));
        }
        Ok(TarHash { hash })
    }
}
