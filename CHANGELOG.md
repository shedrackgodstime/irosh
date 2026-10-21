# Changelog

All notable changes to irosh are documented here.

## Unreleased

Reliability and security fixes from the second audit pass:

- **Blobs-ALPN failure is no longer fatal.** A peer that does not offer the
  blobs ALPN, or a blobs dial that fails or times out, now degrades to
  "no blobs connection" instead of tearing down the healthy SSH connection.
  Content-addressed blob transfers are unavailable in that case; everything
  else (shell, exec, scp) keeps working.
- **Blank stealth secrets are treated as "no secret".** An empty or
  whitespace-only secret supplied through the client/server builders, the
  config file, or config import is normalized away. A stealth ALPN derived
  from a degenerate secret provided zero secrecy while making the peer look
  hidden; the base `irosh/1` ALPN is used instead.
- **Home-directory resolution is hardened.** A blank, missing, or relative
  `HOME`/`USERPROFILE` is treated as unset, so `~` and `~/x` no longer
  resolve to a relative path against the server's working directory.
- **Durable atomic writes.** `atomic_write_secure` now fsyncs the parent
  directory after the rename, so a rotated identity, trust, or shadow file
  cannot silently revert on power loss. Filesystems that do not support
  directory fsync are tolerated.
- **`scripts/build_android.sh` no longer hardcodes a personal toolchain
  path.** The musl cross toolchain is located via `IROSH_MUSL_TOOLCHAIN_DIR`,
  a pre-set `TOOLCHAIN_DIR`, or `aarch64-linux-musl-gcc` on `PATH`.

## v0.5.0 - 2026-09-21

Hardening and tooling: runtime metrics counters, tracing coverage across
the public API, CI fuzz targets with a coverage gate, transfer and SSH
benchmarks, and public API soundness fixes.

## Foundation (v0.1.0 - v0.4.0) - 2026-04-20 .. 2026-05-31

Early-stage bootstrap: SSH sessions over the Iroh P2P transport, a unified
single-binary CLI, wormhole rendezvous codes, a unified authenticator with
rate limiting, cross-platform service management, and Windows stability.

---

v0.5.0 is the last release of this bootstrap phase. The project changes
direction after it, so this changelog restarts cleanly from there.