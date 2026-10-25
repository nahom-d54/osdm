//! Per-user discovery for a native-messaging helper. Websites never receive the
//! loopback encryption key. Windows protects the descriptor with user-scoped DPAPI.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    nonce: String,
    body: String,
}

pub enum Direction {
    Request,
    Response,
}
impl Direction {
    fn aad(&self) -> &'static [u8] {
        match self {
            Self::Request => b"OSDM capture v1 request",
            Self::Response => b"OSDM capture v1 response",
        }
    }
}

fn cipher(token: &str) -> std::io::Result<XChaCha20Poly1305> {
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(std::io::Error::other("invalid bridge key"));
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&token[i * 2..i * 2 + 2], 16).map_err(std::io::Error::other)?;
    }
    Ok(XChaCha20Poly1305::new((&key).into()))
}

pub fn seal(
    token: &str,
    value: &serde_json::Value,
    direction: Direction,
) -> std::io::Result<Envelope> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 256 * 1024 {
        return Err(std::io::Error::other("bridge message too large"));
    }
    let cipher = cipher(token)?;
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let body = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &bytes,
                aad: direction.aad(),
            },
        )
        .map_err(|_| std::io::Error::other("bridge encryption failed"))?;
    Ok(Envelope {
        nonce: STANDARD.encode(nonce),
        body: STANDARD.encode(body),
    })
}

pub fn open(
    token: &str,
    envelope: &Envelope,
    direction: Direction,
) -> std::io::Result<serde_json::Value> {
    if envelope.body.len() > 360 * 1024 || envelope.nonce.len() > 64 {
        return Err(std::io::Error::other("bridge envelope too large"));
    }
    let nonce = STANDARD
        .decode(&envelope.nonce)
        .map_err(std::io::Error::other)?;
    if nonce.len() != 24 {
        return Err(std::io::Error::other("invalid bridge nonce"));
    }
    let body = STANDARD
        .decode(&envelope.body)
        .map_err(std::io::Error::other)?;
    if body.len() > 256 * 1024 + 16 {
        return Err(std::io::Error::other("bridge message too large"));
    }
    let bytes = cipher(token)?
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &body,
                aad: direction.aad(),
            },
        )
        .map_err(|_| std::io::Error::other("bridge authentication failed"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn descriptor_path() -> std::io::Result<PathBuf> {
    if let Some(directory) = native_test_directory()? {
        return Ok(directory.join("capture-bridge.bin"));
    }
    Ok(dirs::data_dir()
        .ok_or_else(|| std::io::Error::other("no application data directory"))?
        .join("app.osdm.desktop")
        .join("capture-bridge.bin"))
}

/// Debug-only, fail-closed isolation for disposable real-browser tests. Release
/// binaries never redirect application data based on this environment variable.
pub fn native_test_directory() -> std::io::Result<Option<PathBuf>> {
    #[cfg(debug_assertions)]
    {
        if let Some(directory) = std::env::var_os("OSDM_NATIVE_TEST_DATA") {
            return validate_test_directory(Path::new(&directory)).map(Some);
        }
    }
    Ok(None)
}

#[cfg(debug_assertions)]
fn validate_test_directory(directory: &Path) -> std::io::Result<PathBuf> {
    if !directory.is_absolute() || !directory.is_dir() {
        return Err(std::io::Error::other("invalid native test directory"));
    }
    let marker = directory.join(".osdm-native-test");
    let metadata = std::fs::symlink_metadata(&marker)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 64
        || std::fs::read(&marker)? != b"OSDM native test v1\n"
    {
        return Err(std::io::Error::other("native test marker required"));
    }
    std::fs::canonicalize(directory)
}

pub async fn forward(
    client: &reqwest::Client,
    message: &serde_json::Value,
) -> Option<serde_json::Value> {
    let endpoint = Endpoint::load(&descriptor_path().ok()?).ok()?;
    let envelope = seal(&endpoint.token, message, Direction::Request).ok()?;
    let mut response = client
        .post(format!("http://127.0.0.1:{}/v1/capture", endpoint.port))
        .json(&envelope)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 400 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let envelope: Envelope = serde_json::from_slice(&bytes).ok()?;
    open(&endpoint.token, &envelope, Direction::Response).ok()
}

impl Endpoint {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        }
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        let bytes = protect(&bytes, false)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(std::io::Error::other("unsafe descriptor path"));
        }
        let mut file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&bytes)?;
        file.sync_all()
    }
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(4097)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(std::io::Error::other("descriptor too large"));
        }
        let endpoint: Self = serde_json::from_slice(&protect(&bytes, true)?)?;
        if endpoint.port == 0
            || endpoint.token.len() != 64
            || !endpoint.token.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(std::io::Error::other("invalid descriptor"));
        }
        Ok(endpoint)
    }
}

#[cfg(windows)]
fn protect(bytes: &[u8], decrypt: bool) -> std::io::Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // Both calls allocate output with LocalAlloc, freed below; input remains live.
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(not(windows))]
fn protect(bytes: &[u8], _decrypt: bool) -> std::io::Result<Vec<u8>> {
    Ok(bytes.to_vec())
}

pub fn read_frame(reader: &mut impl Read) -> std::io::Result<Option<serde_json::Value>> {
    let mut header = [0u8; 4];
    match reader.read(&mut header[..1])? {
        0 => return Ok(None),
        _ => reader.read_exact(&mut header[1..])?,
    }
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > 256 * 1024 {
        return Err(std::io::Error::other("invalid native message size"));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

pub fn write_frame(writer: &mut impl Write, value: &serde_json::Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value)?;
    if body.len() > 256 * 1024 {
        return Err(std::io::Error::other("native response too large"));
    }
    writer.write_all(&(body.len() as u32).to_le_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_protected_descriptor_round_trip() {
        let value = serde_json::json!({"op":"ping"});
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        assert_eq!(read_frame(&mut bytes.as_slice()).unwrap(), Some(value));
        assert!(read_frame(&mut &b"\xff\xff\xff\xff"[..]).is_err());
        assert!(read_frame(&mut &b"\x04\x00"[..]).is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("endpoint");
        let endpoint = Endpoint::new(12345);
        endpoint.save(&path).unwrap();
        assert_eq!(Endpoint::load(&path).unwrap().token, endpoint.token);
        #[cfg(windows)]
        assert!(!String::from_utf8_lossy(&std::fs::read(path).unwrap()).contains(&endpoint.token));
    }

    #[test]
    fn encrypted_messages_bind_key_direction_and_content() {
        let endpoint = Endpoint::new(12345);
        let value = serde_json::json!({"op":"prepare","cookie":"private-fixture"});
        let mut envelope = seal(&endpoint.token, &value, Direction::Request).unwrap();
        assert_eq!(
            open(&endpoint.token, &envelope, Direction::Request).unwrap(),
            value
        );
        assert!(open(&endpoint.token, &envelope, Direction::Response).is_err());
        assert!(open(&Endpoint::new(12345).token, &envelope, Direction::Request).is_err());
        assert!(
            !serde_json::to_string(&envelope)
                .unwrap()
                .contains("private-fixture")
        );
        envelope.body.replace_range(..1, "!");
        assert!(open(&endpoint.token, &envelope, Direction::Request).is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn native_test_isolation_requires_absolute_owned_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_test_directory(dir.path()).is_err());
        assert!(validate_test_directory(Path::new("relative")).is_err());
        std::fs::write(
            dir.path().join(".osdm-native-test"),
            b"OSDM native test v1\n",
        )
        .unwrap();
        assert_eq!(
            validate_test_directory(dir.path()).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }
}
