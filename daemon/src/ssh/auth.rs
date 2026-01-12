#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    CredentialsPrompt,
    UsernamePrompt,
}

impl std::str::FromStr for SSHAuthMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "credentials_prompt" => Ok(SSHAuthMethod::CredentialsPrompt),
            "username_prompt" => Ok(SSHAuthMethod::UsernamePrompt),
            _ => Err(format!("invalid authentication method: {}", s)),
        }
    }
}
