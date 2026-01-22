use std::fmt::Display;

#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    // both username and password are required
    Password,
    // only username is required
    None,
}

impl Display for SSHAuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SSHAuthMethod::Password => write!(f, "Password"),
            SSHAuthMethod::None => write!(f, "None"),
        }
    }
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
