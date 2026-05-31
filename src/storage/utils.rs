//! Common utilities for secure and atomic storage operations.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::error::{Result, StorageError};

/// Atomically writes data to a file by writing to a temporary file first
/// and then performing an OS-level rename.
///
/// This also ensures the file has strict permissions (0600) on Unix-like systems,
/// and equivalent restricted ACLs on Windows.
pub fn atomic_write_secure(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| StorageError::DirectoryCreate {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;

    // 1. Ensure parent directory exists and has strict permissions
    ensure_dir_secure(parent)?;

    // 2. Create a temporary file in the same directory
    let tmp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp_path).map_err(|source| StorageError::FileWrite {
        path: tmp_path.clone(),
        source,
    })?;

    // 3. Set strict permissions (0600 / ACLs) on the temp file before writing data
    apply_secure_permissions(&tmp_path)?;

    // 4. Write data and sync to disk
    file.write_all(data)
        .map_err(|source| StorageError::FileWrite {
            path: tmp_path.clone(),
            source,
        })?;
    file.sync_all().map_err(|source| StorageError::FileWrite {
        path: tmp_path.clone(),
        source,
    })?;
    drop(file);

    // 5. Atomic rename
    fs::rename(&tmp_path, path).map_err(|source| StorageError::FileWrite {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(())
}

/// Ensures a directory exists and has strict permissions (0700) on Unix/Windows.
pub fn ensure_dir_secure(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|source| StorageError::DirectoryCreate {
            path: path.to_path_buf(),
            source,
        })?;
    }

    apply_secure_permissions(path)?;

    Ok(())
}

/// Applies strict file-system permissions to the given path.
///
/// On Unix, files are set to `0o600` and directories to `0o700`.
/// On Windows, an ACL is applied that grants access only to the current user,
/// the SYSTEM account, and the Administrators group.
fn apply_secure_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).map_err(|source| StorageError::DirectoryRead {
            path: path.to_path_buf(),
            source,
        })?;
        let mut perms = metadata.permissions();
        let target_mode = if metadata.is_dir() { 0o700 } else { 0o600 };

        if perms.mode() & 0o777 != target_mode {
            perms.set_mode(target_mode);
            fs::set_permissions(path, perms).map_err(|source| StorageError::FileWrite {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::*;
        use windows_sys::Win32::Security::*;
        use windows_sys::Win32::System::Threading::*;

        const SECURITY_LOCAL_SYSTEM_RID: u32 = 0x00000012;
        const SECURITY_BUILTIN_DOMAIN_RID: u32 = 0x00000020;
        const DOMAIN_ALIAS_RID_ADMINS: u32 = 0x00000220;

        // SAFETY: Windows API calls for security descriptor manipulation.
        // We ensure that buffers are correctly sized and pointers are valid
        // by using established Win32 patterns (query size first, then allocate).
        unsafe {
            let mut process_token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut process_token) == 0 {
                return Err(StorageError::FileWrite {
                    path: path.to_path_buf(),
                    source: io::Error::last_os_error(),
                }
                .into());
            }

            let mut len = 0;
            let _ =
                GetTokenInformation(process_token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            if GetTokenInformation(
                process_token,
                TokenUser,
                buf.as_mut_ptr() as *mut _,
                len,
                &mut len,
            ) == 0
            {
                let _ = CloseHandle(process_token);
                return Err(StorageError::FileWrite {
                    path: path.to_path_buf(),
                    source: io::Error::last_os_error(),
                }
                .into());
            }
            let _ = CloseHandle(process_token);

            let token_user = buf.as_ptr() as *const TOKEN_USER;
            let user_sid = (*token_user).User.Sid;

            // Define SIDs for SYSTEM and Administrators
            let mut system_sid: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut admin_sid: *mut std::ffi::c_void = std::ptr::null_mut();
            let system_authority = SID_IDENTIFIER_AUTHORITY {
                Value: [0, 0, 0, 0, 0, 5],
            };
            let nt_authority = SID_IDENTIFIER_AUTHORITY {
                Value: [0, 0, 0, 0, 0, 5],
            };

            let res_system = AllocateAndInitializeSid(
                &system_authority,
                1,
                SECURITY_LOCAL_SYSTEM_RID,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut system_sid,
            );
            let res_admin = AllocateAndInitializeSid(
                &nt_authority,
                2,
                SECURITY_BUILTIN_DOMAIN_RID,
                DOMAIN_ALIAS_RID_ADMINS,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut admin_sid,
            );

            // Use a closure or a helper to handle cleanup and return errors
            let result: Result<()> = (|| {
                if res_system == 0 || res_admin == 0 {
                    return Err(StorageError::FileWrite {
                        path: path.to_path_buf(),
                        source: io::Error::last_os_error(),
                    }
                    .into());
                }

                // Initialize a security descriptor
                let mut sd: SECURITY_DESCRIPTOR = std::mem::zeroed();
                if InitializeSecurityDescriptor(&mut sd as *mut _ as *mut _, 1) == 0 {
                    return Err(StorageError::FileWrite {
                        path: path.to_path_buf(),
                        source: io::Error::last_os_error(),
                    }
                    .into());
                }

                // Create a DACL that allows the user, SYSTEM, and Administrators.
                let mut dacl_size = std::mem::size_of::<ACL>()
                    + (std::mem::size_of::<ACCESS_ALLOWED_ACE>() * 3)
                    + GetLengthSid(user_sid) as usize;

                if !system_sid.is_null() {
                    dacl_size += GetLengthSid(system_sid) as usize;
                }
                if !admin_sid.is_null() {
                    dacl_size += GetLengthSid(admin_sid) as usize;
                }

                let mut dacl_buf = vec![0u32; dacl_size.div_ceil(4)];
                let dacl = dacl_buf.as_mut_ptr() as *mut ACL;

                if InitializeAcl(dacl, (dacl_buf.len() * 4) as u32, ACL_REVISION) == 0 {
                    return Err(StorageError::FileWrite {
                        path: path.to_path_buf(),
                        source: io::Error::last_os_error(),
                    }
                    .into());
                }

                // Add ACEs for User, SYSTEM, and Administrators
                AddAccessAllowedAce(dacl, ACL_REVISION, GENERIC_ALL, user_sid);
                if !system_sid.is_null() {
                    AddAccessAllowedAce(dacl, ACL_REVISION, GENERIC_ALL, system_sid);
                }
                if !admin_sid.is_null() {
                    AddAccessAllowedAce(dacl, ACL_REVISION, GENERIC_ALL, admin_sid);
                }

                // Set the DACL to the security descriptor.
                if SetSecurityDescriptorDacl(&mut sd as *mut _ as *mut _, 1, dacl, 0) == 0 {
                    return Err(StorageError::FileWrite {
                        path: path.to_path_buf(),
                        source: io::Error::last_os_error(),
                    }
                    .into());
                }

                // Protect the DACL from inheritance (break inheritance).
                // 0x1000 is SE_DACL_PROTECTED
                let _ = SetSecurityDescriptorControl(&mut sd as *mut _ as *mut _, 0x1000, 0x1000);

                // Apply the security descriptor to the path.
                let path_u16: Vec<u16> = path
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();

                if SetFileSecurityW(
                    path_u16.as_ptr(),
                    DACL_SECURITY_INFORMATION,
                    &mut sd as *mut _ as *mut _,
                ) == 0
                {
                    return Err(StorageError::FileWrite {
                        path: path.to_path_buf(),
                        source: io::Error::last_os_error(),
                    }
                    .into());
                }

                Ok(())
            })();

            // ALWAYS free SIDs, even if the closure returned an error
            if !system_sid.is_null() {
                FreeSid(system_sid);
            }
            if !admin_sid.is_null() {
                FreeSid(admin_sid);
            }

            result?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-utils-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        path
    }

    #[test]
    fn atomic_write_secure_creates_file_with_content() {
        let dir = temp_dir("write-file");
        let file_path = dir.join("test.txt");
        atomic_write_secure(&file_path, b"hello world").unwrap();
        assert!(file_path.exists());
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_secure_overwrites_existing_file() {
        let dir = temp_dir("overwrite");
        let file_path = dir.join("test.txt");
        atomic_write_secure(&file_path, b"first").unwrap();
        atomic_write_secure(&file_path, b"second").unwrap();
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_secure_cleans_up_temp_file() {
        let dir = temp_dir("cleanup");
        let file_path = dir.join("test.txt");
        atomic_write_secure(&file_path, b"data").unwrap();
        // The .tmp file should have been renamed away
        let tmp_path = file_path.with_extension("tmp");
        assert!(!tmp_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_secure_errors_on_path_without_parent() {
        let result = atomic_write_secure(Path::new(""), b"data");
        assert!(result.is_err());
    }

    #[test]
    fn ensure_dir_secure_creates_directory() {
        let dir = temp_dir("ensure-dir");
        let sub = dir.join("sub").join("nested");
        assert!(!sub.exists());
        ensure_dir_secure(&sub).unwrap();
        assert!(sub.exists());
        assert!(sub.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_dir_secure_succeeds_on_existing_directory() {
        let dir = temp_dir("ensure-existing");
        std::fs::create_dir_all(&dir).unwrap();
        // Should not error on existing dir
        ensure_dir_secure(&dir).unwrap();
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_secure_writes_multiple_times() {
        let dir = temp_dir("multi-write");
        let file_path = dir.join("multi.txt");
        atomic_write_secure(&file_path, b"a").unwrap();
        atomic_write_secure(&file_path, b"b").unwrap();
        atomic_write_secure(&file_path, b"c").unwrap();
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "c");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
