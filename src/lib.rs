// irosh (Fat Library)
// All core networking, cryptography, and state synchronization go here.
// DO NOT put UI prompts or println! in this crate.

pub mod sys {
    #[cfg(unix)]
    pub mod unix;
    
    #[cfg(windows)]
    pub mod windows;
}

// TODO: Start migrating Auth, Vault, and Server logic from src_old/
