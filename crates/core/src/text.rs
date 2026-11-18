//! The guard for the opt-in text features (`[text]` in the config).
//!
//! Text is only ever looked at inside the editor plugins, in files you
//! work on. Whatever the config says, it's never looked at in:
//!
//! - secret files: [`NEVER`], plus your `[text] exclude`;
//! - anything that isn't a file buffer: terminals, prompts, the Claude
//!   Code prompt, password prompts and the shell are never text features'
//!   business, because only editor plugins take part;
//! - encrypted buffers (Vim's 'key'), and outside `[text] projects` for
//!   snippets.
//!
//! The daemon sends the plugins the full exclusion list, so they decide
//! locally and file paths never leave the editor. Before any snippet goes
//! to an AI, [`redact`] takes out keys, tokens, emails, credentials in
//! URLs and the values of anything that looks like a password.

use crate::config::TextConfig;

/// Files never looked at. A pattern without `/` matches the file name; one
/// with `/` matches the end of the path. `*` is any run of characters.
pub const NEVER: &[&str] = &[
    ".env",
    ".env.*",
    "*.env",
    ".envrc",
    "*.pem",
    "*.key",
    "*.crt",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "id_rsa*",
    "id_dsa*",
    "id_ecdsa*",
    "id_ed25519*",
    "*.ppk",
    "secrets.*",
    "*secret*",
    "*password*",
    "*credential*",
    ".git-credentials",
    ".netrc",
    ".pgpass",
    ".npmrc",
    ".pypirc",
    ".htpasswd",
    "*.kdbx",
    "*.gpg",
    "*.asc",
    "*.age",
    "*.tfstate",
    "*.tfvars",
    ".ssh/*",
    ".gnupg/*",
    ".aws/*",
    ".kube/config",
    ".docker/config.json",
    ".password-store/*",
];

/// `*` matches any run of characters (including none), `?` one.
fn wildcard(pattern: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Whether `pattern` matches `path` (see [`NEVER`]).
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let chars = |s: &str| s.chars().collect::<Vec<_>>();
    if pattern.contains('/') {
        let pattern = pattern.trim_start_matches('/');
        wildcard(&chars(&format!("*/{pattern}")), &chars(path))
            || wildcard(&chars(pattern), &chars(path))
    } else {
        let name = path.rsplit('/').next().unwrap_or(path);
        wildcard(&chars(pattern), &chars(name))
    }
}

/// The built-in list and yours: what the daemon sends the plugins.
pub fn exclusions(config: &TextConfig) -> Vec<String> {
    NEVER
        .iter()
        .map(|p| p.to_string())
        .chain(config.exclude.iter().cloned())
        .collect()
}

pub fn excluded(path: &str, exclusions: &[String]) -> bool {
    exclusions.iter().any(|p| glob_match(p, path))
}

/// `~/code/app` as a full path, with no trailing `/`.
pub fn expand(dir: &str) -> String {
    let dir = match dir.strip_prefix("~/") {
        Some(rest) => format!(
            "{}/{rest}",
            std::env::var("HOME")
                .unwrap_or_default()
                .trim_end_matches('/')
        ),
        None => dir.to_string(),
    };
    dir.trim_end_matches('/').to_string()
}

/// Whether `path` is inside one of the (expanded) `projects`.
pub fn in_projects(path: &str, projects: &[String]) -> bool {
    projects
        .iter()
        .any(|p| !p.is_empty() && path.starts_with(&format!("{p}/")))
}

/// Whether a plugin may look at the file at `path` for shapes, or for
/// snippets too. `path` is `None` for a buffer without a file.
pub fn allowed(path: Option<&str>, config: &TextConfig, snippets: bool) -> bool {
    let Some(path) = path.filter(|p| p.starts_with('/')) else {
        return false;
    };
    if excluded(path, &exclusions(config)) {
        return false;
    }
    if snippets {
        let projects: Vec<String> = config.projects.iter().map(|p| expand(p)).collect();
        return config.ai_snippets && in_projects(path, &projects);
    }
    config.shapes
}

/// Whether `url`'s host is this machine: exactly `localhost`, `127.0.0.1`
/// or `[::1]`, with any port. `localhost.example.com` isn't.
pub fn is_local_url(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // After any user:password@.
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = if host_port.starts_with('[') {
        host_port.split_inclusive(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]"
    )
}

/// Whether the AI provider runs on this machine: a command (such as
/// `ollama run`), or an OpenAI-compatible server on localhost. Snippets
/// only go elsewhere with `[text] cloud = true`.
pub fn local_provider(ai: &crate::config::AiConfig) -> bool {
    use crate::config::AiProvider;
    match ai.provider {
        AiProvider::Command => true,
        AiProvider::None => ai.command.is_some(),
        AiProvider::Openai => ai.base_url.as_deref().is_some_and(is_local_url),
        AiProvider::Anthropic | AiProvider::ClaudeCode => false,
    }
}

/// Whether `flowbuddy coach` may send snippets to this provider.
pub fn snippets_may_go_to(text: &TextConfig, ai: &crate::config::AiConfig) -> bool {
    text.ai_snippets && (text.cloud || local_provider(ai))
}

/// Words that name a secret when a value follows them.
const SECRET_NAMES: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "private_key",
    "secret_key",
    "signing_key",
    "encryption_key",
    "auth",
    "credential",
    "credentials",
    "bearer",
];

/// Prefixes of well-known keys and tokens.
const KEY_PREFIXES: &[&str] = &[
    "sk-",
    "sk_live_",
    "sk_test_",
    "pk_live_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xapp-",
    "AKIA",
    "ASIA",
    "AIza",
    "ya29.",
    "eyJ",
    "npm_",
    "hf_",
];

/// Characters a token is made of, for the checks below.
fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "+/=_-.@:%~".contains(c)
}

/// One token's replacement, if it's something to hide.
fn redact_token(t: &str) -> Option<&'static str> {
    if let Some((user, host)) = t.split_once('@')
        && !user.is_empty()
        && !user.contains("://")
        && host.contains('.')
        && !host.starts_with('.')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c))
    {
        return Some("<email>");
    }
    if KEY_PREFIXES
        .iter()
        .any(|p| t.starts_with(p) && t.len() >= p.len() + 8)
    {
        return Some("<key>");
    }
    let hex = t.chars().filter(|c| c.is_ascii_hexdigit()).count();
    if t.len() >= 20 && hex == t.len() {
        return Some("<hex>");
    }
    // A long run of mixed letters and digits, as keys and hashes are.
    let alnum = t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "+/=_-".contains(c));
    let (upper, lower, digit) = (
        t.chars().any(|c| c.is_ascii_uppercase()),
        t.chars().any(|c| c.is_ascii_lowercase()),
        t.chars().any(|c| c.is_ascii_digit()),
    );
    if t.len() >= 24 && alnum && digit && (upper || lower) && !t.contains("__") {
        return Some("<secret>");
    }
    None
}

/// `scheme://user:pass@host` → `scheme://<credentials>@host`.
fn redact_url_credentials(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'')
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(at) if tail[..at].contains(':') => {
                out.push_str("<credentials>");
                rest = &tail[at..];
            }
            _ => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

/// After `password = ` or `"token": `, the value to the end of the line,
/// quote, comma or semicolon.
fn redact_assignments(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while i < line.len() {
        let hit = SECRET_NAMES
            .iter()
            .filter_map(|name| lower[i..].find(name).map(|at| (i + at, name.len())))
            .min_by_key(|(at, len)| (*at, std::cmp::Reverse(*len)));
        let Some((at, len)) = hit else { break };
        let after = &line[at + len..];
        // The name must end with the word: `DB_PASSWORD`, `apiToken`, but
        // not `token_count` or `password_hash`.
        let ends_name = !after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let gap = after;
        let sep = gap.find(|c: char| !(c.is_whitespace() || c == '"' || c == '\''));
        // `Bearer <token>` has no `=` or `:`.
        let bearer = lower[at..].starts_with("bearer") && gap.starts_with(' ');
        let Some(sep) = sep.filter(|s| {
            ends_name && (bearer || matches!(gap[*s..].chars().next(), Some('=' | ':')))
        }) else {
            out.push_str(&line[i..at + len]);
            i = at + len;
            continue;
        };
        let value_start = at + len + if bearer { sep } else { sep + 1 };
        let value = &line[value_start..];
        let lead = value.len() - value.trim_start().len();
        let quoted = value[lead..].starts_with(['"', '\'']);
        let body = &value[lead + usize::from(quoted)..];
        let end = body
            .find(|c: char| {
                if quoted {
                    c == '"' || c == '\''
                } else {
                    c == ',' || c == ';' || c.is_whitespace()
                }
            })
            .unwrap_or(body.len());
        if end == 0 {
            out.push_str(&line[i..value_start]);
            i = value_start;
            continue;
        }
        let body_start = value_start + lead + usize::from(quoted);
        out.push_str(&line[i..body_start]);
        out.push_str("<redacted>");
        i = body_start + end;
    }
    out.push_str(&line[i.min(line.len())..]);
    out
}

/// Text with keys, tokens, emails, credentials in URLs and secret values
/// replaced. Lines of a private key block are dropped whole.
pub fn redact(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_key = false;
    for line in text.lines() {
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_key = true;
        }
        if in_key {
            if line.contains("-----END") {
                in_key = false;
            }
            out.push("<private key>".to_string());
            continue;
        }
        let line = redact_assignments(&redact_url_credentials(line));
        let mut cleaned = String::with_capacity(line.len());
        let mut token = String::new();
        let flush = |token: &mut String, cleaned: &mut String| {
            let t = token.trim_end_matches(['.', ':']);
            match redact_token(t) {
                Some(r) => {
                    cleaned.push_str(r);
                    cleaned.push_str(&token[t.len()..]);
                }
                None => cleaned.push_str(token),
            }
            token.clear();
        };
        for c in line.chars() {
            if token_char(c) {
                token.push(c);
            } else {
                flush(&mut token, &mut cleaned);
                cleaned.push(c);
            }
        }
        flush(&mut token, &mut cleaned);
        out.push(cleaned);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_urls_are_exactly_local() {
        for u in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:1234/v1",
            "https://LOCALHOST/v1",
            "localhost:11434",
        ] {
            assert!(is_local_url(u), "{u}");
        }
        for u in [
            "http://localhost.example.com/v1",
            "http://127.0.0.1.example.com",
            "http://localhost@example.com/v1",
            "https://api.example.com/localhost",
            "http://example.com:80/?h=localhost",
            "",
        ] {
            assert!(!is_local_url(u), "{u}");
        }
    }

    fn on() -> TextConfig {
        TextConfig {
            shapes: true,
            ai_snippets: true,
            projects: vec!["/home/me/code/app".into()],
            ..Default::default()
        }
    }

    #[test]
    fn secret_files_are_never_looked_at() {
        for path in [
            "/home/me/code/app/.env",
            "/home/me/code/app/.env.local",
            "/home/me/code/app/prod.env",
            "/home/me/code/app/certs/server.pem",
            "/home/me/code/app/tls.key",
            "/home/me/.ssh/config",
            "/home/me/.ssh/id_ed25519",
            "/home/me/code/app/config/secrets.yml",
            "/home/me/code/app/db_password.txt",
            "/home/me/.aws/credentials",
            "/home/me/.kube/config",
            "/home/me/.git-credentials",
            "/home/me/.netrc",
            "/home/me/vault.kdbx",
            "/home/me/notes.txt.gpg",
            "/home/me/code/app/infra/main.tfvars",
        ] {
            assert!(!allowed(Some(path), &on(), false), "{path}");
            assert!(!allowed(Some(path), &on(), true), "{path}");
        }
        assert!(allowed(Some("/home/me/code/app/src/main.rs"), &on(), false));
        assert!(allowed(Some("/home/me/code/app/src/main.rs"), &on(), true));
    }

    #[test]
    fn your_exclusions_projects_and_switches() {
        let mut c = on();
        c.exclude = vec!["*.sql".into(), "notes/*".into()];
        assert!(!allowed(Some("/home/me/code/app/db/schema.sql"), &c, false));
        assert!(!allowed(
            Some("/home/me/code/app/notes/today.md"),
            &c,
            false
        ));
        // Snippets only inside a project; shapes anywhere.
        assert!(allowed(Some("/tmp/scratch.py"), &c, false));
        assert!(!allowed(Some("/tmp/scratch.py"), &c, true));
        assert!(!allowed(Some("/home/me/code/application/x.rs"), &c, true));
        // Off is off; no file, a terminal or a prompt, is never allowed.
        assert!(!allowed(
            Some("/home/me/code/app/a.rs"),
            &TextConfig::default(),
            false
        ));
        assert!(!allowed(
            Some("/home/me/code/app/a.rs"),
            &TextConfig::default(),
            true
        ));
        assert!(!allowed(None, &c, false));
        assert!(!allowed(Some("term://~//1234:/bin/bash"), &c, false));
        assert!(!allowed(Some("[Prompt]"), &c, false));
    }

    #[test]
    fn globs() {
        assert!(glob_match("*.pem", "/a/b/c.pem"));
        assert!(!glob_match("*.pem", "/a/b.pem/c.txt"));
        assert!(glob_match(".ssh/*", "/home/me/.ssh/known_hosts"));
        assert!(!glob_match(".ssh/*", "/home/me/ssh/known_hosts"));
        assert!(glob_match("id_rsa*", "/x/id_rsa.pub"));
        assert!(glob_match("?.txt", "/x/a.txt") && !glob_match("?.txt", "/x/ab.txt"));
    }

    #[test]
    fn redacts_secrets() {
        let cases = [
            (
                "let key = \"sk-ant-api03-abcdefghijklmnop\";",
                "let key = \"<key>\";",
            ),
            ("password = hunter2", "password = <redacted>"),
            (
                "  \"apiToken\": \"abc123\",",
                "  \"apiToken\": \"<redacted>\",",
            ),
            ("DB_PASSWORD=s3cr3t ./run", "DB_PASSWORD=<redacted> ./run"),
            ("mail me@example.com today", "mail <email> today"),
            (
                "postgres://admin:pw@db.local/app",
                "postgres://<credentials>@db.local/app",
            ),
            (
                "sha = 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b",
                "sha = <hex>",
            ),
            ("aws AKIAIOSFODNN7EXAMPLE x", "aws <key> x"),
            ("x = Zm9vYmFyMTIzNDU2Nzg5MGFiY2RlZmdo", "x = <secret>"),
            (
                "Authorization: Bearer abc.def.ghi",
                "Authorization: Bearer <redacted>",
            ),
            ("aws_credentials = abc", "aws_credentials = <redacted>"),
        ];
        for (input, want) in cases {
            assert_eq!(redact(input), want, "{input}");
        }
        assert_eq!(
            redact(
                "a\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3Blbn\n-----END OPENSSH PRIVATE KEY-----\nz"
            ),
            "a\n<private key>\n<private key>\n<private key>\nz"
        );
    }

    #[test]
    fn snippets_stay_local_unless_allowed() {
        use crate::config::{AiConfig, AiProvider};
        let ai = |provider, base_url: Option<&str>| AiConfig {
            provider,
            base_url: base_url.map(Into::into),
            ..Default::default()
        };
        let mut t = on();
        assert!(snippets_may_go_to(&t, &ai(AiProvider::Command, None)));
        assert!(snippets_may_go_to(
            &t,
            &ai(AiProvider::Openai, Some("http://localhost:11434/v1"))
        ));
        assert!(!snippets_may_go_to(&t, &ai(AiProvider::Openai, None)));
        assert!(!snippets_may_go_to(&t, &ai(AiProvider::Anthropic, None)));
        assert!(!snippets_may_go_to(&t, &ai(AiProvider::ClaudeCode, None)));
        t.cloud = true;
        assert!(snippets_may_go_to(&t, &ai(AiProvider::Anthropic, None)));
        t.ai_snippets = false;
        assert!(!snippets_may_go_to(&t, &ai(AiProvider::Command, None)));
    }

    #[test]
    fn leaves_ordinary_code_alone() {
        for line in [
            "fn word_hops(lines: &[String], start: Pos) -> Option<u32> {",
            "    let token_count = tokens.len();",
            "if (user.passwordHash === hash(input)) {",
            "SOME_CONSTANT_NAME_THAT_IS_VERY_LONG = 1",
            "see https://example.com/docs/page",
            "// the secret is to keep it short",
        ] {
            assert_eq!(redact(line), line);
        }
    }
}
