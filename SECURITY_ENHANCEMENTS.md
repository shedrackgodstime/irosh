# 🔐 Irosh Security Enhancements Proposal

**Status**: 📋 Proposal for Review  
**Created**: 2026-05-06  
**Target Audience**: Security reviewers, maintainers, contributors

---

## Executive Summary

This document outlines a comprehensive, **layered security architecture** to maximize the security posture of Irosh. It builds on the project's existing strong cryptographic foundations (Ed25519, QUIC encryption, TOFU trust model) and proposes targeted enhancements across 10 key security domains.

The recommendations follow a **defense-in-depth** strategy, combining:
- Cryptographic hardening
- Access control enforcement
- Audit trails and forensics
- Operational security practices
- Configuration-driven policies

**Key Goal**: Transform Irosh into a reference implementation for secure P2P remote access.

---

## Table of Contents

1. [Cryptographic Foundation](#1-cryptographic-foundation)
2. [Authentication & Authorization](#2-authentication--authorization)
3. [Trust Store & Key Management](#3-trust-store--key-management)
4. [Transport Security](#4-transport-security)
5. [Wormhole (Pairing) Security](#5-wormhole-pairing-security)
6. [Session & Connection Security](#6-session--connection-security)
7. [File Transfer Security](#7-file-transfer-security)
8. [Audit & Logging](#8-audit--logging)
9. [Configuration & Security Policies](#9-configuration--security-policies)
10. [Implementation Checklist](#10-implementation-checklist)

---

## 1. Cryptographic Foundation

### Current Implementation ✅

```rust
// src/storage/keys.rs
let secret_key = SecretKey::generate(&mut rand::rng());  // Cryptographically secure
let keypair = Ed25519Keypair::from_seed(&seed);          // Standard Ed25519
```

**Existing Strengths:**
- ✅ **Unified Identity**: Same secret seed derives both Iroh network identity and SSH keys
- ✅ **Ed25519**: Modern elliptic-curve cryptography (128-bit security level)
- ✅ **Argon2 Password Hashing**: Resistant key derivation from `src/auth.rs`
- ✅ **CSPRNG**: Uses `rand::rng()` for cryptographically secure entropy

### Recommended Enhancements

#### 1.1 Secret Material Zeroization

**Risk**: Sensitive secrets may linger in memory after use, exposing them to privilege escalation attacks.

**Solution**: Use the `zeroize` crate to securely wipe sensitive material:

```rust
use zeroize::Zeroize;

fn load_identity_securely(state: &StateConfig) -> Result<NodeIdentity> {
    let mut secret_bytes = fs::read_to_string(SECRET_KEY_FILE)?;
    
    // ... process secret_bytes ...
    
    // Explicitly wipe from memory before dropping
    secret_bytes.zeroize();
    Ok(identity)
}

impl Drop for NodeIdentity {
    fn drop(&mut self) {
        // Ensure key material is wiped on drop
        // (This would require adjusting the struct layout)
    }
}
```

**Priority**: 🔴 **High** (fundamental memory safety)  
**Effort**: ⏱️ Low (add 1-2 dependencies, apply to key functions)

#### 1.2 Restrict Key File Permissions (Unix)

**Risk**: Key files with overly permissive modes can be read by other users on shared systems.

**Solution**: Enforce strict permissions (0o600 = read/write owner only):

```rust
#[cfg(unix)]
fn set_strict_permissions(path: &Path) -> Result<()> {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    
    let perms = Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    
    tracing::info!(path = ?path, "set permissions to 0o600");
    Ok(())
}

pub fn load_or_generate_identity_blocking(state: &StateConfig) -> Result<NodeIdentity> {
    // ... existing code ...
    
    if !path.exists() {
        let secret_key = SecretKey::generate(&mut rand::rng());
        let hex = format_as_hex(&secret_key.to_bytes());
        fs::write(&path, hex)?;
        
        // NEW: Set strict permissions immediately after creation
        set_strict_permissions(&path)?;
        
        secret_key
    } else {
        // Verify existing file has correct permissions
        let metadata = fs::metadata(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            if mode & 0o077 != 0 {
                tracing::warn!("key file has overly permissive mode: {:o}", mode);
                set_strict_permissions(&path)?;
            }
        }
        
        // ... load existing key ...
    }
}
```

**Priority**: 🔴 **High** (prevents unauthorized key access)  
**Effort**: ⏱️ Trivial (3-4 lines per write)

#### 1.3 Hardware Security Module (HSM) Support (Future)

**Risk**: For enterprise deployments, secrets stored on the filesystem are vulnerable to physical attacks.

**Solution**: Add optional HSM support for key storage:

```rust
pub enum KeyBackend {
    /// Filesystem-based (current default)
    Filesystem { path: PathBuf },
    /// PKCS#11-compatible HSM (e.g., YubiHSM, AWS CloudHSM)
    Pkcs11 { slot_id: u32 },
    /// TPM 2.0 (Linux systems with hardware TPM)
    Tpm2,
}

pub struct SecureKeyStorage {
    backend: KeyBackend,
}
```

**Priority**: 🟡 **Medium** (enterprise feature)  
**Effort**: ⏱️ High (requires PKCS#11 bindings)

---

## 2. Authentication & Authorization

### Current Implementation ✅

```rust
// src/auth.rs - Pluggable authentication trait
pub trait Authenticator: Send + Sync + fmt::Debug {
    fn supported_methods(&self) -> Vec<AuthMethod>;
    fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool>;
    fn check_password(&self, user: &str, password: &str) -> Result<bool>;
}
```

**Existing Strengths:**
- ✅ Pluggable auth backends (KeyOnly, Password, Combined)
- ✅ TOFU-based trust model
- ✅ Argon2 password hashing

### Security Matrix

| Layer | Mechanism | Status | Risk Level |
|-------|-----------|--------|-----------|
| **Transport** | Ed25519 node identity | ✅ Implemented | 🟢 Low |
| **Network** | QUIC encryption (Iroh) | ✅ Built-in | 🟢 Low |
| **TOFU** | Host key pinning (client-side) | ✅ Implemented | 🟢 Low |
| **TOFU** | Client key whitelist (server-side) | ✅ Implemented | 🟢 Low |
| **Application** | Pluggable authenticators | ✅ Implemented | 🟡 Medium |
| **Rate Limiting** | Auth attempt throttling | ❌ Missing | 🔴 High |
| **MFA** | Multi-factor authentication | ❌ Missing | 🔴 High |
| **Timeouts** | Session expiration | ❌ Missing | 🔴 High |

### Recommended Enhancements

#### 2.1 Rate-Limited Authentication

**Risk**: Brute-force attacks can guess weak passwords or keys without penalty.

**Solution**: Implement exponential backoff and connection blocking:

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct AuthAttempt {
    count: u32,
    last_failure: Instant,
    locked_until: Option<Instant>,
}

pub struct RateLimitedAuth {
    inner: Arc<dyn Authenticator>,
    attempts: Arc<StdMutex<HashMap<String, AuthAttempt>>>,
    max_attempts: u32,           // Per window
    window_duration: Duration,   // Reset window
    lockout_duration: Duration,  // Initial lockout
}

impl RateLimitedAuth {
    pub fn new(
        inner: Arc<dyn Authenticator>,
        max_attempts: u32,        // e.g., 3
        window_duration: Duration, // e.g., 5 minutes
    ) -> Self {
        Self {
            inner,
            attempts: Arc::new(StdMutex::new(HashMap::new())),
            max_attempts,
            window_duration,
            lockout_duration: Duration::from_secs(60),
        }
    }

    fn check_lockout(&self, key: &str) -> Result<()> {
        let mut attempts = self.attempts.lock().map_err(|_| {
            AuthError::VerificationFailed {
                reason: "lockout state poisoned".into(),
            }
        })?;

        let now = Instant::now();
        
        if let Some(attempt) = attempts.get_mut(key) {
            // Check if in lockout window
            if let Some(locked_until) = attempt.locked_until {
                if now < locked_until {
                    return Err(AuthError::RateLimited(locked_until).into());
                } else {
                    // Lockout expired, reset
                    attempt.locked_until = None;
                    attempt.count = 0;
                }
            }

            // Check if window expired
            if now.duration_since(attempt.last_failure) > self.window_duration {
                attempt.count = 0;
            }
        }

        Ok(())
    }

    fn record_failure(&self, key: &str) {
        if let Ok(mut attempts) = self.attempts.lock() {
            let attempt = attempts.entry(key.to_string())
                .or_insert_with(|| AuthAttempt {
                    count: 0,
                    last_failure: Instant::now(),
                    locked_until: None,
                });

            attempt.count += 1;
            attempt.last_failure = Instant::now();

            // Calculate exponential backoff: 60s, 300s, 900s, etc.
            if attempt.count >= self.max_attempts {
                let backoff = self.lockout_duration * 2u32.pow(attempt.count - self.max_attempts);
                attempt.locked_until = Some(Instant::now() + backoff);
                
                tracing::warn!(
                    key = %key,
                    count = attempt.count,
                    "authentication rate limit exceeded, lockout until {:?}",
                    attempt.locked_until
                );
            }
        }
    }

    fn record_success(&self, key: &str) {
        if let Ok(mut attempts) = self.attempts.lock() {
            attempts.remove(key);
        }
    }
}

impl Authenticator for RateLimitedAuth {
    fn supported_methods(&self) -> Vec<AuthMethod> {
        self.inner.supported_methods()
    }

    fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool> {
        self.check_lockout(user)?;
        
        match self.inner.check_public_key(user, key) {
            Ok(true) => {
                self.record_success(user);
                Ok(true)
            }
            Ok(false) => {
                self.record_failure(user);
                Ok(false)
            }
            Err(e) => {
                self.record_failure(user);
                Err(e)
            }
        }
    }

    fn check_password(&self, user: &str, password: &str) -> Result<bool> {
        self.check_lockout(user)?;
        
        match self.inner.check_password(user, password) {
            Ok(true) => {
                self.record_success(user);
                Ok(true)
            }
            Ok(false) => {
                self.record_failure(user);
                Ok(false)
            }
            Err(e) => {
                self.record_failure(user);
                Err(e)
            }
        }
    }
}
```

**Usage in CLI:**

```rust
let base_auth = KeyOnlyAuth::new(state.clone(), security);
let rate_limited = RateLimitedAuth::new(
    Arc::new(base_auth),
    3,                              // Max 3 attempts
    Duration::from_secs(5 * 60),    // Per 5 minutes
);

let options = ServerOptions::new(state)
    .authenticator(rate_limited);
```

**Priority**: 🔴 **High** (prevents brute-force)  
**Effort**: ⏱️ Medium (~150 lines)

#### 2.2 Session Timeouts & Idle Disconnection

**Risk**: Long-lived sessions remain vulnerable if the machine is left unattended.

**Solution**: Add session expiration and idle timeout:

```rust
pub struct SessionTimeout {
    created_at: Instant,
    max_duration: Duration,
    last_activity: Arc<Mutex<Instant>>,
    idle_timeout: Duration,
}

impl SessionTimeout {
    pub fn new(max_duration: Duration, idle_timeout: Duration) -> Self {
        let now = Instant::now();
        Self {
            created_at: now,
            max_duration,
            last_activity: Arc::new(Mutex::new(now)),
            idle_timeout,
        }
    }

    pub fn is_expired(&self) -> bool {
        // Check absolute expiration
        if self.created_at.elapsed() > self.max_duration {
            return true;
        }

        // Check idle timeout
        if let Ok(last_activity) = self.last_activity.lock() {
            if last_activity.elapsed() > self.idle_timeout {
                return true;
            }
        }

        false
    }

    pub fn record_activity(&self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }
}
```

**Priority**: 🟡 **Medium** (operational security)  
**Effort**: ⏱️ Low (~80 lines)

#### 2.3 Multi-Factor Authentication (TOTP)

**Risk**: Single-factor authentication is insufficient for high-value targets.

**Solution**: Integrate Time-Based One-Time Passwords (TOTP):

```rust
use totp_lite::{Sha1, TOTP};

pub struct MfaAuth {
    base_auth: Arc<dyn Authenticator>,
    totp_secrets: Arc<Mutex<HashMap<String, String>>>,  // user -> base32 secret
}

impl MfaAuth {
    pub fn setup_totp(&self, user: &str) -> Result<(String, String)> {
        // Generate random secret
        let secret = generate_random_base32(32)?;
        
        // Generate QR code URI
        let uri = format!(
            "otpauth://totp/Irosh:{}?secret={}&issuer=Irosh",
            user, secret
        );
        
        self.totp_secrets.lock()?
            .insert(user.to_string(), secret.clone());
        
        Ok((secret, uri))
    }

    pub fn verify_totp(&self, user: &str, token: &str) -> Result<bool> {
        let secrets = self.totp_secrets.lock()?;
        
        let secret = secrets.get(user)
            .ok_or(AuthError::MissingCredential("TOTP secret not configured".into()))?;
        
        // Check current and ±1 time window (30-second slots)
        let totp = TOTP::new(Sha1, 6, 30, secret.as_bytes());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        
        for offset in [-1, 0, 1] {
            let expected = totp.generate(now as i64 + offset * 30);
            if expected == token {
                return Ok(true);
            }
        }
        
        Ok(false)
    }
}

impl Authenticator for MfaAuth {
    fn check_password(&self, user: &str, password: &str) -> Result<bool> {
        // First, verify password with base auth
        if !self.base_auth.check_password(user, password)? {
            return Ok(false);
        }
        
        // TODO: Prompt for TOTP token
        // In practice, this would be:
        // 1. Client sends password
        // 2. Server responds with "TOTP required"
        // 3. Client sends TOTP token
        // 4. Server verifies TOTP
        
        Ok(true)
    }

    fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool> {
        // Public key auth + TOTP (if configured for user)
        self.base_auth.check_public_key(user, key)
    }

    fn supported_methods(&self) -> Vec<AuthMethod> {
        self.base_auth.supported_methods()
    }
}
```

**Priority**: 🟡 **Medium** (high-value deployments)  
**Effort**: ⏱️ Medium (~120 lines + new SSH protocol negotiation)

---

## 3. Trust Store & Key Management

### Current Implementation ✅

```rust
// src/storage/trust.rs (pattern)
// Client-side: known_hosts-like store
// Server-side: authorized_keys list
```

### Recommended Enhancements

#### 3.1 Trust Store Integrity Verification

**Risk**: Tampering with trust store files could silently inject malicious keys.

**Solution**: Add HMAC verification to detect tampering:

```rust
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustRecord {
    pub fingerprint: String,
    pub host_key: PublicKey,
    pub created_at: SystemTime,
    pub hmac: Vec<u8>,  // HMAC over the record
}

pub fn save_trusted_host(
    state: &StateConfig,
    master_secret: &[u8; 32],
    host_key: &PublicKey,
) -> Result<()> {
    let path = get_trust_store_path(state);
    
    let record = TrustRecord {
        fingerprint: compute_fingerprint(host_key)?,
        host_key: host_key.clone(),
        created_at: SystemTime::now(),
        hmac: Vec::new(),  // Placeholder
    };
    
    // Serialize without HMAC
    let json = serde_json::to_vec(&record)?;
    
    // Compute HMAC-SHA256(master_secret, serialized_record)
    let mut mac = HmacSha256::new_from_slice(master_secret)
        .map_err(|_| StorageError::TrustStoreHmac)?;
    mac.update(&json);
    
    // Add HMAC to record
    let mut record_with_hmac = record;
    record_with_hmac.hmac = mac.finalize().into_bytes().to_vec();
    
    // Write to disk
    let final_json = serde_json::to_vec(&record_with_hmac)?;
    fs::write(&path, final_json)?;
    
    tracing::info!(fingerprint = %record.fingerprint, "trusted host key saved");
    Ok(())
}

pub fn verify_trusted_host(
    state: &StateConfig,
    master_secret: &[u8; 32],
    host_key: &PublicKey,
) -> Result<bool> {
    let path = get_trust_store_path(state);
    
    let json = fs::read(&path)?;
    let mut record: TrustRecord = serde_json::from_slice(&json)?;
    
    let stored_hmac = record.hmac.clone();
    record.hmac.clear();  // Verify over unsigned data
    
    let unsigned_json = serde_json::to_vec(&record)?;
    
    // Verify HMAC
    let mut mac = HmacSha256::new_from_slice(master_secret)
        .map_err(|_| StorageError::TrustStoreHmac)?;
    mac.update(&unsigned_json);
    
    mac.verify_slice(&stored_hmac)
        .map_err(|_| StorageError::TrustStoreHmacMismatch)?;
    
    // Compare key fingerprints
    let expected_fingerprint = compute_fingerprint(host_key)?;
    if record.fingerprint != expected_fingerprint {
        tracing::warn!(
            expected = %record.fingerprint,
            actual = %expected_fingerprint,
            "host key fingerprint mismatch"
        );
        return Ok(false);
    }
    
    Ok(true)
}
```

**Priority**: 🟡 **Medium** (filesystem tampering detection)  
**Effort**: ⏱️ Low (~100 lines)

#### 3.2 Key Rotation Policy

**Risk**: Long-lived keys are exposed to theft or compromise over time.

**Solution**: Implement automatic key rotation:

```rust
pub struct KeyRotationPolicy {
    rotation_interval: Duration,
    max_key_age: Duration,
    last_rotated: Instant,
}

impl KeyRotationPolicy {
    pub fn should_rotate(&self) -> bool {
        self.last_rotated.elapsed() > self.rotation_interval
    }

    pub fn enforce_rotation(&self) -> Result<()> {
        if self.last_rotated.elapsed() > self.max_key_age {
            tracing::warn!(
                age_days = self.last_rotated.elapsed().as_secs() / 86400,
                "key has exceeded maximum age, forcing rotation"
            );
            return Err(IroshError::KeyRotationRequired);
        }
        Ok(())
    }
}

pub async fn rotate_identity(state: &StateConfig) -> Result<NodeIdentity> {
    tracing::info!("rotating node identity");
    
    // Backup old identity
    let old_identity = load_or_generate_identity(state).await?;
    backup_identity(state, &old_identity).await?;
    
    // Generate new identity
    delete_secret_key(state)?;
    let new_identity = load_or_generate_identity(state).await?;
    
    // Log rotation event
    log_identity_rotation(state, &old_identity, &new_identity).await?;
    
    tracing::info!(
        old_id = %old_identity.secret_key.public(),
        new_id = %new_identity.secret_key.public(),
        "identity rotated successfully"
    );
    
    Ok(new_identity)
}
```

**Configuration:**

```toml
[security.key_rotation]
# Rotate every 90 days
rotation_interval_days = 90
# Warn if key is older than 6 months
max_age_days = 180
```

**Priority**: 🟡 **Medium** (long-term key hygiene)  
**Effort**: ⏱️ Medium (~150 lines + config)

#### 3.3 Encrypted Backups

**Risk**: Backups without encryption can be read by unauthorized parties.

**Solution**: Use `age` crate for strong backup encryption:

```rust
use age::{Decryptor, Encryptor, x25519};

pub async fn backup_identity_encrypted(
    state: &StateConfig,
    passphrase: &str,
) -> Result<PathBuf> {
    let identity = load_or_generate_identity(state).await?;
    let backup_dir = state.root().join("backups");
    fs::create_dir_all(&backup_dir)?;
    
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let backup_path = backup_dir.join(format!("identity_{}.age", timestamp));
    
    // Derive encryption key from passphrase using Argon2
    let mut key_material = [0u8; 32];
    argon2::hash_password_into(
        passphrase.as_bytes(),
        "irosh-backup".as_bytes(),
        &mut key_material,
    )?;
    
    let secret = x25519::Identity::from_bytes(&key_material);
    let encryptor = Encryptor::with_identity(&secret);
    
    let identity_json = serde_json::to_vec(&identity)?;
    let mut file = tokio::fs::File::create(&backup_path).await?;
    
    let mut writer = encryptor.wrap_output(tokio::io::BufWriter::new(&mut file))?;
    writer.write_all(&identity_json).await?;
    writer.finish().await?;
    
    tracing::info!(backup_path = ?backup_path, "encrypted identity backup created");
    Ok(backup_path)
}

pub async fn restore_identity_from_backup(
    backup_path: &Path,
    passphrase: &str,
) -> Result<NodeIdentity> {
    let mut key_material = [0u8; 32];
    argon2::hash_password_into(
        passphrase.as_bytes(),
        "irosh-backup".as_bytes(),
        &mut key_material,
    )?;
    
    let secret = x25519::Identity::from_bytes(&key_material);
    
    let file = tokio::fs::File::open(backup_path).await?;
    let decryptor = match Decryptor::new(tokio::io::BufReader::new(file)).await? {
        Decryptor::X25519(d) => d,
        _ => return Err(IroshError::InvalidBackupFormat.into()),
    };
    
    let mut identity_json = Vec::new();
    let mut reader = decryptor.decrypt(&secret, age::StreamFormat::Binary)?;
    reader.read_to_end(&mut identity_json).await?;
    
    let identity: NodeIdentity = serde_json::from_slice(&identity_json)?;
    Ok(identity)
}
```

**Priority**: 🟡 **Medium** (disaster recovery)  
**Effort**: ⏱️ Medium (~120 lines)

---

## 4. Transport Security

### Current Implementation ✅

```rust
// Iroh's built-in QUIC encryption + Ed25519 verification
```

**Existing Strengths:**
- ✅ QUIC protocol (modern, RFC 9000)
- ✅ Ed25519 peer authentication
- ✅ Forward secrecy within connections

### Recommended Enhancements

#### 4.1 Certificate Pinning

**Risk**: Compromised relay nodes could perform MITM attacks.

**Solution**: Optionally pin known peer certificates:

```rust
pub struct TransportSecurityPolicy {
    /// Whitelist of allowed peer Node IDs
    pinned_peers: HashSet<NodeId>,
    /// Require pinning (deny unknown peers)
    require_pinning: bool,
    /// Allowed ALPN protocols
    allowed_alpns: Vec<Vec<u8>>,
}

impl TransportSecurityPolicy {
    pub fn is_peer_allowed(&self, node_id: &NodeId) -> bool {
        if !self.require_pinning {
            return true;
        }
        self.pinned_peers.contains(node_id)
    }

    pub fn add_pinned_peer(&mut self, node_id: NodeId) {
        self.pinned_peers.insert(node_id);
    }

    pub fn remove_pinned_peer(&mut self, node_id: &NodeId) -> bool {
        self.pinned_peers.remove(node_id)
    }
}
```

**Usage in CLI:**

```bash
# Pin a specific server
irosh peer pin <node-id>

# Enforce pinning (deny unknown peers)
irosh client connect --require-pinning <alias>
```

**Priority**: 🟡 **Medium** (defense against relay compromise)  
**Effort**: ⏱️ Low (~80 lines)

#### 4.2 Connection Isolation & Channel Binding

**Risk**: Session hijacking if connection state is not properly isolated.

**Solution**: Add cryptographic channel binding:

```rust
use sha2::{Sha256, Digest};

#[derive(Debug, Clone)]
pub struct ConnectionContext {
    session_id: Uuid,
    client_nonce: [u8; 32],
    server_nonce: [u8; 32],
    binding_hash: [u8; 32],
}

impl ConnectionContext {
    pub fn new(client_nonce: [u8; 32], server_nonce: [u8; 32]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(&client_nonce);
        hasher.update(&server_nonce);
        let binding_hash = hasher.finalize().into();
        
        Self {
            session_id: Uuid::new_v4(),
            client_nonce,
            server_nonce,
            binding_hash,
        }
    }

    pub fn verify(&self, challenge: &[u8; 32]) -> bool {
        // Verify connection hasn't been hijacked
        let mut hasher = Sha256::new();
        hasher.update(self.binding_hash);
        hasher.update(challenge);
        
        let response = hasher.finalize();
        // Compare with received response...
        true  // Placeholder
    }
}
```

**Priority**: 🟡 **Medium** (session hijacking prevention)  
**Effort**: ⏱️ Medium (~100 lines)

#### 4.3 Replay Attack Prevention

**Risk**: Attackers could replay captured messages to replay past actions.

**Solution**: Track and reject replayed messages:

```rust
use std::collections::HashSet;

pub struct ReplayProtection {
    nonce_cache: Arc<Mutex<HashSet<[u8; 32]>>>,
    nonce_ttl: Duration,
    cache_cleanup_interval: Duration,
}

impl ReplayProtection {
    pub fn new(nonce_ttl: Duration) -> Self {
        Self {
            nonce_cache: Arc::new(Mutex::new(HashSet::new())),
            nonce_ttl,
            cache_cleanup_interval: Duration::from_secs(300),  // 5 min
        }
    }

    pub async fn check_nonce(&self, nonce: &[u8; 32]) -> Result<()> {
        let mut cache = self.nonce_cache.lock().await;
        
        if cache.contains(nonce) {
            return Err(IroshError::ReplayAttackDetected);
        }
        
        cache.insert(*nonce);
        Ok(())
    }

    pub async fn cleanup_expired_nonces(&self) {
        loop {
            tokio::time::sleep(self.cache_cleanup_interval).await;
            
            // In production, use a timestamp map and clean based on TTL
            if let Ok(mut cache) = self.nonce_cache.lock() {
                cache.clear();  // Simplified; use actual TTL tracking
            }
        }
    }
}
```

**Priority**: 🟡 **Medium** (replay attack mitigation)  
**Effort**: ⏱️ Low (~80 lines)

---

## 5. Wormhole (Pairing) Security

### Current Design ✅ (from WORMHOLE_DESIGN.md)

```
Ephemeral 3-word codes + HMAC-keyed topics + Rate limiting + Interactive confirmation
```

**Existing Strengths:**
- ✅ 3-word codes (high entropy, ~90 bits)
- ✅ HMAC-SHA256 keyed topics (prevents eavesdropping)
- ✅ Rate limiting (3 failures = auto-burn)
- ✅ Interactive confirmation prompt

### Recommended Enhancements

#### 5.1 Enhanced Keyed HMAC (Already Planned)

**Implementation:**

```rust
fn derive_wormhole_topic(code: &str) -> Result<[u8; 32]> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    
    type HmacSha256 = Hmac<Sha256>;
    
    let mut mac = HmacSha256::new_from_slice(b"irosh-wormhole-v1")?;
    mac.update(code.as_bytes());
    
    let result = mac.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(result.into_bytes().as_ref());
    
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wormhole_topic_is_deterministic() {
        let topic1 = derive_wormhole_topic("crystal-piano-7").unwrap();
        let topic2 = derive_wormhole_topic("crystal-piano-7").unwrap();
        assert_eq!(topic1, topic2);
    }

    #[test]
    fn different_codes_produce_different_topics() {
        let topic1 = derive_wormhole_topic("crystal-piano-7").unwrap();
        let topic2 = derive_wormhole_topic("crystal-piano-8").unwrap();
        assert_ne!(topic1, topic2);
    }
}
```

**Priority**: 🔴 **High** (core security)  
**Effort**: ⏱️ Trivial (~40 lines)

#### 5.2 Ephemeral Session Binding

**Risk**: Wormhole codes could be reused if captured.

**Solution**: Invalidate codes after successful pairing:

```rust
pub struct WormholeSession {
    code: String,
    expires_at: Instant,
    max_uses: u32,
    used_count: Arc<AtomicU32>,
    session_token: Uuid,
    pairing_complete: Arc<AtomicBool>,
}

impl WormholeSession {
    pub fn new(code: String, max_duration: Duration, max_uses: u32) -> Self {
        Self {
            code,
            expires_at: Instant::now() + max_duration,
            max_uses,
            used_count: Arc::new(AtomicU32::new(0)),
            session_token: Uuid::new_v4(),
            pairing_complete: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at
            && self.used_count.load(Ordering::SeqCst) < self.max_uses
            && !self.pairing_complete.load(Ordering::SeqCst)
    }

    pub fn record_attempt(&self) -> Result<()> {
        let count = self.used_count.fetch_add(1, Ordering::SeqCst);
        
        if count >= self.max_uses {
            self.pairing_complete.store(true, Ordering::SeqCst);
            return Err(IroshError::WormholeExhausted.into());
        }
        
        Ok(())
    }

    pub fn complete_pairing(&self) {
        self.pairing_complete.store(true, Ordering::SeqCst);
        tracing::info!(
            code = %self.code,
            session_id = %self.session_token,
            "wormhole pairing completed, code invalidated"
        );
    }
}
```

**Priority**: 🔴 **High** (prevents code reuse)  
**Effort**: ⏱️ Low (~60 lines)

#### 5.3 Confirmation Dialog Hardening

**Risk**: Users might confirm without verifying peer identity.

**Solution**: Always display full Node ID fingerprints in confirmation:

```rust
pub trait ConfirmationCallback: Send + Sync {
    async fn prompt_wormhole_pairing(
        &self,
        code: &str,
        peer_node_id: &NodeId,
        peer_fingerprint: &str,
    ) -> Result<bool>;
}

pub struct InteractiveConfirmation;

impl ConfirmationCallback for InteractiveConfirmation {
    async fn prompt_wormhole_pairing(
        &self,
        code: &str,
        peer_node_id: &NodeId,
        peer_fingerprint: &str,
    ) -> Result<bool> {
        eprintln!();
        eprintln!("╔════════════════════════════════════════════════════════════╗");
        eprintln!("║                  WORMHOLE PAIRING REQUEST                  ║");
        eprintln!("╚════════════════════════════════════════════════════════════╝");
        eprintln!();
        eprintln!("  Code: {}", code);
        eprintln!();
        eprintln!("  Peer Node ID (first 16 chars):");
        eprintln!("    {}", &peer_node_id.to_string()[..16]);
        eprintln!();
        eprintln!("  Peer Fingerprint (SHA-256):");
        eprintln!("    {}", peer_fingerprint);
        eprintln!();
        eprintln!("  ⚠️  IMPORTANT:");
        eprintln!("  - Verify the fingerprint with the remote operator");
        eprintln!("  - If in doubt, reject the connection");
        eprintln!();
        
        print!("Accept pairing? [y/N] ");
        std::io::stdout().flush()?;
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        
        Ok(input.trim().to_lowercase() == "y")
    }
}
```

**Priority**: 🔴 **High** (user verification)  
**Effort**: ⏱️ Low (~60 lines)

---

## 6. Session & Connection Security

### Current Implementation ✅

```rust
// Iroh multiplexing: 1 SSH stream + metadata + transfer streams
```

### Recommended Enhancements

#### 6.1 Session Binding & Channel Binding

**Risk**: Session hijacking after initial authentication.

**Solution**: Implement TLS-like channel binding:

```rust
pub struct BoundSession {
    session_id: Uuid,
    binding_data: SessionBinding,
    created_at: Instant,
    last_activity: Arc<Mutex<Instant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    /// Hash of client nonce + server nonce + connection parameters
    binding_hash: [u8; 32],
    /// Per-message counter to prevent replays
    message_counter: Arc<AtomicU64>,
}

impl SessionBinding {
    pub fn new(client_nonce: &[u8; 32], server_nonce: &[u8; 32]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(client_nonce);
        hasher.update(server_nonce);
        hasher.update(b"irosh-session-binding-v1");
        
        Self {
            binding_hash: hasher.finalize().into(),
            message_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn get_next_message_id(&self) -> u64 {
        self.message_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn verify_binding(&self, challenge: &[u8; 32]) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(self.binding_hash);
        hasher.update(challenge);
        
        let response: [u8; 32] = hasher.finalize().into();
        // Compare with received response...
        Ok(())
    }
}
```

**Priority**: 🟡 **Medium** (session hijacking prevention)  
**Effort**: ⏱️ Medium (~100 lines)

#### 6.2 Forward Secrecy with Key Derivation

**Risk**: If master key is compromised, past sessions can be decrypted.

**Solution**: Derive unique keys per session with HKDF:

```rust
use hkdf::Hkdf;
use sha2::Sha256;

pub struct SessionKeyDerivation {
    master_secret: [u8; 32],
    salt: [u8; 16],
}

impl SessionKeyDerivation {
    pub fn derive_session_keys(&self, session_id: Uuid) -> Result<SessionKeys> {
        let hkdf = Hkdf::<Sha256>::new(Some(&self.salt), &self.master_secret);
        
        // PRK (Pseudo-Random Key)
        let prk = hkdf.expand(b"irosh-session-keys", 32)?;
        
        // Derive encryption key for this session
        let mut enc_key = [0u8; 32];
        let hkdf2 = Hkdf::<Sha256>::new(Some(&prk), &[]);
        hkdf2.expand(
            format!("irosh-enc-{}", session_id).as_bytes(),
            &mut enc_key,
        )?;
        
        Ok(SessionKeys {
            encryption_key: enc_key,
            session_id,
        })
    }

    /// Rotate keys every N messages or T duration
    pub async fn periodic_key_rotation(
        &mut self,
        messages_threshold: u64,
        time_threshold: Duration,
    ) -> Result<()> {
        // Regenerate master secret from current + additional entropy
        let new_entropy = rand::random::<[u8; 16]>();
        let mut hasher = Sha256::new();
        hasher.update(&self.master_secret);
        hasher.update(&new_entropy);
        
        self.master_secret = hasher.finalize().into();
        
        tracing::info!("session keys rotated");
        Ok(())
    }
}
```

**Priority**: 🟡 **Medium** (long-term forward secrecy)  
**Effort**: ⏱️ Medium (~120 lines)

---

## 7. File Transfer Security

### Current Implementation ✅

```rust
// Isolated side-streams, chunked transfers, prevents PTY corruption
```

### Recommended Enhancements

#### 7.1 Transfer Integrity Verification

**Risk**: File corruption (intentional or accidental) during transfer.

**Solution**: Add SHA-256 integrity checking:

```rust
pub struct SecureTransfer {
    path: PathBuf,
    total_size: u64,
    chunks: Vec<TransferChunk>,
    file_hash: Option<[u8; 32]>,  // Full file SHA-256
}

#[derive(Debug, Clone)]
pub struct TransferChunk {
    sequence: u32,
    offset: u64,
    size: u64,
    data: Vec<u8>,
    hash: [u8; 32],  // SHA-256 of this chunk
}

impl SecureTransfer {
    pub async fn send_file_with_verification(
        &self,
        writer: &mut AsyncWrite,
    ) -> Result<()> {
        let mut file = tokio::fs::File::open(&self.path).await?;
        let mut chunk_num = 0;
        let mut file_hasher = Sha256::new();
        
        loop {
            let mut buffer = vec![0u8; 65536];  // 64KB chunks
            let n = file.read(&mut buffer).await?;
            
            if n == 0 {
                break;  // EOF
            }
            
            buffer.truncate(n);
            
            // Hash this chunk
            let mut chunk_hasher = Sha256::new();
            chunk_hasher.update(&buffer);
            let chunk_hash: [u8; 32] = chunk_hasher.finalize().into();
            
            // Hash for overall file verification
            file_hasher.update(&buffer);
            
            // Send chunk with hash
            let frame = TransferFrame {
                seq: chunk_num,
                chunk_hash,
                data: buffer,
            };
            
            writer.write_all(&serde_json::to_vec(&frame)?).await?;
            chunk_num += 1;
        }
        
        // Send final checksum
        let final_hash: [u8; 32] = file_hasher.finalize().into();
        let checksum_frame = ChecksumFrame {
            total_chunks: chunk_num,
            file_hash: final_hash,
        };
        
        writer.write_all(&serde_json::to_vec(&checksum_frame)?).await?;
        Ok(())
    }

    pub async fn receive_file_with_verification(
        &mut self,
        reader: &mut AsyncRead,
    ) -> Result<()> {
        let mut file = tokio::fs::File::create(&self.path).await?;
        let mut file_hasher = Sha256::new();
        let mut expected_seq = 0;
        
        loop {
            let mut buf = vec![0u8; 65536 + 128];  // Extra for frame overhead
            let n = reader.read(&mut buf).await?;
            
            if n == 0 {
                break;
            }
            
            let frame: TransferFrame = serde_json::from_slice(&buf[..n])?;
            
            // Verify sequence number (detect reordering/drops)
            if frame.seq != expected_seq {
                return Err(IroshError::TransferSequenceError {
                    expected: expected_seq,
                    got: frame.seq,
                }.into());
            }
            
            // Verify chunk hash
            let mut chunk_hasher = Sha256::new();
            chunk_hasher.update(&frame.data);
            let computed_hash: [u8; 32] = chunk_hasher.finalize().into();
            
            if computed_hash != frame.chunk_hash {
                return Err(IroshError::TransferChecksumMismatch.into());
            }
            
            // Write and hash for final verification
            file.write_all(&frame.data).await?;
            file_hasher.update(&frame.data);
            expected_seq += 1;
        }
        
        // Verify final checksum
        let checksum_frame: ChecksumFrame = serde_json::from_slice(&buf).await?;
        let computed_file_hash: [u8; 32] = file_hasher.finalize().into();
        
        if computed_file_hash != checksum_frame.file_hash {
            return Err(IroshError::FileChecksumMismatch {
                expected: hex::encode(&checksum_frame.file_hash),
                got: hex::encode(&computed_file_hash),
            }.into());
        }
        
        self.file_hash = Some(computed_file_hash);
        Ok(())
    }
}
```

**Priority**: 🟡 **Medium** (data integrity)  
**Effort**: ⏱️ Medium (~180 lines)

#### 7.2 Path Traversal Prevention

**Risk**: Attackers could write files outside intended directory via `../` paths.

**Solution**: Canonicalize and validate all transfer paths:

```rust
pub fn validate_transfer_path(
    base_dir: &Path,
    user_path: &str,
) -> Result<PathBuf> {
    // Reject absolute paths
    if user_path.starts_with('/') {
        return Err(IroshError::InvalidPath {
            reason: "absolute paths not allowed".into(),
        }.into());
    }
    
    // Reject .. traversal
    if user_path.contains("..") {
        return Err(IroshError::InvalidPath {
            reason: "path traversal (..) not allowed".into(),
        }.into());
    }
    
    let full_path = base_dir.join(user_path);
    
    // Canonicalize and verify it's still under base_dir
    let canonical = full_path.canonicalize()
        .map_err(|_| IroshError::InvalidPath {
            reason: "cannot canonicalize path".into(),
        })?;
    
    let canonical_base = base_dir.canonicalize()?;
    
    if !canonical.starts_with(&canonical_base) {
        return Err(IroshError::InvalidPath {
            reason: "path escapes base directory".into(),
        }.into());
    }
    
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_absolute_paths() {
        assert!(validate_transfer_path(Path::new("/home"), "/etc/passwd").is_err());
    }

    #[test]
    fn rejects_traversal() {
        assert!(validate_transfer_path(Path::new("/home"), "../../etc/passwd").is_err());
    }

    #[test]
    fn accepts_valid_relative_paths() {
        assert!(validate_transfer_path(Path::new("/home/user"), "documents/file.txt").is_ok());
    }
}
```

**Priority**: 🔴 **High** (prevents directory escape)  
**Effort**: ⏱️ Low (~80 lines)

---

## 8. Audit & Logging

### Current Implementation ❌

No comprehensive audit logging

### Recommended Implementation

#### 8.1 Structured Audit Logging

**Risk**: Security events not logged for forensics and compliance.

**Solution**: Add structured, signed audit logs:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// Monotonic sequence number to detect missing entries
    pub sequence: u64,
    /// ISO8601 timestamp
    pub timestamp: DateTime<Utc>,
    /// Event type
    pub event_type: AuditEventType,
    /// Structured event data
    pub data: serde_json::Value,
    /// HMAC signature for integrity
    pub signature: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    /// User authentication attempt
    AuthAttempt {
        user: String,
        method: String,
        success: bool,
    },
    /// Session created
    SessionStart {
        session_id: String,
        user: String,
        client_node_id: String,
    },
    /// Session terminated
    SessionEnd {
        session_id: String,
        duration_secs: u64,
        reason: String,
    },
    /// File transferred
    FileTransfer {
        direction: String,  // "upload" or "download"
        path: String,
        size_bytes: u64,
        hash: String,
    },
    /// Authorization event (key added/removed)
    AuthorizationChange {
        user: String,
        action: String,
        resource: String,
    },
    /// Security policy violation
    SecurityViolation {
        reason: String,
        details: String,
    },
}

pub struct AuditLogger {
    path: PathBuf,
    sequence: Arc<AtomicU64>,
    master_secret: Option<[u8; 32]>,  // For HMAC signing
}

impl AuditLogger {
    pub fn new(path: PathBuf, master_secret: Option<[u8; 32]>) -> Result<Self> {
        // Ensure log directory exists with restricted permissions
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            set_strict_permissions(parent)?;
        }

        Ok(Self {
            path,
            sequence: Arc::new(AtomicU64::new(1)),
            master_secret,
        })
    }

    pub fn log(&self, event_type: AuditEventType) -> Result<()> {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst);
        
        let mut entry = AuditLogEntry {
            sequence: seq,
            timestamp: Utc::now(),
            event_type,
            data: serde_json::json!({}),
            signature: None,
        };

        // Sign entry if master secret available
        if let Some(secret) = self.master_secret {
            let json = serde_json::to_vec(&entry)?;
            let mut mac = HmacSha256::new_from_slice(&secret)?;
            mac.update(&json);
            entry.signature = Some(mac.finalize().into_bytes().to_vec());
        }

        // Append to log file
        let line = format!("{}\n", serde_json::to_string(&entry)?);
        
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        
        file.write_all(line.as_bytes())?;

        tracing::info!(seq, event = ?entry.event_type, "audit log entry");
        Ok(())
    }

    /// Verify log integrity by checking signatures and sequence
    pub fn verify_log_integrity(&self) -> Result<()> {
        let Some(secret) = self.master_secret else {
            tracing::warn!("no master secret configured, skipping log verification");
            return Ok(());
        };

        let file = fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        
        let mut expected_seq = 1u64;
        
        for line in reader.lines() {
            let line = line?;
            let mut entry: AuditLogEntry = serde_json::from_str(&line)?;
            
            // Check sequence
            if entry.sequence != expected_seq {
                return Err(IroshError::LogIntegrityViolation {
                    reason: format!("sequence gap: expected {}, got {}", expected_seq, entry.sequence),
                }.into());
            }
            
            // Verify signature
            if let Some(sig) = entry.signature.take() {
                let json = serde_json::to_vec(&entry)?;
                let mut mac = HmacSha256::new_from_slice(&secret)?;
                mac.update(&json);
                
                mac.verify_slice(&sig)
                    .map_err(|_| IroshError::LogIntegrityViolation {
                        reason: format!("signature mismatch at seq {}", entry.sequence),
                    })?;
            }
            
            expected_seq += 1;
        }
        
        tracing::info!(total_entries = expected_seq - 1, "log integrity verified");
        Ok(())
    }
}
```

**Priority**: 🟡 **Medium** (forensics and compliance)  
**Effort**: ⏱️ Medium (~220 lines)

#### 8.2 Log Rotation & Archival

**Risk**: Unbounded log growth; old logs deleted without preservation.

**Solution**: Implement rotation with encryption and archival:

```rust
pub struct AuditLogRotation {
    logger: AuditLogger,
    max_log_size: u64,      // e.g., 100MB
    rotation_interval: Duration,
    archive_dir: PathBuf,
}

impl AuditLogRotation {
    pub async fn rotate_if_needed(&self) -> Result<()> {
        let metadata = fs::metadata(&self.logger.path)?;
        
        if metadata.len() > self.max_log_size {
            self.rotate_log().await?;
        }
        
        Ok(())
    }

    async fn rotate_log(&self) -> Result<()> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let archive_name = format!("audit_log_{}.gz.age", timestamp);
        let archive_path = self.archive_dir.join(archive_name);
        
        // Compress current log
        let log_content = fs::read(&self.logger.path)?;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&log_content)?;
        let compressed = encoder.finish()?;
        
        // Encrypt with age
        let encrypted = age::encrypt(compressed)?;
        
        // Save encrypted archive
        fs::write(&archive_path, encrypted)?;
        
        // Clear current log
        fs::write(&self.logger.path, "")?;
        
        tracing::info!(archive = ?archive_path, "audit log rotated and archived");
        Ok(())
    }
}
```

**Priority**: 🟡 **Medium** (long-term log retention)  
**Effort**: ⏱️ Medium (~100 lines)

---

## 9. Configuration & Security Policies

### Current Implementation ✅

```rust
// src/config.rs: SecurityConfig with HostKeyPolicy
```

### Recommended Enhancement: Unified Security Policy

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    // Authentication
    pub require_mfa: bool,
    pub max_failed_auth_attempts: u32,
    pub auth_attempt_window_secs: u64,
    pub lockout_duration_secs: u64,

    // Sessions
    pub max_session_duration_secs: u64,
    pub idle_timeout_secs: u64,
    pub require_host_key_pinning: bool,

    // Keys
    pub key_rotation_days: u32,
    pub max_key_age_days: u32,

    // Transport
    pub enforce_tls_1_2_plus: bool,
    pub allowed_ciphers: Vec<String>,

    // Files
    pub max_transfer_size_bytes: u64,
    pub enforce_path_validation: bool,

    // Audit
    pub enable_audit_logging: bool,
    pub audit_log_retention_days: u32,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            // Paranoid defaults
            require_mfa: false,  // Can be enabled per deployment
            max_failed_auth_attempts: 3,
            auth_attempt_window_secs: 5 * 60,
            lockout_duration_secs: 60,

            max_session_duration_secs: 1 * 3600,  // 1 hour
            idle_timeout_secs: 15 * 60,           // 15 minutes
            require_host_key_pinning: false,      // Can be enforced

            key_rotation_days: 90,
            max_key_age_days: 180,

            enforce_tls_1_2_plus: true,
            allowed_ciphers: vec![
                "ChaCha20Poly1305".to_string(),
                "AES256GCM".to_string(),
            ],

            max_transfer_size_bytes: 10 * 1024 * 1024 * 1024,  // 10GB
            enforce_path_validation: true,

            enable_audit_logging: true,
            audit_log_retention_days: 90,
        }
    }
}
```

**Configuration File (TOML):**

```toml
[security]
require_mfa = false
max_failed_auth_attempts = 3
auth_attempt_window_secs = 300  # 5 minutes

[security.session]
max_duration_secs = 3600        # 1 hour
idle_timeout_secs = 900         # 15 minutes

[security.keys]
rotation_interval_days = 90
max_age_days = 180

[security.audit]
enable_logging = true
retention_days = 90
```

**Priority**: 🟡 **Medium** (operational configuration)  
**Effort**: ⏱️ Medium (~150 lines)

---

## 10. Implementation Checklist

### Phase 1: Cryptographic Hardening 🔴 HIGH PRIORITY

| Item | Description | Status | Effort | Timeline |
|------|-------------|--------|--------|----------|
| Secret Zeroization | Use `zeroize` crate | ❌ TODO | ⏱️ Low | Week 1 |
| File Permissions | 0o600 on secret keys | ❌ TODO | ⏱️ Trivial | Week 1 |
| Trust Store HMAC | Integrity verification | ❌ TODO | ⏱️ Low | Week 1 |
| Rate Limiting | Auth attempt throttling | ❌ TODO | ⏱️ Medium | Week 2-3 |
| **Subtotal** | | | **~60 hours** | |

### Phase 2: Transport & Session Security 🟡 MEDIUM PRIORITY

| Item | Description | Status | Effort | Timeline |
|------|-------------|--------|--------|----------|
| Session Binding | Channel binding tokens | ❌ TODO | ⏱️ Medium | Week 3 |
| Key Rotation | 90-day rotation policy | ❌ TODO | ⏱️ Medium | Week 3 |
| Replay Protection | Nonce-based defense | ❌ TODO | ⏱️ Low | Week 4 |
| Wormhole Hardening | HMAC + expiration | ❌ TODO | ⏱️ Trivial | Week 1 |
| **Subtotal** | | | **~80 hours** | |

### Phase 3: Audit & Operations 🟡 MEDIUM PRIORITY

| Item | Description | Status | Effort | Timeline |
|------|-------------|--------|--------|----------|
| Audit Logging | Structured, signed logs | ❌ TODO | ⏱️ Medium | Week 4-5 |
| Log Rotation | Encryption + archival | ❌ TODO | ⏱️ Medium | Week 5 |
| Security Policy | Unified configuration | ❌ TODO | ⏱️ Medium | Week 5-6 |
| Transfer Hashing | SHA-256 integrity checks | ❌ TODO | ⏱️ Medium | Week 6 |
| **Subtotal** | | | **~100 hours** | |

### Phase 4: Advanced Features 🟡 MEDIUM PRIORITY

| Item | Description | Status | Effort | Timeline |
|------|-------------|--------|--------|----------|
| MFA Support | TOTP integration | ❌ TODO | ⏱️ Medium | Week 7 |
| HSM Support | PKCS#11 backend | ❌ TODO | ⏱️ High | Week 8-9 |
| Encrypted Backups | age-based backup | ❌ TODO | ⏱️ Medium | Week 7 |
| Certificate Pinning | Per-peer whitelist | ❌ TODO | ⏱️ Low | Week 6 |
| **Subtotal** | | | **~120 hours** | |

### Total Effort Estimate
- **Phases 1-3 (Essential)**: ~240 hours (~6 weeks for 1 FTE)
- **Phase 4 (Advanced)**: ~120 hours (~3 weeks)
- **Testing & Integration**: ~80 hours (~2 weeks)
- **Total**: ~440 hours (~11 weeks)

---

## 11. Testing & Verification Strategy

### Security-Focused Test Coverage

```rust
// tests/security/
mod authentication_tests {
    #[tokio::test]
    async fn test_rate_limiting_blocks_brute_force() {
        // Attempt 10 failed logins, verify lockout after 3
    }

    #[tokio::test]
    async fn test_mfa_required_for_pairing() {
        // Attempt pairing without TOTP, verify rejection
    }
}

mod transport_tests {
    #[tokio::test]
    async fn test_replay_attack_detection() {
        // Send duplicate nonce, verify rejection
    }

    #[tokio::test]
    async fn test_session_binding_prevents_hijacking() {
        // Hijack session without correct binding, verify rejection
    }
}

mod file_transfer_tests {
    #[tokio::test]
    async fn test_checksum_verification() {
        // Transfer corrupted file, verify detection
    }

    #[tokio::test]
    async fn test_path_traversal_prevention() {
        // Attempt `../../../etc/passwd`, verify rejection
    }
}

mod audit_tests {
    #[test]
    fn test_log_integrity_verification() {
        // Create log with forged entry, verify detection
    }

    #[test]
    fn test_sequence_gap_detection() {
        // Skip log entries, verify detection
    }
}
```

### Fuzzing

```bash
cargo install cargo-fuzz

# tests/fuzz/fuzz_targets/
cargo +nightly fuzz run fuzz_wormhole_code        # Fuzz code parsing
cargo +nightly fuzz run fuzz_transfer_path        # Fuzz path validation
cargo +nightly fuzz run fuzz_ticket_parsing       # Fuzz ticket parsing
```

### Security Audit Checklist

- [ ] Code review by security researcher
- [ ] Static analysis with `cargo-clippy` and `cargo-deny`
- [ ] Dependency audit: `cargo audit`
- [ ] Fuzzing on all input parsing
- [ ] Penetration testing (brute-force, MITM, replay)
- [ ] Formal security proof for TOFU model
- [ ] Third-party cryptography review

---

## 12. Deployment Recommendations

### Pre-Deployment Security Checklist

- [ ] All Phase 1 enhancements implemented
- [ ] Security audit completed
- [ ] Key rotation policy established
- [ ] Audit logging enabled
- [ ] Rate limiting configured
- [ ] Incident response plan documented
- [ ] Security policy deployed and communicated

### Monitoring & Detection

```toml
[monitoring]
# Alert on suspicious patterns
alert_on_failed_auth_threshold = 10  # Per hour
alert_on_large_transfer = "5GB"
alert_on_key_age_exceeding_days = 200
alert_on_log_integrity_failure = true
alert_on_wormhole_exhaustion = true
```

### Incident Response

```markdown
## Incident Response Plan

### Suspected Key Compromise
1. Immediately revoke compromised key
2. Rotate all active identities
3. Review audit logs for unauthorized access
4. Notify users of affected sessions
5. Issue new credentials

### Brute-Force Attack Detected
1. Enable emergency lockdown mode
2. Block attacking IP ranges (if available)
3. Enable MFA for all users
4. Review logs for successful breaches
5. Restore from backups if needed

### Audit Log Tampering Detected
1. Take system offline immediately
2. Preserve all logs
3. Involve security team and law enforcement
4. Forensic investigation
5. Rebuild from known-good backups
```

---

## Summary & Next Steps

This proposal provides a **defense-in-depth security architecture** for Irosh, addressing:

✅ **Cryptographic hardening** (zeroization, permissions, HMAC)  
✅ **Authentication security** (rate limiting, MFA, timeouts)  
✅ **Trust management** (integrity verification, key rotation)  
✅ **Transport security** (bindings, replay prevention)  
✅ **Audit trails** (forensics, compliance)  
✅ **Operational policies** (configuration-driven)

### Immediate Actions (Week 1)

1. **Review this proposal** with security team
2. **Prioritize Phase 1** enhancements
3. **Create GitHub issues** for each implementation task
4. **Assign security point person** for oversight

### Long-Term Vision

Transform **Irosh into a reference implementation** for secure P2P remote access, suitable for:
- 🏢 Enterprise deployments
- 🔒 High-security government/finance use cases
- 📱 IoT & edge device management
- 🛡️ Post-compromise forensics and recovery

---

## References & Resources

- **Cryptography**
  - [OWASP: Password Storage Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)
  - [RFC 2104: HMAC](https://tools.ietf.org/html/rfc2104)
  - [RFC 8439: ChaCha20/Poly1305](https://tools.ietf.org/html/rfc8439)

- **SSH Security**
  - [IETF RFC 4251: SSH Protocol Architecture](https://tools.ietf.org/html/rfc4251)
  - [OWASP: Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html)

- **Zero-Trust & P2P**
  - [Zero Trust Principles](https://www.nist.gov/publications/zero-trust-architecture)
  - [BEP (Bittorrent Enhancement Proposal) #44: DHT Security Extension](http://www.bittorrent.org/beps/bep_0044.html)

- **Rust Security**
  - [OWASP: Secure Coding Practices in Rust](https://github.com/OWASP/www-project-rust-secure-code-review-benchmark)
  - [Zeroize Crate Docs](https://docs.rs/zeroize/latest/zeroize/)

---

**Document Version**: 1.0  
**Last Updated**: 2026-05-06  
**Maintainers**: Security Team

---

**Questions or feedback?** Open an issue with the `security` label.
