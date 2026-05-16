# Irosh v0.4.0 Roadmap

This roadmap focuses on production hardening, security persistence, and cross-platform refinement.

## Active Development
- [ ] **Automatic Session Timeout** — implement idle-timeout logic to close inactive SSH channels and release P2P resources.
- [ ] **Authenticator Persistence** — harden `UnifiedAuthenticator` to survive daemon restarts by persisting session trust tokens and rate-limit states.
- [ ] **Android Client Polish** — refine terminal resizing and input handling for Termux and other mobile SSH environments.
- [ ] **Performance Benchmarking** — conduct transfer speed tests over high-latency and lossy P2P links to optimize `iroh-blobs` windowing.

## Completed (v0.3.x Cycle)
- [x] Native `iroh-blobs` integration (verified BLAKE3 transfers). ✅
- [x] Stealth Mode (ALPN-locked private discovery). ✅
- [x] Rich System Status (telemetry table). ✅
- [x] Headless Execution (`iroh connect --exec`). ✅
- [x] "Fat Library" architecture (Thin CLI). ✅

---
*Status: v0.3.0 is stable. v0.4.0 development is focused on reliability and edge-case hardening.*
