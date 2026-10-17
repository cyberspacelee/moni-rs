//! Release identifiers cross both the administrator and local privilege boundaries.

pub fn valid_repo(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    [owner, repo].into_iter().all(|part| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    })
}

pub fn version(value: &str) -> Option<(u64, u64, u64)> {
    if value.len() > 64 {
        return None;
    }
    let mut parts = value.strip_prefix('v').unwrap_or(value).split('.');
    let mut component = || {
        let part = parts.next()?;
        if part.is_empty()
            || part.len() > 9
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        part.parse::<u64>().ok()
    };
    let parsed = (component()?, component()?, component()?);
    parts.next().is_none().then_some(parsed)
}

pub fn valid_job_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_identifiers_cannot_be_paths_commands_or_unordered_versions() {
        assert!(valid_repo("owner/moni-rs"));
        for value in [
            "../repo",
            "a/b/c",
            "a/..",
            "a/b?x",
            "a/b\n",
            "https://github.com/a/b",
        ] {
            assert!(!valid_repo(value), "{value:?}");
        }
        assert_eq!(version("v0.1.2"), Some((0, 1, 2)));
        assert!(version("0.10.0") > version("v0.9.9"));
        for value in [
            "v01.2.3",
            "v1.2",
            "v1.2.3.4",
            "v1.2.3-rc1",
            "v1.2.3+build",
            "v1.2.3/evil",
            "v1.2.3\n",
            "v18446744073709551616.0.0",
        ] {
            assert_eq!(version(value), None, "{value:?}");
        }
        assert!(valid_job_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_job_id("../update-request"));
    }
}
