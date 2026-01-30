/// Extract token_id and secret from PAT token format
/// Expected format: `pat_<token_id>.<secret>`
pub fn extract_creds(token: String) -> anyhow::Result<(String, String)> {
    let token = token.trim();
    if token.is_empty() {
        anyhow::bail!("token is required")
    }

    if !token.starts_with("pat_") {
        anyhow::bail!("invalid token format");
    }

    let Some(pat_body) = token.strip_prefix("pat_") else {
        anyhow::bail!("broken pat token format")
    };

    let (token_id, secret) = match pat_body.split_once('.') {
        Some((token_id, secret)) => (token_id, Some(secret)),
        None => {
            anyhow::bail!("failed to extract required token part")
        }
    };

    let Some(secret) = secret else {
        anyhow::bail!("failed to extract required secret")
    };

    Ok((token_id.to_string(), secret.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_creds_valid() {
        let result = extract_creds("pat_abc123.def456".to_string());
        assert!(result.is_ok());
        let (token_id, secret) = result.unwrap();
        assert_eq!(token_id, "abc123");
        assert_eq!(secret, "def456");
    }

    #[test]
    fn test_extract_creds_missing_prefix() {
        let result = extract_creds("abc123.def456".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_creds_missing_secret() {
        let result = extract_creds("pat_abc123".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_creds_empty() {
        let result = extract_creds("".to_string());
        assert!(result.is_err());
    }
}
