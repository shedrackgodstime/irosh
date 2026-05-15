# Irosh Post-Migration Roadmap

This document tracks the development phases for Irosh v0.3.0 and beyond, now that the Iroh 1.0 migration is complete.

## Phase 1: High-Performance File Transfers (`iroh-blobs`)
*Goal: Replace manual stream-based file transfers with the native Iroh blobs protocol.*

- [ ] Implement `iroh-blobs` provider in the server.
- [ ] Add `iroh-blobs` client logic to the CLI.
- [ ] Implement BLAKE3 integrity checking for all transfers.
- [ ] Add transfer resume support (native to blobs).
- [ ] Benchmark transfer speeds over high-latency connections.

## Phase 2: Stealth Mode & Protocol Hardening
*Goal: Make Irosh nodes invisible to unauthorized scanners.*

- [ ] Implement **Challenge-Response ALPN** negotiation.
- [ ] Add server-side "Stealth Mode" toggle to ignore unknown ALPNs.
- [ ] Harden the `UnifiedAuthenticator` session persistence.
- [ ] Implement automatic session timeout for idle connections.

## Phase 3: UI/UX Polish (TUI Dashboard)
*Goal: Create a professional terminal interface for managing a fleet of nodes.*

- [ ] Design a `ratatui` based dashboard for the CLI.
- [ ] Implement real-time connection monitoring.
- [ ] Add a "File Browser" UI for `iroh-blobs` interactions.
- [ ] Add interactive "Device Pairing" flow with better visual feedback.

## Phase 4: Mobile & Cross-Platform Refinement
*Goal: Ensure a premium experience on Android and other unix-like systems.*

- [ ] Optimize the `musl` build for smaller binary size (LTO + Strip).
- [ ] Refine terminal resize handling on Android/Termux.
- [ ] Implement background keep-alive for the Android server.

---
*Note: Provisioning suite is intentionally excluded from this immediate roadmap as per USER request.*
