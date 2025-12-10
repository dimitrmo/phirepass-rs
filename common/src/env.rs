#[derive(Clone, Debug)]
pub enum Mode {
    Development,
    Production,
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "development" | "dev" => Ok(Mode::Development),
            "production" | "prod" => Ok(Mode::Production),
            _ => Err(format!("invalid mode: {}", s)),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Mode::Development => "development",
            Mode::Production => "production",
        };
        write!(f, "{}", s)
    }
}
