use clap::Parser;

use super::*;

const JOB: &str = "0123456789abcdef0123456789abcdef";
const OTHER_JOB: &str = "fedcba9876543210fedcba9876543210";

fn config() -> Config {
    Config::try_parse_from([
        "agent",
        "--server",
        "wss://example.com/ws/agent",
        "--token",
        "secret-token",
    ])
    .unwrap()
}

#[test]
fn update_commands_require_a_local_repository_secure_transport_and_a_new_stable_version() {
    let mut config = config();
    assert_eq!(advertised_repo(&config), None);
    assert_eq!(
        validate(&config, JOB, "v999.0.0", "owner/repo"),
        Err(UpdateError::Disabled)
    );
    config.update_repo = Some("owner/repo".into());
    #[cfg(target_os = "linux")]
    {
        assert_eq!(advertised_repo(&config).as_deref(), Some("owner/repo"));
        assert_eq!(validate(&config, JOB, "v999.0.0", "owner/repo"), Ok(()));
        for repo in ["other/repo", "../repo", "https://github.com/owner/repo"] {
            assert_eq!(
                validate(&config, JOB, "v999.0.0", repo),
                Err(UpdateError::RepoMismatch)
            );
        }
        for version in [
            "999.0.0",
            "vv999.0.0",
            "v0999.0.0",
            "v999.0.0-rc1",
            "v999.0.0+build",
            "v999.0.0\n",
            "v999.0.0/../../evil",
        ] {
            assert_eq!(
                validate(&config, JOB, version, "owner/repo"),
                Err(UpdateError::InvalidVersion)
            );
        }
        for version in [
            "v0.0.0".to_owned(),
            format!("v{}", env!("CARGO_PKG_VERSION")),
        ] {
            assert_eq!(
                validate(&config, JOB, &version, "owner/repo"),
                Err(UpdateError::Downgrade)
            );
        }
        assert_eq!(
            validate(&config, "../job", "v999.0.0", "owner/repo"),
            Err(UpdateError::RequestFailed)
        );
    }
    config.server = "ws://localhost/ws/agent".into();
    assert_eq!(advertised_repo(&config), None);
    assert_eq!(
        validate(&config, JOB, "v999.0.0", "owner/repo"),
        Err(UpdateError::InsecureTransport)
    );
}

#[test]
fn accepted_only_confirms_the_local_request_and_errors_are_enumerated() {
    assert!(matches!(
        status(JOB.into(), "v999.0.0".into(), None),
        AgentMessage::UpdateStatus {
            state: UpdateState::Accepted,
            error: None,
            ..
        }
    ));
    let failed = status(
        JOB.into(),
        "v999.0.0".into(),
        Some(UpdateError::RequestFailed),
    );
    assert!(matches!(
        failed,
        AgentMessage::UpdateStatus {
            state: UpdateState::Failed,
            error: Some(UpdateError::RequestFailed),
            ..
        }
    ));
    let json = serde_json::to_string(&failed).unwrap();
    assert!(!json.contains("secret-token"));
    assert!(!json.contains(REQUEST_DIRECTORY));
}

#[cfg(target_os = "linux")]
mod filesystem {
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        path::PathBuf,
        sync::{
            Arc, Barrier,
            atomic::{AtomicU64, Ordering},
        },
    };

    use super::*;

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "moni-agent-update-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&directory)
                .unwrap();
            Self(directory)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn requests_are_complete_private_idempotent_and_do_not_overwrite_other_versions() {
        let directory = Directory::new();
        let path = directory.0.join("update-request");
        assert_eq!(write_request(&directory.0, JOB, "v999.0.0"), Ok(()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "v999.0.0\n");
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(write_request(&directory.0, OTHER_JOB, "v999.0.0"), Ok(()));
        assert_eq!(fs::metadata(&path).unwrap().ino(), metadata.ino());
        assert_eq!(
            write_request(&directory.0, OTHER_JOB, "v1000.0.0"),
            Err(UpdateError::Busy)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "v999.0.0\n");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn symlinks_unsafe_permissions_and_large_requests_are_rejected() {
        let directory = Directory::new();
        let outside = directory.0.join("outside");
        fs::write(&outside, "unchanged").unwrap();
        let request = directory.0.join("update-request");
        symlink(&outside, &request).unwrap();
        assert_eq!(
            write_request(&directory.0, JOB, "v999.0.0"),
            Err(UpdateError::RequestFailed)
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "unchanged");
        fs::remove_file(&request).unwrap();
        let parent_link = directory.0.join("link");
        symlink(&directory.0, &parent_link).unwrap();
        assert_eq!(
            write_request(&parent_link, JOB, "v999.0.0"),
            Err(UpdateError::RequestFailed)
        );
        let temporary = directory.0.join(format!(".update-request-{JOB}.tmp"));
        symlink(&outside, &temporary).unwrap();
        assert_eq!(
            write_request(&directory.0, JOB, "v999.0.0"),
            Err(UpdateError::Busy)
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "unchanged");
        fs::remove_file(&temporary).unwrap();

        fs::write(&request, "x".repeat(65)).unwrap();
        assert_eq!(
            write_request(&directory.0, JOB, "v999.0.0"),
            Err(UpdateError::RequestFailed)
        );
        fs::write(&request, "v999.0.0\n").unwrap();
        fs::set_permissions(&request, fs::Permissions::from_mode(0o620)).unwrap();
        assert_eq!(
            write_request(&directory.0, JOB, "v999.0.0"),
            Err(UpdateError::RequestFailed)
        );
        fs::remove_file(&request).unwrap();
        fs::create_dir(&request).unwrap();
        assert_eq!(
            write_request(&directory.0, JOB, "v999.0.0"),
            Err(UpdateError::RequestFailed)
        );
        fs::remove_dir(&request).unwrap();
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o770)).unwrap();
        assert_eq!(
            write_request(&directory.0, JOB, "v999.0.0"),
            Err(UpdateError::RequestFailed)
        );
    }

    #[test]
    fn concurrent_jobs_cannot_replace_an_existing_request() {
        let directory = Directory::new();
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                write_request(&directory.0, JOB, "v999.0.0")
            });
            let second = scope.spawn(|| {
                barrier.wait();
                write_request(&directory.0, OTHER_JOB, "v1000.0.0")
            });
            let results = [first.join().unwrap(), second.join().unwrap()];
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert_eq!(
                results
                    .iter()
                    .filter(|result| **result == Err(UpdateError::Busy))
                    .count(),
                1
            );
        });
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        assert!(matches!(
            fs::read_to_string(directory.0.join("update-request"))
                .unwrap()
                .as_str(),
            "v999.0.0\n" | "v1000.0.0\n"
        ));
    }

    #[test]
    fn health_marker_atomically_replaces_old_regular_files_and_rejects_symlinks() {
        let directory = Directory::new();
        let health = directory.0.join("health");
        fs::write(&health, "old-version\n1\n").unwrap();
        fs::set_permissions(&health, fs::Permissions::from_mode(0o600)).unwrap();
        let previous = fs::metadata(&health).unwrap().ino();
        assert_eq!(write_health(&directory.0), Ok(()));
        assert_eq!(
            fs::read_to_string(&health).unwrap(),
            format!("{}\n{}\n", env!("CARGO_PKG_VERSION"), std::process::id())
        );
        let metadata = fs::metadata(&health).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_ne!(metadata.ino(), previous);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        fs::remove_file(&health).unwrap();
        let outside = directory.0.join("outside");
        fs::write(&outside, "unchanged").unwrap();
        symlink(&outside, &health).unwrap();
        assert_eq!(write_health(&directory.0), Err(UpdateError::RequestFailed));
        assert_eq!(fs::read_to_string(&outside).unwrap(), "unchanged");
        fs::remove_file(&health).unwrap();
        fs::create_dir(&health).unwrap();
        assert_eq!(write_health(&directory.0), Err(UpdateError::RequestFailed));
    }
}
