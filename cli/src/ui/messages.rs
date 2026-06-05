/// Central registry for all user-facing tip strings.
///
/// Every tip shown to the user via `Ui::error(msg, tip)` should reference
/// a constant from this file. This makes it possible to:
///   1. Audit every tip in one place.
///   2. Ensure consistent casing and punctuation.
///   3. Update wording across the entire codebase from a single file.
// ── Initialisation & setup ─────────────────────────────────────────────────────

pub const TIP_INIT_FAILED: &str =
    "check that ~/.irosh exists and is readable, or re-run with --verbose";

// ── Peer / address book ───────────────────────────────────────────────────────

pub const TIP_PEER_LIST: &str = "run 'irosh peer list' to see saved peers";
pub const TIP_PEER_REMOVE_FIRST: &str =
    "remove the existing peer first with 'irosh peer remove'";
pub const TIP_PEER_USE_DIFFERENT_ALIAS: &str =
    "use a different alias, or remove the existing one with 'irosh peer remove'";

// ── Config ─────────────────────────────────────────────────────────────────────

pub const TIP_CONFIG_LIST: &str = "run 'irosh config list' to see all valid keys";

// ── Host / daemon ──────────────────────────────────────────────────────────────

pub const TIP_DAEMON_STATUS: &str =
    "run 'irosh system status' to inspect the running daemon";
pub const TIP_DAEMON_WORMHOLE: &str =
    "run 'irosh system status' for daemon health details";

// ── Security & auth ────────────────────────────────────────────────────────────

pub const TIP_AUTH_WRONG_PASSWORD: &str =
    "wrong password — ask the server admin or use 'irosh wormhole' to re-pair";
pub const TIP_AUTH_KEY_REJECTED: &str =
    "host key verification failed — run 'irosh trust list' to see trusted keys";

pub const TIP_WORMHOLE_PASSWD: &str =
    "run 'irosh passwd set' or use '--passwd' to issue a one-time invite";

// ── Wormhole ──────────────────────────────────────────────────────────────────

pub const TIP_WORMHOLE_TIMEOUT: &str =
    "wormhole codes expire after 60s — ask for a fresh one";
pub const TIP_WORMHOLE_CODE_LENGTH: &str =
    "use a longer code, or add a password with --passwd";

// ── Connectivity ───────────────────────────────────────────────────────────────

pub const TIP_CONNECTION_REFUSED: &str =
    "check that the remote server is running with 'irosh host'";

// ── Transport ──────────────────────────────────────────────────────────────────

pub const TIP_BLOB_STORE: &str =
    "check permissions on ~/.irosh/client/blobs, or re-run with --verbose";

// ── System checks ──────────────────────────────────────────────────────────────

pub const TIP_INSTALL_SSH: &str = "install openssh-client, then re-run 'irosh check'";
pub const TIP_UDP_FIREWALL: &str =
    "irosh requires UDP for P2P transport — check your firewall settings";
pub const TIP_CHECK_DIAGNOSTIC: &str = "run 'irosh check' for a full diagnostic";
pub const TIP_VERIFY_PATH: &str = "verify the path exists and is accessible from the current directory";

// ── Fallback ───────────────────────────────────────────────────────────────────

pub const TIP_FALLBACK: &str = "run with --verbose for full diagnostic details";
