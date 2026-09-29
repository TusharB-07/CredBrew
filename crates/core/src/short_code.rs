use rand::Rng;
use thiserror::Error;

const BASE62_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const SHORT_CODE_LEN: usize = 8;
const MAX_RETRIES: usize = 5;

#[derive(Error, Debug)]
pub enum ShortCodeError {
    #[error("collision after {0} retries")]
    Collision(usize),
    #[error("invalid length")]
    InvalidLength,
}

pub fn generate_short_code() -> String {
    let mut rng = rand::thread_rng();
    (0..SHORT_CODE_LEN)
        .map(|_| BASE62_ALPHABET[rng.gen_range(0..BASE62_ALPHABET.len())] as char)
        .collect()
}

pub async fn generate_unique_short_code<F, Fut>(mut exists: F) -> Result<String, ShortCodeError>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<bool, sqlx::Error>>,
{
    for attempt in 0..=MAX_RETRIES {
        let code = generate_short_code();
        if !exists(code.clone()).await.map_err(|e| {
            tracing::error!("short_code existence check failed: {}", e);
            ShortCodeError::Collision(attempt)
        })? {
            return Ok(code);
        }
    }
    Err(ShortCodeError::Collision(MAX_RETRIES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_code_length() {
        let code = generate_short_code();
        assert_eq!(code.len(), SHORT_CODE_LEN);
    }

    #[test]
    fn test_short_code_alphabet() {
        let code = generate_short_code();
        assert!(code.chars().all(|c| BASE62_ALPHABET.contains(&(c as u8))));
    }

    #[test]
    fn test_short_code_uniqueness() {
        let mut codes = std::collections::HashSet::new();
        for _ in 0..1000 {
            let code = generate_short_code();
            assert!(codes.insert(code), "collision in 1000 generations");
        }
    }
}