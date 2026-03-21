//! RC4 stream cipher implementation.
//!
//! RC4 is a simple and fast stream cipher. While it has known weaknesses
//! for general cryptographic use, MSE/PE uses it with the first 1024 bytes
//! of keystream discarded, which mitigates the main known attacks.

/// RC4 cipher state.
#[derive(Clone)]
pub struct Rc4 {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    /// Create a new RC4 cipher initialized with the given key.
    pub fn new(key: &[u8]) -> Self {
        assert!(!key.is_empty(), "RC4 key must not be empty");

        let mut state = [0u8; 256];
        for (i, byte) in state.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            {
                *byte = i as u8;
            }
        }

        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, j as usize);
        }

        Rc4 { state, i: 0, j: 0 }
    }

    /// Generate the next byte of the keystream.
    #[inline]
    fn next_byte(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.state[self.i as usize]);
        self.state.swap(self.i as usize, self.j as usize);
        let idx = self.state[self.i as usize].wrapping_add(self.state[self.j as usize]);
        self.state[idx as usize]
    }

    /// Discard the first `n` bytes of the keystream.
    /// MSE requires discarding the first 1024 bytes.
    pub fn discard(&mut self, n: usize) {
        for _ in 0..n {
            self.next_byte();
        }
    }

    /// Encrypt/decrypt data in place (RC4 is symmetric).
    pub fn process_in_place(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            *byte ^= self.next_byte();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rc4_known_vector() {
        // Test vector: key = "Key", plaintext = "Plaintext"
        // Expected ciphertext from RFC 6229 / known test vectors
        let mut cipher = Rc4::new(b"Key");
        let mut data = *b"Plaintext";
        cipher.process_in_place(&mut data);
        // RC4("Key", "Plaintext") = [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]
        assert_eq!(data, [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]);
    }

    #[test]
    fn test_rc4_symmetry() {
        let key = b"test_key_12345";
        let plaintext = b"Hello, World! This is a test of RC4 encryption symmetry.";

        let mut encrypt = Rc4::new(key);
        let mut decrypt = Rc4::new(key);

        let mut ciphertext = plaintext.to_vec();
        encrypt.process_in_place(&mut ciphertext);

        // Ciphertext should differ from plaintext
        assert_ne!(&ciphertext[..], &plaintext[..]);

        // Decrypting should restore plaintext
        decrypt.process_in_place(&mut ciphertext);
        assert_eq!(&ciphertext[..], &plaintext[..]);
    }

    #[test]
    fn test_rc4_discard() {
        let key = b"test_key";

        let mut c1 = Rc4::new(key);
        let mut c2 = Rc4::new(key);

        // Discard 1024 bytes from c2
        c2.discard(1024);

        // Advance c1 by 1024 bytes manually
        for _ in 0..1024 {
            c1.next_byte();
        }

        // Now both should produce the same keystream
        let mut data1 = [0u8; 32];
        let mut data2 = [0u8; 32];
        c1.process_in_place(&mut data1);
        c2.process_in_place(&mut data2);
        assert_eq!(data1, data2);
    }

    #[test]
    fn test_rc4_clone() {
        let key = b"clone_test_key";
        let mut c1 = Rc4::new(key);
        c1.discard(100);

        let mut c2 = c1.clone();

        let mut data1 = [0u8; 64];
        let mut data2 = [0u8; 64];
        c1.process_in_place(&mut data1);
        c2.process_in_place(&mut data2);
        assert_eq!(data1, data2);
    }

    #[test]
    fn test_rc4_empty_data() {
        let mut cipher = Rc4::new(b"key");
        let mut empty: [u8; 0] = [];
        cipher.process_in_place(&mut empty);
    }
}
