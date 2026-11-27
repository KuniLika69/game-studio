use std::{
    fmt,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    Read {
        path: PathBuf,
        detail: String,
    },
    Invalid {
        path: Option<PathBuf>,
        context: String,
        detail: String,
    },
}

impl ImportError {
    pub fn read(path: impl AsRef<Path>, cause: impl ToString) -> Self {
        Self::Read {
            path: path.as_ref().to_owned(),
            detail: cause.to_string(),
        }
    }

    pub fn new(context: impl Into<String>, detail: impl ToString) -> Self {
        Self::Invalid {
            path: None,
            context: context.into(),
            detail: detail.to_string(),
        }
    }

    pub fn file(path: impl AsRef<Path>, detail: impl ToString) -> Self {
        Self::new("Импорт", detail).with_file(path)
    }

    pub fn with_file(self, path: impl AsRef<Path>) -> Self {
        match self {
            Self::Invalid {
                path: None,
                context,
                detail,
            } => Self::Invalid {
                path: Some(path.as_ref().to_owned()),
                context,
                detail,
            },
            error => error,
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, detail } => {
                write!(f, "Не удалось прочитать «{}». {detail}", path.display())
            }
            Self::Invalid {
                path: Some(path),
                context,
                detail,
            } if context.is_empty() => {
                write!(f, "Файл «{}». {detail}", path.display())
            }
            Self::Invalid {
                path: Some(path),
                context,
                detail,
            } => {
                write!(f, "{context}, файл «{}». {detail}", path.display())
            }
            Self::Invalid {
                path: None,
                context,
                detail,
            } if !context.is_empty() => {
                write!(f, "{context}. {detail}")
            }
            Self::Invalid { detail, .. } => f.write_str(detail),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<String> for ImportError {
    fn from(value: String) -> Self {
        Self::new("", value)
    }
}

impl From<&str> for ImportError {
    fn from(value: &str) -> Self {
        Self::new("", value)
    }
}

impl From<ImportError> for String {
    fn from(error: ImportError) -> Self {
        error.to_string()
    }
}
