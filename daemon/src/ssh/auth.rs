#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    // both username is required
    Password,
    // only username
    None,
}

impl std::str::FromStr for SSHAuthMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "password" => Ok(SSHAuthMethod::Password),
            "none" => Ok(SSHAuthMethod::None),
            _ => Err(format!("invalid authentication method: {}", s)),
        }
    }
}
