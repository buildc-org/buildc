pub enum Error {
    Invocation(String),
    Operation(String),
    Cancelled,
}

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Self::Invocation(message) | Self::Operation(message) => message,
            Self::Cancelled => "cancelled",
        }
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        match common::prompt::cancelled(&message) {
            true => Self::Cancelled,
            false => Self::Operation(message),
        }
    }
}
