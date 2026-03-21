//! MSE/PE (Message Stream Encryption / Protocol Encryption) implementation.
//!
//! Implements BEP 50/68 for obfuscating BitTorrent traffic using a Diffie-Hellman
//! key exchange followed by RC4 stream encryption.

mod rc4;

use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::{Context as _, bail};
use librtbit_core::hash_id::Id20;
use num_bigint::BigUint;
use num_traits::One;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tracing::{debug, trace};

use self::rc4::Rc4;

/// The 768-bit DH prime used by MSE.
const DH_PRIME_HEX: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A63A36210000000000090563";

/// DH generator.
const DH_GENERATOR: u32 = 2;

/// Number of bytes to discard from RC4 keystream (standard MSE requirement).
const RC4_DISCARD_BYTES: usize = 1024;

/// Maximum padding size.
const MAX_PAD_LEN: usize = 512;

/// Crypto methods bitmask: plaintext.
const CRYPTO_PLAINTEXT: u32 = 0x01;
/// Crypto methods bitmask: RC4.
const CRYPTO_RC4: u32 = 0x02;

/// Encryption mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EncryptionMode {
    /// No encryption attempted or accepted.
    Disabled,
    /// Prefer encrypted connections, but allow plaintext as fallback.
    #[default]
    Enabled,
    /// Require encryption; reject plaintext connections.
    Forced,
}

impl std::fmt::Display for EncryptionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncryptionMode::Disabled => f.write_str("disabled"),
            EncryptionMode::Enabled => f.write_str("enabled"),
            EncryptionMode::Forced => f.write_str("forced"),
        }
    }
}

impl std::str::FromStr for EncryptionMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disabled" => Ok(EncryptionMode::Disabled),
            "enabled" => Ok(EncryptionMode::Enabled),
            "forced" => Ok(EncryptionMode::Forced),
            other => bail!("invalid encryption mode: {other:?}, expected disabled/enabled/forced"),
        }
    }
}

/// Encryption status of a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EncryptionStatus {
    /// No encryption (plain BitTorrent protocol).
    Plaintext,
    /// RC4 encrypted via MSE/PE.
    Rc4,
}

impl std::fmt::Display for EncryptionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncryptionStatus::Plaintext => f.write_str("plaintext"),
            EncryptionStatus::Rc4 => f.write_str("rc4"),
        }
    }
}

fn dh_prime() -> BigUint {
    BigUint::parse_bytes(DH_PRIME_HEX.as_bytes(), 16).expect("hardcoded DH prime is valid")
}

/// Generate a DH keypair: (private_key, public_key).
fn dh_keypair() -> (BigUint, BigUint) {
    let p = dh_prime();
    let g = BigUint::from(DH_GENERATOR);

    // Generate a random 160-bit private key (standard MSE uses 160 bits).
    let mut priv_bytes = [0u8; 20];
    rand::rng().fill_bytes(&mut priv_bytes);
    let private_key = BigUint::from_bytes_be(&priv_bytes);

    // public_key = g^private_key mod p
    let public_key = g.modpow(&private_key, &p);

    (private_key, public_key)
}

/// Compute shared secret: S = other_public^my_private mod p.
fn dh_secret(my_private: &BigUint, other_public: &BigUint) -> BigUint {
    let p = dh_prime();
    other_public.modpow(my_private, &p)
}

/// Encode a BigUint as a 96-byte big-endian array (768 bits), zero-padding on the left.
fn biguint_to_96_bytes(n: &BigUint) -> [u8; 96] {
    let bytes = n.to_bytes_be();
    let mut out = [0u8; 96];
    if bytes.len() <= 96 {
        out[96 - bytes.len()..].copy_from_slice(&bytes);
    } else {
        // Should not happen with 768-bit prime, but truncate if it does.
        out.copy_from_slice(&bytes[bytes.len() - 96..]);
    }
    out
}

/// SHA-1 of multiple slices concatenated.
fn sha1_multi(slices: &[&[u8]]) -> [u8; 20] {
    use sha1w::ISha1;
    let mut hasher = sha1w::Sha1::new();
    for s in slices {
        hasher.update(s);
    }
    hasher.finish()
}

/// Generate random padding of length 0..=MAX_PAD_LEN.
fn random_pad() -> Vec<u8> {
    let len = (rand::random::<u16>() as usize) % (MAX_PAD_LEN + 1);
    let mut buf = vec![0u8; len];
    rand::rng().fill_bytes(&mut buf);
    buf
}

/// Result of the MSE handshake.
pub struct MseHandshakeResult<R, W> {
    pub reader: R,
    pub writer: W,
    pub encryption_status: EncryptionStatus,
}

/// Perform MSE handshake as the initiator (outgoing connection).
///
/// After this completes, the returned reader/writer are ready for the regular
/// BitTorrent handshake (which happens inside the encrypted stream).
pub async fn mse_handshake_initiator<R, W>(
    mut reader: R,
    mut writer: W,
    info_hash: Id20,
    encryption_mode: EncryptionMode,
) -> anyhow::Result<MseHandshakeResult<EncryptedReader<R>, EncryptedWriter<W>>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let skey = info_hash.0;

    // Step 1: DH key exchange
    let (my_private, my_public) = dh_keypair();
    let my_pub_bytes = biguint_to_96_bytes(&my_public);

    // Send: Ya + PadA
    let pad_a = random_pad();
    writer.write_all(&my_pub_bytes).await.context("sending Ya")?;
    writer.write_all(&pad_a).await.context("sending PadA")?;
    writer.flush().await?;

    trace!("MSE initiator: sent Ya ({} bytes) + PadA ({} bytes)", my_pub_bytes.len(), pad_a.len());

    // Receive: Yb (96 bytes)
    let mut yb_bytes = [0u8; 96];
    reader.read_exact(&mut yb_bytes).await.context("reading Yb")?;
    let other_public = BigUint::from_bytes_be(&yb_bytes);

    // Validate: Yb must not be 0 or 1 (weak keys)
    if other_public.is_one() || other_public == BigUint::ZERO {
        bail!("MSE: received weak DH public key");
    }

    // Step 2: Compute shared secret
    let secret = dh_secret(&my_private, &other_public);
    let s_bytes = biguint_to_96_bytes(&secret);

    trace!("MSE initiator: computed shared secret");

    // Derive keys
    let req1 = sha1_multi(&[b"req1", &s_bytes]);
    let req2 = sha1_multi(&[b"req2", &skey]);
    let req3 = sha1_multi(&[b"req3", &s_bytes]);

    // HASH('keyA', S, SKEY) - initiator's encrypt key
    let key_a = sha1_multi(&[b"keyA", &s_bytes, &skey]);
    // HASH('keyB', S, SKEY) - initiator's decrypt key
    let key_b = sha1_multi(&[b"keyB", &s_bytes, &skey]);

    // Create RC4 ciphers (discard first 1024 bytes)
    let mut encrypt_rc4 = Rc4::new(&key_a);
    encrypt_rc4.discard(RC4_DISCARD_BYTES);
    let mut decrypt_rc4 = Rc4::new(&key_b);
    decrypt_rc4.discard(RC4_DISCARD_BYTES);

    // Step 3: Send HASH('req1', S) + HASH('req2', SKEY) XOR HASH('req3', S)
    writer.write_all(&req1).await.context("sending req1")?;

    let mut req2_xor_req3 = [0u8; 20];
    for i in 0..20 {
        req2_xor_req3[i] = req2[i] ^ req3[i];
    }
    writer.write_all(&req2_xor_req3).await.context("sending req2^req3")?;

    // Step 4: Send encrypted payload:
    // ENCRYPT(VC, crypto_provide, len(PadC), PadC, len(IA))
    let vc = [0u8; 8];
    let crypto_provide = if encryption_mode == EncryptionMode::Forced {
        CRYPTO_RC4
    } else {
        CRYPTO_PLAINTEXT | CRYPTO_RC4
    };

    let pad_c = random_pad();

    let mut payload = Vec::new();
    payload.extend_from_slice(&vc);
    payload.extend_from_slice(&crypto_provide.to_be_bytes());
    payload.extend_from_slice(&(pad_c.len() as u16).to_be_bytes());
    payload.extend_from_slice(&pad_c);
    // IA length = 0 (we'll send the BT handshake after MSE completes)
    payload.extend_from_slice(&0u16.to_be_bytes());

    encrypt_rc4.process_in_place(&mut payload);
    writer.write_all(&payload).await.context("sending encrypted initiator payload")?;
    writer.flush().await?;

    trace!("MSE initiator: sent encrypted payload");

    // Step 5: Read responder's encrypted stream.
    // We need to read until we find the encrypted VC (8 zero bytes encrypted with decrypt_rc4).
    // But first, we may need to skip PadB from the responder.
    // The responder sends: ENCRYPT(VC, crypto_select, len(PadD), PadD)
    //
    // We need to scan for VC. We have a synchronized decrypt_rc4 stream,
    // so we read byte by byte and look for 8 consecutive zero bytes after decryption.
    //
    // Actually, the responder may also send PadB (unencrypted) before the encrypted part.
    // We need to skip until we find VC in the decrypted stream.
    // The tricky part: we don't know where PadB ends and the encrypted part begins.
    // But we know the responder's encrypted VC will be the first 8 bytes of their encrypted stream.
    //
    // Per the MSE spec, after the responder sends Yb + PadB, they send ENCRYPT2(VC, ...).
    // Since we've already read Yb (96 bytes), we need to read and discard PadB.
    // PadB is up to 512 bytes. We scan for the encrypted VC pattern.
    //
    // We compute what the encrypted VC would look like (8 zero bytes encrypted by decrypt_rc4
    // at the current position), then scan the incoming stream for that pattern.

    // Compute what encrypted-VC looks like (VC=0, so it's just the RC4 keystream)
    let mut expected_enc_vc = [0u8; 8];
    {
        let mut temp_rc4 = decrypt_rc4.clone();
        temp_rc4.process_in_place(&mut expected_enc_vc);
    }

    // Now scan the incoming stream for expected_enc_vc
    // Read one byte at a time into a sliding window
    let mut window = [0u8; 8];
    let mut total_read = 0usize;

    // Fill the window initially
    reader.read_exact(&mut window).await.context("reading initial scan bytes")?;
    total_read += 8;

    loop {
        if window == expected_enc_vc {
            // Found VC! Advance decrypt_rc4 past the VC.
            let pad_b_len = total_read - 8;
            trace!("MSE initiator: found VC after PadB of {} bytes", pad_b_len);

            // Decrypt the VC (consume it from the RC4 stream)
            let mut vc_dec = window;
            decrypt_rc4.process_in_place(&mut vc_dec);
            // vc_dec should now be all zeros
            break;
        }

        if total_read >= MAX_PAD_LEN + 8 {
            bail!("MSE initiator: could not find VC within {} bytes", MAX_PAD_LEN + 8);
        }

        // Slide window: shift left by 1, read 1 new byte
        window.copy_within(1.., 0);
        let mut one = [0u8; 1];
        reader.read_exact(&mut one).await.context("scanning for VC")?;
        window[7] = one[0];
        total_read += 1;
    }

    // Read crypto_select (4 bytes, encrypted)
    let mut crypto_select_enc = [0u8; 4];
    reader.read_exact(&mut crypto_select_enc).await.context("reading crypto_select")?;
    decrypt_rc4.process_in_place(&mut crypto_select_enc);
    let crypto_select = u32::from_be_bytes(crypto_select_enc);

    trace!("MSE initiator: crypto_select = 0x{:x}", crypto_select);

    // Read len(PadD) (2 bytes, encrypted)
    let mut pad_d_len_enc = [0u8; 2];
    reader.read_exact(&mut pad_d_len_enc).await.context("reading PadD length")?;
    decrypt_rc4.process_in_place(&mut pad_d_len_enc);
    let pad_d_len = u16::from_be_bytes(pad_d_len_enc) as usize;

    if pad_d_len > MAX_PAD_LEN {
        bail!("MSE initiator: PadD too long: {}", pad_d_len);
    }

    // Read and discard PadD
    if pad_d_len > 0 {
        let mut pad_d = vec![0u8; pad_d_len];
        reader.read_exact(&mut pad_d).await.context("reading PadD")?;
        decrypt_rc4.process_in_place(&mut pad_d);
    }

    // Determine encryption status
    let encryption_status = if crypto_select & CRYPTO_RC4 != 0 {
        EncryptionStatus::Rc4
    } else if crypto_select & CRYPTO_PLAINTEXT != 0 {
        if encryption_mode == EncryptionMode::Forced {
            bail!("MSE initiator: peer selected plaintext but we require encryption");
        }
        EncryptionStatus::Plaintext
    } else {
        bail!("MSE initiator: peer selected unknown crypto method: 0x{:x}", crypto_select);
    };

    debug!("MSE initiator: handshake complete, encryption: {}", encryption_status);

    // Wrap the streams
    let (enc_reader, enc_writer) = match encryption_status {
        EncryptionStatus::Rc4 => (
            EncryptedReader::new(reader, Some(decrypt_rc4)),
            EncryptedWriter::new(writer, Some(encrypt_rc4)),
        ),
        EncryptionStatus::Plaintext => (
            EncryptedReader::new(reader, None),
            EncryptedWriter::new(writer, None),
        ),
    };

    Ok(MseHandshakeResult {
        reader: enc_reader,
        writer: enc_writer,
        encryption_status,
    })
}

/// Perform MSE handshake as the responder (incoming connection).
///
/// `info_hashes` is the list of active info hashes to try matching against.
/// Returns the matched info_hash along with the wrapped streams.
pub async fn mse_handshake_responder<R, W>(
    mut reader: R,
    mut writer: W,
    info_hashes: &[Id20],
    encryption_mode: EncryptionMode,
) -> anyhow::Result<(Id20, MseHandshakeResult<EncryptedReader<R>, EncryptedWriter<W>>)>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if info_hashes.is_empty() {
        bail!("MSE responder: no active info hashes to match against");
    }

    // Step 1: Read Ya (96 bytes)
    let mut ya_bytes = [0u8; 96];
    reader.read_exact(&mut ya_bytes).await.context("reading Ya")?;
    let other_public = BigUint::from_bytes_be(&ya_bytes);

    if other_public.is_one() || other_public == BigUint::ZERO {
        bail!("MSE: received weak DH public key");
    }

    // Generate our DH keypair
    let (my_private, my_public) = dh_keypair();
    let my_pub_bytes = biguint_to_96_bytes(&my_public);

    // Send: Yb + PadB
    let pad_b = random_pad();
    writer.write_all(&my_pub_bytes).await.context("sending Yb")?;
    writer.write_all(&pad_b).await.context("sending PadB")?;
    writer.flush().await?;

    trace!("MSE responder: sent Yb + PadB ({} bytes)", pad_b.len());

    // Step 2: Compute shared secret
    let secret = dh_secret(&my_private, &other_public);
    let s_bytes = biguint_to_96_bytes(&secret);

    // Step 3: Read and scan for HASH('req1', S)
    let expected_req1 = sha1_multi(&[b"req1", &s_bytes]);
    let req3 = sha1_multi(&[b"req3", &s_bytes]);

    // We need to scan past PadA to find req1.
    // Read bytes and look for the 20-byte req1 hash.
    let mut scan_window = [0u8; 20];
    reader.read_exact(&mut scan_window).await.context("reading initial req1 scan")?;
    let mut total_scanned = 20usize;

    loop {
        if scan_window == expected_req1 {
            let pad_a_len = total_scanned - 20;
            trace!("MSE responder: found req1 after PadA of {} bytes", pad_a_len);
            break;
        }
        if total_scanned >= MAX_PAD_LEN + 20 {
            bail!("MSE responder: could not find req1 within {} bytes", MAX_PAD_LEN + 20);
        }
        scan_window.copy_within(1.., 0);
        let mut one = [0u8; 1];
        reader.read_exact(&mut one).await.context("scanning for req1")?;
        scan_window[19] = one[0];
        total_scanned += 1;
    }

    // Read HASH('req2', SKEY) XOR HASH('req3', S) (20 bytes)
    let mut req2_xor_req3 = [0u8; 20];
    reader.read_exact(&mut req2_xor_req3).await.context("reading req2^req3")?;

    // Try each info_hash to find matching SKEY
    let mut matched_info_hash = None;
    for &ih in info_hashes {
        let skey = ih.0;
        let req2 = sha1_multi(&[b"req2", &skey]);
        let mut expected = [0u8; 20];
        for i in 0..20 {
            expected[i] = req2[i] ^ req3[i];
        }
        if expected == req2_xor_req3 {
            matched_info_hash = Some(ih);
            break;
        }
    }

    let info_hash = matched_info_hash.context("MSE responder: no matching info hash found")?;
    let skey = info_hash.0;

    trace!("MSE responder: matched info hash {:?}", info_hash);

    // Derive keys (note: for responder, keyA is decrypt, keyB is encrypt)
    let key_a = sha1_multi(&[b"keyA", &s_bytes, &skey]);
    let key_b = sha1_multi(&[b"keyB", &s_bytes, &skey]);

    let mut decrypt_rc4 = Rc4::new(&key_a); // decrypt what initiator encrypted
    decrypt_rc4.discard(RC4_DISCARD_BYTES);
    let mut encrypt_rc4 = Rc4::new(&key_b); // encrypt for initiator to decrypt
    encrypt_rc4.discard(RC4_DISCARD_BYTES);

    // Step 4: Read encrypted payload from initiator
    // ENCRYPT(VC, crypto_provide, len(PadC), PadC, len(IA), IA)

    // Read VC (8 bytes, encrypted)
    let mut vc_enc = [0u8; 8];
    reader.read_exact(&mut vc_enc).await.context("reading encrypted VC")?;
    decrypt_rc4.process_in_place(&mut vc_enc);

    if vc_enc != [0u8; 8] {
        bail!("MSE responder: VC verification failed");
    }

    // Read crypto_provide (4 bytes, encrypted)
    let mut crypto_provide_enc = [0u8; 4];
    reader.read_exact(&mut crypto_provide_enc).await.context("reading crypto_provide")?;
    decrypt_rc4.process_in_place(&mut crypto_provide_enc);
    let crypto_provide = u32::from_be_bytes(crypto_provide_enc);

    trace!("MSE responder: crypto_provide = 0x{:x}", crypto_provide);

    // Read len(PadC) (2 bytes, encrypted)
    let mut pad_c_len_enc = [0u8; 2];
    reader.read_exact(&mut pad_c_len_enc).await.context("reading PadC length")?;
    decrypt_rc4.process_in_place(&mut pad_c_len_enc);
    let pad_c_len = u16::from_be_bytes(pad_c_len_enc) as usize;

    if pad_c_len > MAX_PAD_LEN {
        bail!("MSE responder: PadC too long: {}", pad_c_len);
    }

    // Read PadC
    if pad_c_len > 0 {
        let mut pad_c = vec![0u8; pad_c_len];
        reader.read_exact(&mut pad_c).await.context("reading PadC")?;
        decrypt_rc4.process_in_place(&mut pad_c);
    }

    // Read len(IA) (2 bytes, encrypted)
    let mut ia_len_enc = [0u8; 2];
    reader.read_exact(&mut ia_len_enc).await.context("reading IA length")?;
    decrypt_rc4.process_in_place(&mut ia_len_enc);
    let ia_len = u16::from_be_bytes(ia_len_enc) as usize;

    // Read IA (initial payload from initiator, if any)
    let mut ia = vec![0u8; ia_len];
    if ia_len > 0 {
        reader.read_exact(&mut ia).await.context("reading IA")?;
        decrypt_rc4.process_in_place(&mut ia);
    }

    // Step 5: Select crypto method and send response
    let crypto_select = if encryption_mode == EncryptionMode::Forced {
        if crypto_provide & CRYPTO_RC4 == 0 {
            bail!("MSE responder: peer doesn't support RC4, but we require encryption");
        }
        CRYPTO_RC4
    } else if crypto_provide & CRYPTO_RC4 != 0 {
        // Prefer RC4 if available
        CRYPTO_RC4
    } else if crypto_provide & CRYPTO_PLAINTEXT != 0 {
        CRYPTO_PLAINTEXT
    } else {
        bail!("MSE responder: no compatible crypto method (provide=0x{:x})", crypto_provide);
    };

    // Send: ENCRYPT2(VC, crypto_select, len(PadD), PadD)
    let pad_d = random_pad();
    let mut response = Vec::new();
    response.extend_from_slice(&[0u8; 8]); // VC
    response.extend_from_slice(&crypto_select.to_be_bytes());
    response.extend_from_slice(&(pad_d.len() as u16).to_be_bytes());
    response.extend_from_slice(&pad_d);

    encrypt_rc4.process_in_place(&mut response);
    writer.write_all(&response).await.context("sending responder crypto response")?;
    writer.flush().await?;

    let encryption_status = if crypto_select == CRYPTO_RC4 {
        EncryptionStatus::Rc4
    } else {
        EncryptionStatus::Plaintext
    };

    debug!("MSE responder: handshake complete, encryption: {}", encryption_status);

    // Build the encrypted reader, prepending any IA data
    let (enc_reader, enc_writer) = match encryption_status {
        EncryptionStatus::Rc4 => (
            EncryptedReader::with_prefix(reader, Some(decrypt_rc4), ia),
            EncryptedWriter::new(writer, Some(encrypt_rc4)),
        ),
        EncryptionStatus::Plaintext => (
            EncryptedReader::with_prefix(reader, None, ia),
            EncryptedWriter::new(writer, None),
        ),
    };

    Ok((info_hash, MseHandshakeResult {
        reader: enc_reader,
        writer: enc_writer,
        encryption_status,
    }))
}

/// Detect if an incoming connection is using MSE or plain BitTorrent protocol.
///
/// Plain BT connections start with `\x13BitTorrent protocol`.
/// MSE connections start with random bytes (the DH public key).
///
/// Returns true if the first byte looks like MSE (not 0x13).
pub async fn detect_mse<R: AsyncRead + Unpin>(reader: &mut R, peek_buf: &mut [u8; 1]) -> anyhow::Result<bool> {
    reader.read_exact(peek_buf).await.context("reading first byte for MSE detection")?;
    // The BitTorrent handshake starts with byte 19 (0x13) which is the length of "BitTorrent protocol".
    // MSE starts with random DH public key bytes.
    Ok(peek_buf[0] != 0x13)
}

/// An async reader that optionally decrypts data using RC4.
pub struct EncryptedReader<R> {
    inner: R,
    cipher: Option<Rc4>,
    /// Prefix buffer for data already decrypted during handshake (e.g., IA payload).
    prefix: Vec<u8>,
    prefix_pos: usize,
}

impl<R> EncryptedReader<R> {
    pub fn new(inner: R, cipher: Option<Rc4>) -> Self {
        Self {
            inner,
            cipher,
            prefix: Vec::new(),
            prefix_pos: 0,
        }
    }

    pub fn with_prefix(inner: R, cipher: Option<Rc4>, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            cipher,
            prefix,
            prefix_pos: 0,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for EncryptedReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // First, drain any prefix data
        if this.prefix_pos < this.prefix.len() {
            let remaining = &this.prefix[this.prefix_pos..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            this.prefix_pos += to_copy;
            return Poll::Ready(Ok(()));
        }

        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);

        if let Poll::Ready(Ok(())) = &result {
            if let Some(cipher) = &mut this.cipher {
                let filled = buf.filled_mut();
                let new_data = &mut filled[before..];
                cipher.process_in_place(new_data);
            }
        }

        result
    }
}

/// An async writer that optionally encrypts data using RC4.
///
/// Handles partial writes correctly by buffering encrypted data so that
/// the RC4 cipher state stays synchronized with what has actually been sent.
pub struct EncryptedWriter<W> {
    inner: W,
    cipher: Option<Rc4>,
    /// Buffer for encrypted data pending write.
    /// Non-empty means we have a partial write in progress.
    pending: Vec<u8>,
    /// How many bytes of `pending` have been written so far.
    pending_written: usize,
}

impl<W> EncryptedWriter<W> {
    pub fn new(inner: W, cipher: Option<Rc4>) -> Self {
        Self {
            inner,
            cipher,
            pending: Vec::new(),
            pending_written: 0,
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for EncryptedWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        if this.cipher.is_none() {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }

        // If we have pending encrypted data from a previous partial write, finish it first.
        if !this.pending.is_empty() {
            let remaining = &this.pending[this.pending_written..];
            match Pin::new(&mut this.inner).poll_write(cx, remaining) {
                Poll::Ready(Ok(n)) => {
                    this.pending_written += n;
                    if this.pending_written >= this.pending.len() {
                        // All pending data written. Return the original buf length.
                        let total = this.pending.len();
                        this.pending.clear();
                        this.pending_written = 0;
                        return Poll::Ready(Ok(total));
                    }
                    // Still more pending data to write - register for wakeup.
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }

        // Encrypt the data
        let cipher = this.cipher.as_mut().unwrap();
        let mut encrypted = buf.to_vec();
        cipher.process_in_place(&mut encrypted);

        match Pin::new(&mut this.inner).poll_write(cx, &encrypted) {
            Poll::Ready(Ok(n)) => {
                if n >= encrypted.len() {
                    // All data written
                    Poll::Ready(Ok(buf.len()))
                } else {
                    // Partial write - store remaining encrypted data
                    this.pending = encrypted;
                    this.pending_written = n;
                    // Wake ourselves to continue writing the pending data
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Poll::Ready(Err(e)) => {
                // The write failed but we've already advanced the cipher.
                // We need to report this as an error. The stream is now in an
                // inconsistent state and should be closed.
                Poll::Ready(Err(e))
            }
            Poll::Pending => {
                // The inner writer wasn't ready. But we've already encrypted
                // the data and advanced the cipher. Store it as pending.
                this.pending = encrypted;
                this.pending_written = 0;
                Poll::Pending
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    #[test]
    fn test_encryption_mode_parsing() {
        assert_eq!("disabled".parse::<EncryptionMode>().unwrap(), EncryptionMode::Disabled);
        assert_eq!("enabled".parse::<EncryptionMode>().unwrap(), EncryptionMode::Enabled);
        assert_eq!("forced".parse::<EncryptionMode>().unwrap(), EncryptionMode::Forced);
        assert!("invalid".parse::<EncryptionMode>().is_err());
    }

    #[test]
    fn test_dh_keypair_generation() {
        let (priv_key, pub_key) = dh_keypair();
        assert!(!priv_key.is_one());
        assert!(!pub_key.is_one());
        assert!(pub_key < dh_prime());
    }

    #[test]
    fn test_dh_shared_secret() {
        let (priv_a, pub_a) = dh_keypair();
        let (priv_b, pub_b) = dh_keypair();
        let secret_a = dh_secret(&priv_a, &pub_b);
        let secret_b = dh_secret(&priv_b, &pub_a);
        assert_eq!(secret_a, secret_b);
    }

    #[test]
    fn test_biguint_to_96_bytes() {
        let n = BigUint::from(255u32);
        let bytes = biguint_to_96_bytes(&n);
        assert_eq!(bytes[95], 255);
        assert_eq!(bytes[94], 0);
    }

    #[tokio::test]
    async fn test_encrypted_reader_writer_rc4() {
        let key = b"test_encryption_key_1234";
        let plaintext = b"Hello, encrypted world! This is a test of the RC4 stream.";

        let (client, server) = duplex(4096);
        let (server_read, server_write) = tokio::io::split(server);
        let (client_read, client_write) = tokio::io::split(client);

        let mut enc_writer = EncryptedWriter::new(client_write, Some(Rc4::new(key)));
        let mut enc_reader = EncryptedReader::new(server_read, Some(Rc4::new(key)));

        // Write encrypted
        enc_writer.write_all(plaintext).await.unwrap();
        enc_writer.flush().await.unwrap();
        drop(enc_writer);

        // Read and decrypt
        let mut result = vec![0u8; plaintext.len()];
        enc_reader.read_exact(&mut result).await.unwrap();
        assert_eq!(&result, plaintext);
    }

    #[tokio::test]
    async fn test_encrypted_reader_with_prefix() {
        let prefix = b"prefix_data".to_vec();
        let stream_data = b"stream_data";

        let (client, server) = duplex(4096);
        let (server_read, _) = tokio::io::split(server);
        let (_, mut client_write) = tokio::io::split(client);

        client_write.write_all(stream_data).await.unwrap();
        drop(client_write);

        let mut reader = EncryptedReader::with_prefix(server_read, None, prefix.clone());
        let mut result = vec![0u8; prefix.len() + stream_data.len()];
        reader.read_exact(&mut result).await.unwrap();
        assert_eq!(&result[..prefix.len()], &prefix[..]);
        assert_eq!(&result[prefix.len()..], stream_data);
    }

    #[tokio::test]
    async fn test_mse_handshake_roundtrip() {
        let info_hash = Id20::new([0xAA; 20]);
        let info_hashes = vec![info_hash];

        let (client_stream, server_stream) = duplex(65536);
        let (server_read, server_write) = tokio::io::split(server_stream);
        let (client_read, client_write) = tokio::io::split(client_stream);

        let initiator = tokio::spawn(async move {
            mse_handshake_initiator(client_read, client_write, info_hash, EncryptionMode::Enabled)
                .await
                .expect("initiator handshake failed")
        });

        let responder = tokio::spawn(async move {
            mse_handshake_responder(server_read, server_write, &info_hashes, EncryptionMode::Enabled)
                .await
                .expect("responder handshake failed")
        });

        let (init_result, resp_result) = tokio::join!(initiator, responder);
        let mut init_result = init_result.unwrap();
        let (matched_hash, mut resp_result) = resp_result.unwrap();

        assert_eq!(matched_hash, info_hash);
        assert_eq!(init_result.encryption_status, EncryptionStatus::Rc4);
        assert_eq!(resp_result.encryption_status, EncryptionStatus::Rc4);

        // Test that data can flow through the encrypted channel
        let test_data = b"Hello through encrypted channel!";

        init_result.writer.write_all(test_data).await.unwrap();
        init_result.writer.flush().await.unwrap();

        let mut buf = vec![0u8; test_data.len()];
        resp_result.reader.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, test_data);

        // Test reverse direction
        resp_result.writer.write_all(test_data).await.unwrap();
        resp_result.writer.flush().await.unwrap();

        let mut buf2 = vec![0u8; test_data.len()];
        init_result.reader.read_exact(&mut buf2).await.unwrap();
        assert_eq!(&buf2, test_data);
    }

    #[tokio::test]
    async fn test_mse_forced_encryption() {
        let info_hash = Id20::new([0xBB; 20]);
        let info_hashes = vec![info_hash];

        let (client_stream, server_stream) = duplex(65536);
        let (server_read, server_write) = tokio::io::split(server_stream);
        let (client_read, client_write) = tokio::io::split(client_stream);

        let initiator = tokio::spawn(async move {
            mse_handshake_initiator(client_read, client_write, info_hash, EncryptionMode::Forced)
                .await
                .expect("initiator handshake failed")
        });

        let responder = tokio::spawn(async move {
            mse_handshake_responder(server_read, server_write, &info_hashes, EncryptionMode::Forced)
                .await
                .expect("responder handshake failed")
        });

        let (init_result, resp_result) = tokio::join!(initiator, responder);
        let init_result = init_result.unwrap();
        let (_, resp_result) = resp_result.unwrap();

        assert_eq!(init_result.encryption_status, EncryptionStatus::Rc4);
        assert_eq!(resp_result.encryption_status, EncryptionStatus::Rc4);
    }
}
