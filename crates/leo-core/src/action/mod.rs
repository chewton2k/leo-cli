/// How one output line should be presented. Naming the intent rather than a
/// color lets each place that shows it pick its own style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Ordinary text.
    Plain,
    /// Secondary text: "Cancelled.", "No changes."
    Dim,
    /// A completed mutation.
    Good,
    /// Something the user should notice but that is not a failure.
    Warn,
    /// A failure.
    Bad,
    /// A directory name.
    Dir,
    /// A blank separator line.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

impl Line {
    pub fn plain(text: impl Into<String>) -> Line {
        Line {
            kind: Kind::Plain,
            text: text.into(),
        }
    }
    pub fn dim(text: impl Into<String>) -> Line {
        Line {
            kind: Kind::Dim,
            text: text.into(),
        }
    }
    pub fn good(text: impl Into<String>) -> Line {
        Line {
            kind: Kind::Good,
            text: text.into(),
        }
    }
    pub fn warn(text: impl Into<String>) -> Line {
        Line {
            kind: Kind::Warn,
            text: text.into(),
        }
    }
    pub fn bad(text: impl Into<String>) -> Line {
        Line {
            kind: Kind::Bad,
            text: text.into(),
        }
    }
    pub fn dir(text: impl Into<String>) -> Line {
        Line {
            kind: Kind::Dir,
            text: text.into(),
        }
    }
    pub fn blank() -> Line {
        Line {
            kind: Kind::Blank,
            text: String::new(),
        }
    }
}

mod frontmatter;

pub use frontmatter::*;
