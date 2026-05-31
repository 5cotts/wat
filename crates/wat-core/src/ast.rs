/// A redirect attached to a command.
#[derive(Debug, Clone, PartialEq)]
pub enum Redirect {
    /// `> file`
    Out(String),
    /// `>> file`
    Append(String),
    /// `< file`
    In(String),
    /// `2> file`
    Err(String),
}

/// A single command with its name, arguments, and redirections.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: String,
    pub args: Vec<String>,
    pub redirects: Vec<Redirect>,
}

/// A sequence of commands joined by `|`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline(pub Vec<Command>);

/// How two pipelines are separated in a [`List`].
#[derive(Debug, Clone, PartialEq)]
pub enum Separator {
    /// `;`
    Semi,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `&` — run the preceding pipeline in the background.
    Background,
    /// End of input (last item in the list has no trailing separator).
    End,
}

/// A top-level list of pipelines with their separators.
#[derive(Debug, Clone, PartialEq)]
pub struct List(pub Vec<(Pipeline, Separator)>);
