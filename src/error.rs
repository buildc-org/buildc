#[derive(Debug, Clone, PartialEq, Eq)]
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
        Self::Operation(message)
    }
}

impl From<common::Error> for Error {
    fn from(error: common::Error) -> Self {
        match error {
            common::Error::Cancelled => Self::Cancelled,
            common::Error::Interrupted | common::Error::Failed(_) => {
                Self::Operation(error.to_string())
            }
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
