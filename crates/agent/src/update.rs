use std::path::Path;

use moni_protocol::{AgentMessage, UpdateError, UpdateState, release};

use crate::config::Config;

pub const REQUEST_DIRECTORY: &str = "/var/lib/moni";

pub fn advertised_repo(config: &Config) -> Option<String> {
    (cfg!(target_os = "linux") && config.server.starts_with("wss://"))
        .then(|| config.update_repo.clone())
        .flatten()
}

pub fn validate(
    config: &Config,
    job_id: &str,
    requested: &str,
    repo: &str,
) -> Result<(), UpdateError> {
    if !config.server.starts_with("wss://") {
        return Err(UpdateError::InsecureTransport);
    }
    if !cfg!(target_os = "linux") {
        return Err(UpdateError::Unsupported);
    }
    let trusted = config.update_repo.as_deref().ok_or(UpdateError::Disabled)?;
    if repo != trusted || !release::valid_repo(repo) {
        return Err(UpdateError::RepoMismatch);
    }
    if !release::valid_job_id(job_id) {
        return Err(UpdateError::RequestFailed);
    }
    let requested = requested
        .starts_with('v')
        .then(|| release::version(requested))
        .flatten()
        .ok_or(UpdateError::InvalidVersion)?;
    let current = release::version(env!("CARGO_PKG_VERSION")).ok_or(UpdateError::InvalidVersion)?;
    if requested <= current {
        return Err(UpdateError::Downgrade);
    }
    Ok(())
}

pub fn status(job_id: String, version: String, error: Option<UpdateError>) -> AgentMessage {
    AgentMessage::UpdateStatus {
        job_id,
        version,
        state: if error.is_some() {
            UpdateState::Failed
        } else {
            UpdateState::Accepted
        },
        error,
    }
}

#[cfg(target_os = "linux")]
fn open_directory(
    directory: &Path,
) -> Result<(std::fs::File, std::path::PathBuf, u32), UpdateError> {
    use std::{
        fs::OpenOptions,
        os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };

    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(directory)
        .map_err(|_| UpdateError::RequestFailed)?;
    let metadata = directory
        .metadata()
        .map_err(|_| UpdateError::RequestFailed)?;
    // The root helper trusts this directory, so only the agent's uid may write it.
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(UpdateError::RequestFailed);
    }
    // Keep operations anchored to the checked directory even if its pathname changes.
    let directory_path =
        std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    Ok((directory, directory_path, uid))
}

#[cfg(target_os = "linux")]
fn existing_file(path: &Path, uid: u32, limit: u64) -> Result<Option<std::fs::File>, UpdateError> {
    use std::{
        fs::OpenOptions,
        io::ErrorKind,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };

    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(UpdateError::RequestFailed),
    };
    let metadata = file.metadata().map_err(|_| UpdateError::RequestFailed)?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
        || metadata.len() > limit
    {
        return Err(UpdateError::RequestFailed);
    }
    Ok(Some(file))
}

#[cfg(target_os = "linux")]
pub fn write_request(directory: &Path, job_id: &str, version: &str) -> Result<(), UpdateError> {
    use std::{
        fs::{self, OpenOptions},
        io::{ErrorKind, Read, Write},
        os::unix::fs::OpenOptionsExt,
    };

    if !release::valid_job_id(job_id)
        || !version.starts_with('v')
        || release::version(version).is_none()
    {
        return Err(UpdateError::RequestFailed);
    }
    let (directory, directory_path, uid) = open_directory(directory)?;
    let request = directory_path.join("update-request");
    let payload = format!("{version}\n");
    let existing = || -> Result<bool, UpdateError> {
        let Some(file) = existing_file(&request, uid, 64)? else {
            return Ok(false);
        };
        let mut contents = String::new();
        file.take(65)
            .read_to_string(&mut contents)
            .map_err(|_| UpdateError::RequestFailed)?;
        if contents.len() > 64 {
            return Err(UpdateError::RequestFailed);
        }
        if contents == payload {
            Ok(true)
        } else {
            Err(UpdateError::Busy)
        }
    };
    if existing()? {
        return Ok(());
    }

    let temporary = directory_path.join(format!(".update-request-{job_id}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&temporary)
        .map_err(|error| {
            if error.kind() == ErrorKind::AlreadyExists {
                UpdateError::Busy
            } else {
                UpdateError::RequestFailed
            }
        })?;
    let result = (|| {
        file.write_all(payload.as_bytes())
            .map_err(|_| UpdateError::RequestFailed)?;
        file.sync_all().map_err(|_| UpdateError::RequestFailed)?;
        // Publishing without replacement preserves a request another process just created.
        match fs::hard_link(&temporary, &request) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if existing()? {
                    Ok(())
                } else {
                    Err(UpdateError::Busy)
                }
            }
            Err(_) => Err(UpdateError::RequestFailed),
        }
    })();
    drop(file);
    let cleaned = fs::remove_file(&temporary).map_err(|_| UpdateError::RequestFailed);
    result?;
    cleaned?;
    directory.sync_all().map_err(|_| UpdateError::RequestFailed)
}

#[cfg(target_os = "linux")]
pub fn write_health(directory: &Path) -> Result<(), UpdateError> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::OpenOptionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    let (directory, directory_path, uid) = open_directory(directory)?;
    let health = directory_path.join("health");
    existing_file(&health, uid, 128)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let temporary = directory_path.join(format!(".health-{}-{nonce}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&temporary)
        .map_err(|_| UpdateError::RequestFailed)?;
    let result = (|| {
        let payload = format!("{}\n{}\n", env!("CARGO_PKG_VERSION"), std::process::id());
        file.write_all(payload.as_bytes())
            .map_err(|_| UpdateError::RequestFailed)?;
        file.sync_all().map_err(|_| UpdateError::RequestFailed)?;
        existing_file(&health, uid, 128)?;
        fs::rename(&temporary, &health).map_err(|_| UpdateError::RequestFailed)?;
        directory.sync_all().map_err(|_| UpdateError::RequestFailed)
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(target_os = "linux"))]
pub fn write_request(_: &Path, _: &str, _: &str) -> Result<(), UpdateError> {
    Err(UpdateError::Unsupported)
}

#[cfg(not(target_os = "linux"))]
pub fn write_health(_: &Path) -> Result<(), UpdateError> {
    Err(UpdateError::Unsupported)
}

#[cfg(test)]
mod tests;
