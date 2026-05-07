# Security Implementation Audit Checklist

This document isolates the **actionable, critical security implementation details** required for the V2 migration, filtering out the over-engineered bloat from previous proposals.

Since Iroh handles Transport Security (QUIC/Noise encryption) and Replay Protection natively at the network layer, we only need to implement Application-Layer defenses.

---

## 1. Required Implementation (MVP)

These items MUST be implemented during the V2 migration.

### [ ] 1. Secret Zeroization
**Risk:** Password hashes or secret keys lingering in RAM can be read by other processes or memory dumps.
**Implementation:** Use the `zeroize` crate (`zeroize = { version = "1", features = ["zeroize_derive"] }`).
- Call `.zeroize()` on the Node Password string immediately after Argon2 hashing is complete.
- Ensure the `SecretKey` and `NodeIdentity` structs drop their key material securely.

### [ ] 2. Strict File Permissions (Unix)
**Risk:** `~/.irosh/server/identity/private.key` being readable by other users on a shared Linux/macOS system.
**Implementation:** 
- When creating the identity or trust files, immediately set permissions to `0o600` (read/write owner only).
- If loading an existing key, verify it is `0o600`. If not, print a `[WARN]` and fix it automatically.

### [ ] 3. Path Traversal Prevention (File Transfers)
**Risk:** Even though `irosh get/put` only accesses designated folders/files, a malicious peer might send a requested filename like `../../../../etc/passwd` to escape the sandbox.
**Implementation:**
- Strip all `../` components from incoming file transfer requests.
- Canonicalize paths using `std::fs::canonicalize` and verify they still strictly start with the intended base directory.

### [ ] 4. Authentication Rate Limiting
**Risk:** Brute-forcing the Wormhole password or Node Password.
**Implementation:**
- Maintain an in-memory counter of failed attempts per NodeID/IP.
- Apply exponential backoff (e.g., 3 fails = 1 minute lockout, 4 fails = 5 minute lockout).

---

## 2. Future Security Enhancements (V3+)

These features are excellent for enterprise environments but are strictly out of scope for the current foundation. They should not be implemented until the core architecture is proven stable.

| Feature | Description |
| :--- | :--- |
| **Audit Logging** | Structured, cryptographically signed JSON logs (`~/.irosh/logs/`) tracking every auth attempt, session start, and file transfer. |
| **Encrypted Backups** | Using the `age` crate to export `~/.irosh/server/identity` as an encrypted `.age` bundle rather than just a zip file. |
| **MFA / TOTP** | Requiring an authenticator app code (Time-Based One-Time Password) in addition to the Node Password. |
| **HSM Support** | Storing the Ed25519 identity key on a Hardware Security Module (like a YubiKey or TPM) rather than the filesystem. |
| **Transfer Checksums** | Sending SHA-256 hashes along with file chunks during `put/get` to detect accidental network corruption. |
