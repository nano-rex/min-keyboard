//! APK Signature Scheme v2 (<https://source.android.com/docs/security/features/apksigning/v2>).
//! The digests and the signing block are built here; the private-key operation is done by the
//! system `openssl` command, so no cryptography is hand-written.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::hash::sha256;
use crate::zip::Layout;

const CHUNK: usize = 1 << 20;
const V2_BLOCK_ID: u32 = 0x7109_871a;
const RSA_PKCS1_SHA256: u32 = 0x0103;
const ECDSA_SHA256: u32 = 0x0201;

/// Where the signing key comes from.
pub enum KeySource {
    /// A PKCS#12 keystore, such as `~/.android/debug.keystore` (password "android").
    Pkcs12 { path: PathBuf, password: String },
    /// A PEM private key and certificate.
    Pem { key: PathBuf, cert: PathBuf },
}

fn openssl(args: &[&str], input: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let mut child = Command::new("openssl")
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("cannot run openssl: {e}")))?;
    if let Some(data) = input {
        child.stdin.take().expect("piped stdin").write_all(data)?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "openssl {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

/// A key ready to sign with: the private key stays in OpenSSL's hands, passed by path.
pub struct Signer {
    key_args: Vec<String>,
    certificate: Vec<u8>,
    public_key: Vec<u8>,
    algorithm: u32,
}

impl Signer {
    pub fn new(source: &KeySource) -> io::Result<Self> {
        let (key_args, cert_pem) = match source {
            KeySource::Pkcs12 { path, password } => {
                let path = path.to_string_lossy().into_owned();
                let pass = format!("pass:{password}");
                let cert = openssl(&["pkcs12", "-in", &path, "-passin", &pass, "-nokeys", "-clcerts"], None)?;
                (vec!["-inkey".into(), path, "-passin".into(), pass], cert)
            }
            KeySource::Pem { key, cert } => (vec!["-inkey".into(), key.to_string_lossy().into_owned()], std::fs::read(cert)?),
        };
        let certificate = openssl(&["x509", "-outform", "DER"], Some(&cert_pem))?;
        let public_key = openssl(&["x509", "-noout", "-pubkey"], Some(&cert_pem))?;
        let public_key = openssl(&["pkey", "-pubin", "-outform", "DER"], Some(&public_key))?;
        let text = String::from_utf8_lossy(&openssl(&["x509", "-noout", "-text"], Some(&cert_pem))?).into_owned();
        let algorithm = if text.contains("rsaEncryption") {
            RSA_PKCS1_SHA256
        } else if text.contains("id-ecPublicKey") {
            ECDSA_SHA256
        } else {
            return Err(io::Error::other("only RSA and EC signing keys are supported"));
        };
        Ok(Signer { key_args, certificate, public_key, algorithm })
    }

    fn sign(&self, data: &[u8]) -> io::Result<Vec<u8>> {
        let mut args: Vec<&str> = vec!["pkeyutl", "-sign", "-rawin", "-digest", "sha256"];
        args.extend(self.key_args.iter().map(String::as_str));
        if self.algorithm == RSA_PKCS1_SHA256 {
            args.extend(["-pkeyopt", "rsa_padding_mode:pkcs1"]);
        }
        openssl(&args, Some(data))
    }

    /// Creates a self-signed debug keystore the way the Android tools do, if it does not exist.
    pub fn create_debug_keystore(path: &Path, password: &str) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let dir = std::env::temp_dir().join(format!("apkbuild-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let key = dir.join("key.pem");
        let cert = dir.join("cert.pem");
        let result = (|| {
            openssl(
                &[
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-nodes",
                    "-days",
                    "10950",
                    "-subj",
                    "/C=US/O=Android/CN=Android Debug",
                    "-keyout",
                    &key.to_string_lossy(),
                    "-out",
                    &cert.to_string_lossy(),
                ],
                None,
            )?;
            openssl(
                &[
                    "pkcs12",
                    "-export",
                    "-name",
                    "androiddebugkey",
                    "-inkey",
                    &key.to_string_lossy(),
                    "-in",
                    &cert.to_string_lossy(),
                    "-passout",
                    &format!("pass:{password}"),
                    "-out",
                    &path.to_string_lossy(),
                ],
                None,
            )
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result.map(|_| ())
    }
}

fn length_prefixed(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(data);
    out
}

fn sequence(items: &[Vec<u8>]) -> Vec<u8> {
    length_prefixed(&items.iter().flat_map(|item| length_prefixed(item)).collect::<Vec<u8>>())
}

/// The v2 content digest: SHA-256 over 1 MiB chunks of the entries, central directory and EOCD.
fn content_digest(sections: &[&[u8]]) -> [u8; 32] {
    let mut chunk_digests = Vec::new();
    let mut count = 0u32;
    for section in sections {
        for chunk in section.chunks(CHUNK) {
            let mut input = vec![0xa5];
            input.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            input.extend_from_slice(chunk);
            chunk_digests.extend_from_slice(&sha256(&input));
            count += 1;
        }
    }
    let mut top = vec![0x5a];
    top.extend_from_slice(&count.to_le_bytes());
    top.extend_from_slice(&chunk_digests);
    sha256(&top)
}

/// Inserts an APK Signing Block with a v2 signature between the entries and the central directory.
pub fn sign_v2(apk: &[u8], layout: &Layout, signer: &Signer) -> io::Result<Vec<u8>> {
    let entries = &apk[..layout.central_directory_offset];
    let central = &apk[layout.central_directory_offset..layout.end_of_central_directory_offset];
    let eocd = &apk[layout.end_of_central_directory_offset..];
    // The digest is computed as if the central directory still started where the block will begin,
    // which is exactly its current offset.
    let digest = content_digest(&[entries, central, eocd]);

    let digests = sequence(&[[signer.algorithm.to_le_bytes().to_vec(), length_prefixed(&digest)].concat()]);
    let certificates = sequence(std::slice::from_ref(&signer.certificate));
    let attributes = sequence(&[]);
    let signed_data = [digests, certificates, attributes].concat();
    let signature = signer.sign(&signed_data)?;
    let signatures = sequence(&[[signer.algorithm.to_le_bytes().to_vec(), length_prefixed(&signature)].concat()]);
    let signer_block = [length_prefixed(&signed_data), signatures, length_prefixed(&signer.public_key)].concat();
    let v2_value = sequence(&[signer_block]);

    let mut pair = (4 + v2_value.len() as u64).to_le_bytes().to_vec();
    pair.extend_from_slice(&V2_BLOCK_ID.to_le_bytes());
    pair.extend_from_slice(&v2_value);
    let block_size = (pair.len() + 8 + 16) as u64;
    let mut block = block_size.to_le_bytes().to_vec();
    block.extend_from_slice(&pair);
    block.extend_from_slice(&block_size.to_le_bytes());
    block.extend_from_slice(b"APK Sig Block 42");

    let mut out = Vec::with_capacity(apk.len() + block.len());
    out.extend_from_slice(entries);
    out.extend_from_slice(&block);
    out.extend_from_slice(central);
    let mut eocd = eocd.to_vec();
    let moved = (layout.central_directory_offset + block.len()) as u32;
    eocd[16..20].copy_from_slice(&moved.to_le_bytes());
    out.extend_from_slice(&eocd);
    Ok(out)
}
