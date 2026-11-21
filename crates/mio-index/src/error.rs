use std::fmt;

/// Failures the indexer reports upward. A single unreadable file is not an
/// error — it lands in the index with null metadata — so these are all
/// whole-operation failures.
#[derive(Debug)]
pub enum IndexError {
    Db(String),
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// The library root does not exist or is not a directory.
    BadRoot(String),
    /// A directory could not be enumerated; pruning after a partial walk is unsafe.
    Walk {
        path: String,
        detail: String,
    },
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Db(m) => write!(f, "index database: {m}"),
            IndexError::Sqlite(e) => write!(f, "sqlite: {e}"),
            IndexError::Io(e) => write!(f, "io: {e}"),
            IndexError::BadRoot(p) => write!(f, "not a directory: {p}"),
            IndexError::Walk { path, detail } => write!(f, "could not scan {path}: {detail}"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        IndexError::Sqlite(e)
    }
}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        IndexError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, IndexError>;
