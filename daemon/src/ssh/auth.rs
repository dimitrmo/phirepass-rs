#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    // both username and password are required
    Password,
    // only username is required
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
