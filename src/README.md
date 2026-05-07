# Irosh: Peer-to-Peer Secure Shell Library

`irosh` is a high-level networking library that combines the [Iroh](https://iroh.computer) networking stack with the [SSH](https://en.wikipedia.org/wiki/Secure_Shell) protocol to provide secure, ad-hoc, and persistent P2P shells.

## Key Features

- **Self-Authenticating Nodes**: Uses Ed25519 keys for both network identity and SSH authentication.
- **NAT Traversal**: Automatic hole-punching and relaying via the Iroh stack.
- **Wormhole Pairing**: Secure out-of-band trust establishment using short human-friendly codes.
- **Unified Auth**: A flexible authentication system supporting Public Keys, Passwords, and TOFU.

## Crate Architecture

This crate follows a **"Fat Library"** design. All logic related to networking, cryptography, and protocol state resides here. The accompanying CLI (`irosh-cli`) is a thin wrapper around this library, handling only UI and OS-specific setup.

### Core Components

- **server**: The P2P SSH server implementation.
- **client**: The P2P SSH client implementation.
- **auth**: Pluggable authentication backends and security policies.
- **transport**: Low-level P2P ticket management and data transfer protocols.
- **storage**: Persistence layer for identities, trust records, and peer profiles.

## Feature Flags

- `server`: Enables the P2P SSH server and PTY orchestration.
- `client`: Enables the P2P SSH client and interactive session handlers.
- `storage`: Enables persistent storage for identities and trust records.
- `transport`: Enables the underlying Iroh networking stack.

## Security Notice

Irosh is built on top of `iroh` and `russh`. While the underlying protocols are industry-standard, this library is in early development. Users should perform their own security audits before using it for mission-critical infrastructure.
