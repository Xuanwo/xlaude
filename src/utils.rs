use anyhow::{Context, Result};
use rand::{RngCore, SeedableRng};
use rand::seq::SliceRandom;

pub fn generate_random_name() -> Result<String> {
    // Allow setting seed for testing
    let mut rng = if let Ok(seed_str) = std::env::var("XLAUDE_TEST_SEED") {
        let seed: u64 = seed_str.parse().unwrap_or(42);
        Box::new(rand::rngs::StdRng::seed_from_u64(seed)) as Box<dyn RngCore>
    } else {
        Box::new(rand::thread_rng()) as Box<dyn RngCore>
    };
    
    // Generate 128 bits of entropy for a 12-word mnemonic
    let mut entropy = [0u8; 16];
    rng.fill_bytes(&mut entropy);

    let mnemonic = bip39::Mnemonic::from_entropy(&entropy)?;
    let words: Vec<&str> = mnemonic.words().collect();
    
    // Use the same RNG for choosing the word
    let mut chooser_rng = if let Ok(seed_str) = std::env::var("XLAUDE_TEST_SEED") {
        let seed: u64 = seed_str.parse().unwrap_or(42);
        rand::rngs::StdRng::seed_from_u64(seed)
    } else {
        rand::rngs::StdRng::from_entropy()
    };
    
    words
        .choose(&mut chooser_rng)
        .map(|&word| word.to_string())
        .context("Failed to generate random name")
}
