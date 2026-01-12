#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    // both username is required
    CredentialsPrompt,
    // only username
    PasswordlessPrompt,
}

impl std::str::FromStr for SSHAuthMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "credentials_prompt" => Ok(SSHAuthMethod::CredentialsPrompt),
            "passwordless_prompt" => Ok(SSHAuthMethod::PasswordlessPrompt),
            _ => Err(format!("invalid authentication method: {}", s)),
        }
    }
}
