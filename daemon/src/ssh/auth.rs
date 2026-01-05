#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    CredentialsPrompt,
}

impl std::str::FromStr for SSHAuthMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "credentials_prompt" => Ok(SSHAuthMethod::CredentialsPrompt),
            _ => Err(format!("invalid authentication method: {}", s)),
        }
    }
}
