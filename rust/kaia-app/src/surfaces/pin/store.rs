//! The screen lock's files in the data folder. `pin` holds the salt and the hash
//! of the PIN. `pin-attempts` holds the wrong tries and the end of the wait.
//!
//! The digits are never written. The file keeps a random 16-byte salt and the
//! PBKDF2-HMAC-SHA256 output of the digits with that salt. The file is plain text,
//! so a person who opens it over USB sees what it holds. Deleting `pin` removes
//! the lock.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::Sha256;

use crate::durable;

/// The PIN file. Its path on the device is `.kaia/pin`.
pub const FILE: &str = "pin";
/// The wrong-try count and the end of the wait. Deleting it resets the count.
pub const ATTEMPTS_FILE: &str = "pin-attempts";
/// PBKDF2 rounds for a new PIN. The file records the rounds it was made with.
pub const ROUNDS: u32 = 100_000;
/// The shortest PIN.
pub const MIN_DIGITS: usize = 4;
/// The longest PIN.
pub const MAX_DIGITS: usize = 8;

const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const KDF: &str = "pbkdf2-sha256";
/// The most rounds a file may ask for. A damaged file cannot make one check run for minutes.
const MAX_ROUNDS: u32 = 10_000_000;
const HEADER: &str = "# Kaia screen lock. Delete this file to remove the lock.";

/// A stored PIN: the salt, the rounds and the hash. It never holds the digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    rounds: u32,
    salt: [u8; SALT_LEN],
    hash: [u8; HASH_LEN],
}

impl Record {
    /// The record for `digits` with `salt`.
    pub fn new(digits: &str, salt: [u8; SALT_LEN], rounds: u32) -> Self {
        Self {
            rounds,
            salt,
            hash: derive(digits, &salt, rounds),
        }
    }

    /// True when `digits` give the stored hash. The comparison looks at every byte.
    pub fn matches(&self, digits: &str) -> bool {
        let candidate = derive(digits, &self.salt, self.rounds);
        candidate
            .iter()
            .zip(self.hash.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }

    fn to_text(&self) -> String {
        format!(
            "{HEADER}\ndevice.pin_kdf={KDF}\ndevice.pin_rounds={}\ndevice.pin_salt={}\ndevice.pin_hash={}\n",
            self.rounds,
            hex(&self.salt),
            hex(&self.hash),
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let (mut kdf, mut rounds, mut salt, mut hash) = (None, None, None, None);
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "device.pin_kdf" => kdf = Some(value),
                "device.pin_rounds" => rounds = Some(value),
                "device.pin_salt" => salt = Some(value),
                "device.pin_hash" => hash = Some(value),
                _ => {}
            }
        }
        if kdf != Some(KDF) {
            return None;
        }
        let rounds = rounds?
            .parse::<u32>()
            .ok()
            .filter(|r| (1..=MAX_ROUNDS).contains(r))?;
        Some(Self {
            rounds,
            salt: unhex(salt?)?,
            hash: unhex(hash?)?,
        })
    }
}

fn derive(digits: &str, salt: &[u8], rounds: u32) -> [u8; HASH_LEN] {
    pbkdf2::pbkdf2_hmac_array::<Sha256, HASH_LEN>(digits.as_bytes(), salt, rounds)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != 2 * N || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, pair) in text.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// True when `text` is a PIN of 4 to 8 ASCII digits.
pub fn valid_digits(text: &str) -> bool {
    (MIN_DIGITS..=MAX_DIGITS).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

/// True when a PIN is set. The file counts even when it cannot be read: the lock
/// stays until someone deletes the file.
pub fn is_on(dir: &Path) -> bool {
    dir.join(FILE).exists()
}

/// The stored PIN. None when there is no file or the file does not parse.
pub fn load(dir: &Path) -> Option<Record> {
    fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| Record::parse(&text))
}

/// Stores `digits` as the PIN with a new random salt, and clears the wrong tries.
/// The digits are not written.
pub fn save(dir: &Path, digits: &str) -> io::Result<()> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(io::Error::other)?;
    let record = Record::new(digits, salt, ROUNDS);
    durable::atomic_write(&dir.join(FILE), record.to_text().as_bytes())?;
    clear_attempts(dir)
}

/// Removes the lock: the PIN file and the wrong tries. A missing file is not an error.
pub fn clear(dir: &Path) -> io::Result<()> {
    remove(dir.join(FILE))?;
    clear_attempts(dir)
}

/// The wrong tries counted since the last right PIN, and the Unix time (seconds) when
/// the current wait ends. Zero when no wait was started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Attempts {
    pub wrong: u32,
    pub until: u64,
}

/// The stored wrong tries. A file that does not parse counts as none.
pub fn load_attempts(dir: &Path) -> Attempts {
    let mut out = Attempts::default();
    let Ok(text) = fs::read_to_string(dir.join(ATTEMPTS_FILE)) else {
        return out;
    };
    for line in text.lines() {
        match line.split_once('=') {
            Some(("wrong", value)) => out.wrong = value.trim().parse().unwrap_or(0),
            Some(("until", value)) => out.until = value.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    out
}

/// Stores the wrong tries. The default value removes the file.
pub fn save_attempts(dir: &Path, attempts: Attempts) -> io::Result<()> {
    if attempts == Attempts::default() {
        return clear_attempts(dir);
    }
    let text = format!("wrong={}\nuntil={}\n", attempts.wrong, attempts.until);
    durable::atomic_write(&dir.join(ATTEMPTS_FILE), text.as_bytes())
}

/// Removes the wrong tries.
pub fn clear_attempts(dir: &Path) -> io::Result<()> {
    remove(dir.join(ATTEMPTS_FILE))
}

fn remove(path: PathBuf) -> io::Result<()> {
    match fs::remove_file(&path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kaia-pin-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_hash_is_not_the_digits_and_changes_with_the_salt() {
        let a = Record::new("4826", [1; SALT_LEN], 1000);
        let b = Record::new("4826", [2; SALT_LEN], 1000);
        assert_ne!(a.hash.as_slice(), b"4826".as_slice());
        assert_ne!(a.hash, b.hash, "the same digits with two salts must differ");
        assert!(a.matches("4826"));
        assert!(!a.matches("4827"));
        assert!(!a.matches("482"));
    }

    #[test]
    fn a_record_round_trips_through_its_text() {
        let record = Record::new("123456", [7; SALT_LEN], 2000);
        let text = record.to_text();
        assert!(text.starts_with(HEADER));
        assert!(!text.contains("123456"));
        let back = Record::parse(&text).expect("the text parses");
        assert!(back.matches("123456"));
        assert_eq!(back, record);
    }

    #[test]
    fn a_damaged_file_has_no_record() {
        let good = Record::new("1234", [3; SALT_LEN], 10).to_text();
        assert!(Record::parse(&good.replace("pbkdf2-sha256", "md5")).is_none());
        assert!(Record::parse(&good.replace("device.pin_salt=", "device.pin_salt=zz")).is_none());
        assert!(Record::parse(&good.replace("device.pin_rounds=10", "device.pin_rounds=0")).is_none());
        assert!(Record::parse("").is_none());
    }

    #[test]
    fn digits_must_be_four_to_eight_ascii_digits() {
        assert!(valid_digits("1234"));
        assert!(valid_digits("12345678"));
        assert!(!valid_digits("123"));
        assert!(!valid_digits("123456789"));
        assert!(!valid_digits("12a4"));
        assert!(!valid_digits("1234\u{0661}"));
    }

    #[test]
    fn save_load_and_clear_the_lock() {
        let dir = dir("save");
        assert!(!is_on(&dir));
        save(&dir, "2468").unwrap();
        assert!(is_on(&dir));
        let stored = load(&dir).expect("the record loads");
        assert!(stored.matches("2468"));
        let text = fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!text.contains("2468"), "the digits must not be written");
        clear(&dir).unwrap();
        assert!(!is_on(&dir));
        assert!(load(&dir).is_none());
        clear(&dir).unwrap();
    }

    #[test]
    fn two_saves_of_one_pin_use_different_salts() {
        let dir = dir("salts");
        save(&dir, "1111").unwrap();
        let first = fs::read_to_string(dir.join(FILE)).unwrap();
        save(&dir, "1111").unwrap();
        let second = fs::read_to_string(dir.join(FILE)).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn attempts_round_trip_and_the_default_removes_the_file() {
        let dir = dir("attempts");
        assert_eq!(load_attempts(&dir), Attempts::default());
        save_attempts(
            &dir,
            Attempts {
                wrong: 5,
                until: 1_700_000_030,
            },
        )
        .unwrap();
        assert_eq!(
            load_attempts(&dir),
            Attempts {
                wrong: 5,
                until: 1_700_000_030
            }
        );
        save_attempts(&dir, Attempts::default()).unwrap();
        assert!(!dir.join(ATTEMPTS_FILE).exists());
    }
}
