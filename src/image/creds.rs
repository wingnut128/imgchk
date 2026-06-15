use std::path::PathBuf;
use std::process::Command;

use oci_client::secrets::RegistryAuth;
use serde::Deserialize;

/// Resolves [`RegistryAuth`] for a given registry hostname.
///
/// Implementors return [`RegistryAuth::Anonymous`] when they have no
/// credentials to offer, allowing chains to fall through to the next
/// resolver.
pub trait CredentialResolver: Send + Sync {
    fn for_registry(&self, registry: &str) -> RegistryAuth;
}

/// Reads `IMGCHK_REGISTRY_USER` / `IMGCHK_REGISTRY_TOKEN` from the
/// environment. Returns [`RegistryAuth::Anonymous`] if either is missing.
pub struct EnvCredentials;

impl CredentialResolver for EnvCredentials {
    fn for_registry(&self, _registry: &str) -> RegistryAuth {
        match (
            std::env::var("IMGCHK_REGISTRY_USER"),
            std::env::var("IMGCHK_REGISTRY_TOKEN"),
        ) {
            (Ok(user), Ok(token)) if !user.is_empty() && !token.is_empty() => {
                RegistryAuth::Basic(user, token)
            }
            _ => RegistryAuth::Anonymous,
        }
    }
}

/// Reads `~/.docker/config.json` and shells out to the configured
/// `docker-credential-*` helper.
pub struct DockerConfigCredentials;

impl CredentialResolver for DockerConfigCredentials {
    fn for_registry(&self, registry: &str) -> RegistryAuth {
        docker_credential(registry).unwrap_or(RegistryAuth::Anonymous)
    }
}

/// Default chain: env vars → docker config → anonymous.
pub struct DefaultCredentials;

impl CredentialResolver for DefaultCredentials {
    fn for_registry(&self, registry: &str) -> RegistryAuth {
        if let RegistryAuth::Basic(u, t) = EnvCredentials.for_registry(registry) {
            eprintln!("Using credentials from IMGCHK_REGISTRY_USER/TOKEN");
            return RegistryAuth::Basic(u, t);
        }
        if let RegistryAuth::Basic(u, t) = DockerConfigCredentials.for_registry(registry) {
            eprintln!("Using credentials from Docker credential store");
            return RegistryAuth::Basic(u, t);
        }
        RegistryAuth::Anonymous
    }
}

#[derive(Deserialize, Default)]
struct DockerConfig {
    #[serde(rename = "credsStore")]
    creds_store: Option<String>,
    #[serde(rename = "credHelpers")]
    cred_helpers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Deserialize)]
struct CredHelperResponse {
    #[serde(rename = "Username")]
    username: String,
    #[serde(rename = "Secret")]
    secret: String,
}

fn docker_credential(registry: &str) -> Option<RegistryAuth> {
    let config_path = docker_config_dir().join("config.json");
    let config_data = std::fs::read_to_string(&config_path).ok()?;
    let docker_config: DockerConfig = serde_json::from_str(&config_data).ok()?;

    let server_url = registry_to_server_url(registry);

    let helper_name = docker_config
        .cred_helpers
        .as_ref()
        .and_then(|h| h.get(registry).or_else(|| h.get(&server_url)))
        .cloned()
        .or(docker_config.creds_store)?;

    let helper_bin = format!("docker-credential-{helper_name}");
    let output = Command::new(&helper_bin)
        .arg("get")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(server_url.as_bytes());
            }
            child.wait_with_output()
        })
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let cred: CredHelperResponse = serde_json::from_slice(&output.stdout).ok()?;
    Some(RegistryAuth::Basic(cred.username, cred.secret))
}

fn docker_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DOCKER_CONFIG") {
        PathBuf::from(dir)
    } else if let Some(home) = super::home_dir() {
        home.join(".docker")
    } else {
        PathBuf::from(".docker")
    }
}

fn registry_to_server_url(registry: &str) -> String {
    match registry {
        "index.docker.io" | "registry-1.docker.io" | "docker.io" => {
            "https://index.docker.io/v1/".to_string()
        }
        other => format!("https://{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_eq(a: &RegistryAuth, b: &RegistryAuth) -> bool {
        match (a, b) {
            (RegistryAuth::Anonymous, RegistryAuth::Anonymous) => true,
            (RegistryAuth::Basic(u1, t1), RegistryAuth::Basic(u2, t2)) => u1 == u2 && t1 == t2,
            _ => false,
        }
    }

    /// Lock to serialize tests that mutate the process environment — env
    /// is global state and parallel test threads would race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn env_creds_returns_anonymous_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: tests with env mutation are serialized via ENV_LOCK.
        unsafe {
            std::env::remove_var("IMGCHK_REGISTRY_USER");
            std::env::remove_var("IMGCHK_REGISTRY_TOKEN");
        }
        assert!(auth_eq(
            &EnvCredentials.for_registry("ghcr.io"),
            &RegistryAuth::Anonymous,
        ));
    }

    #[test]
    fn env_creds_returns_basic_when_both_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("IMGCHK_REGISTRY_USER", "alice");
            std::env::set_var("IMGCHK_REGISTRY_TOKEN", "secret");
        }
        let result = EnvCredentials.for_registry("ghcr.io");
        unsafe {
            std::env::remove_var("IMGCHK_REGISTRY_USER");
            std::env::remove_var("IMGCHK_REGISTRY_TOKEN");
        }
        assert!(auth_eq(
            &result,
            &RegistryAuth::Basic("alice".into(), "secret".into()),
        ));
    }

    #[test]
    fn env_creds_anonymous_when_only_user_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("IMGCHK_REGISTRY_USER", "alice");
            std::env::remove_var("IMGCHK_REGISTRY_TOKEN");
        }
        let result = EnvCredentials.for_registry("ghcr.io");
        unsafe {
            std::env::remove_var("IMGCHK_REGISTRY_USER");
        }
        assert!(auth_eq(&result, &RegistryAuth::Anonymous));
    }

    #[test]
    fn default_chain_prefers_env_over_docker_config() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("IMGCHK_REGISTRY_USER", "envuser");
            std::env::set_var("IMGCHK_REGISTRY_TOKEN", "envtoken");
            // Point DOCKER_CONFIG at a nonexistent dir so the docker step
            // would fail if it were even consulted.
            std::env::set_var("DOCKER_CONFIG", "/nonexistent/imgchk-test");
        }
        let result = DefaultCredentials.for_registry("ghcr.io");
        unsafe {
            std::env::remove_var("IMGCHK_REGISTRY_USER");
            std::env::remove_var("IMGCHK_REGISTRY_TOKEN");
            std::env::remove_var("DOCKER_CONFIG");
        }
        assert!(auth_eq(
            &result,
            &RegistryAuth::Basic("envuser".into(), "envtoken".into()),
        ));
    }

    #[test]
    fn registry_to_server_url_special_cases_docker_hub() {
        assert_eq!(
            registry_to_server_url("index.docker.io"),
            "https://index.docker.io/v1/"
        );
        assert_eq!(
            registry_to_server_url("docker.io"),
            "https://index.docker.io/v1/"
        );
        assert_eq!(registry_to_server_url("ghcr.io"), "https://ghcr.io");
    }
}
