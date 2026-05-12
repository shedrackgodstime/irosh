# Borrow Objectives: Hardening Irosh via Competitive Analysis

While `iroh-ssh` provides a similar tunneling service, Irosh's "Native SSH" architecture (using `russh`) gives us a significant advantage in zero-dependency environments. To solidify our position, we will borrow and adapt the most robust infrastructure patterns from `iroh-ssh`.

## 🛠️ Windows Service Hardening
- [ ] **Virtual Service Account**: Transition from `LocalSystem` to `NT SERVICE\irosh` for least-privilege security.
- [ ] **Restricted SIDs**: Configure the service with an Unrestricted SID and grant it exclusive ACL permissions to the state directory.
- [ ] **Binary Staging**: Automatically copy the `irosh.exe` to `C:\ProgramData\irosh\` during `system install` to prevent breakage if the user moves the original binary.
- [ ] **SCM Failure Actions**: Configure the Windows Service Control Manager to automatically restart the service on failure with exponential backoff.

## 🛡️ Network & Connectivity
- [ ] **Automatic Firewall Rules**: Implement a Windows-specific helper to add/remove `irosh.exe` from the Windows Firewall during installation.
- [ ] **Extra Relay Support**: Add the `--extra-relay-url` flag to allow users to supplement the default Iroh mesh with their own private relays instead of replacing it.

## 💎 UX & Polish
- [ ] **Connection String Generation**: Add a helper in the `system status` or a new `identity show` command that generates a copy-pasteable `irosh user@<ID>` string.
- [ ] **Version Transparency**: Include the underlying `iroh` version in `irosh --version` to simplify troubleshooting.

---

> [!NOTE]
> **Why Irosh is still the superior vision:**
> Unlike `iroh-ssh`, which is just a "pipe" for an existing `sshd`, Irosh is a **complete, standalone P2P shell environment**. We work on systems where `sshd` doesn't exist (like minimal IoT distros or locked-down containers) and provide native features like P2P-native file transfers (`~put`/`~get`) that a simple tunnel cannot match.
