/// Everything a call can fail with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request never got an answer: DNS, TLS, a timeout.
    #[error("vast.ai request failed: {0}")]
    Transport(#[from] reqwest::Error),

    /// Vast answered with an error status. `body` is what it said, which for
    /// most refusals (an offer already taken, a bid below the minimum) is the
    /// only explanation there is.
    #[error("vast.ai answered {status} to {method} {path}: {body}")]
    Status {
        method: &'static str,
        path: String,
        status: u16,
        body: String,
    },

    /// A 2xx whose body says `"success": false`. Vast does this for some
    /// refusals rather than using a status code.
    #[error("vast.ai refused {method} {path}: {message}")]
    Refused {
        method: &'static str,
        path: String,
        message: String,
    },

    /// The body was not the shape this crate expects.
    #[error("vast.ai sent an unexpected body for {path}: {source}")]
    Decode {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    /// The client was built with an empty API key.
    #[error("the vast.ai API key is empty")]
    MissingApiKey,
}

impl Error {
    /// Whether retrying the same request later could succeed: rate limiting,
    /// a server error, or the network.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Transport(_) => true,
            Error::Status { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// The HTTP status, when Vast answered at all.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}
