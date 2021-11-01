use toml;
use std::collections::HashMap;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct RookConfig {
    port: u16,
    hooks: HashMap<String, Hook>
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum Hook {
    #[serde(rename="github")]
    GithubHook {url_path: String, repo: String, secret_path: String, script_path: String},
}

const SAMPLE_CONFIG: &str = r#"
    port = 9000

    [hooks.myhook]
    type = "github"
    url_path = "/hooks/gh"
    repo = "numberoverzero/webhook-test"
    secret_path = "/home/crossj/webhook-test-secret.txt"
    script_path = "/home/crossj/webhook-test-script.sh"
"#;

pub fn parse_sample() -> RookConfig {
    return toml::from_str(SAMPLE_CONFIG).unwrap();
}
