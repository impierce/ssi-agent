use oid4vci::credential_offer::{InputMode, TxCodeConstraints};
use rand::Rng;

/// This function generates a Transaction Code based on parameters defined in the TxCodeConstraints struct within a Credential Offer.
/// These parameters include input mode, length, and description. The generated Transaction Code (TxCode) will then be sent to the end user
/// out-of-band, e.g. via e-mail.
pub fn generate_tx_code(tx_code: &TxCodeConstraints) -> String {
    let length = tx_code.length.unwrap_or(6) as usize; // Default to 6 digits if not otherwise specified
    let input_mode = tx_code.input_mode.as_ref().unwrap_or(&InputMode::Numeric);
    let mut rng = rand::rng();

    match input_mode {
        InputMode::Numeric => {
            // Generate random digits.
            (0..length).map(|_| rng.random_range(0..=9).to_string()).collect()
        }
        InputMode::Text => {
            // Generate random letters.
            const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
            (0..length)
                .map(|_| {
                    let idx = rng.random_range(0..ALPHABET.len());
                    ALPHABET[idx] as char
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SAMPLE_TX_NUMERIC: TxCodeConstraints = TxCodeConstraints {
        input_mode: Some(InputMode::Numeric),
        length: Some(6),
        description: None,
    };

    static SAMPLE_TX_ALPHABET: TxCodeConstraints = TxCodeConstraints {
        input_mode: Some(InputMode::Text),
        length: Some(8),
        description: None,
    };

    #[test]
    pub fn generate_numeric_transaction_code() {
        let code_sample = generate_tx_code(&SAMPLE_TX_NUMERIC);
        println!("This is a sample code: {code_sample}");
        assert_eq!(code_sample.len(), 6);
    }

    #[test]
    pub fn generate_alphabetic_transaction_code() {
        let code_sample_abc = generate_tx_code(&SAMPLE_TX_ALPHABET);
        println!("This is a sample code: {code_sample_abc}");
        assert_eq!(code_sample_abc.len(), 8);
    }
}
