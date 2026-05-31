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

/// A simple command with its leading variable assignments, name, arguments,
/// and redirections. `assignments` are `NAME=value` prefixes (value
/// unexpanded); when `name` is empty the command is a pure assignment
/// statement (e.g. `x=5`).
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleCommand {
    pub assignments: Vec<(String, String)>,
    pub name: String,
    pub args: Vec<String>,
    pub redirects: Vec<Redirect>,
}

/// A command is either a simple command or a compound control-flow construct.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Simple(SimpleCommand),
    Compound(CompoundCommand),
}

/// A control-flow construct. Bodies are nested [`List`]s, so the AST is a
/// recursive statement tree.
#[derive(Debug, Clone, PartialEq)]
pub enum CompoundCommand {
    /// `if cond; then body; [elif cond; then body;]* [else body;] fi`.
    /// `branches` holds the `(condition, then-body)` pairs (if + each elif).
    If {
        branches: Vec<(List, List)>,
        else_body: Option<List>,
    },
    /// `while cond; do body; done`.
    While { cond: List, body: List },
    /// `until cond; do body; done`.
    Until { cond: List, body: List },
    /// `for var in words; do body; done`.
    For {
        var: String,
        words: Vec<String>,
        body: List,
    },
    /// `case word in (pat|pat) body ;; ... esac`.
    Case { word: String, arms: Vec<CaseArm> },
}

/// One arm of a `case`: a set of (unexpanded) glob patterns and a body.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseArm {
    pub patterns: Vec<String>,
    pub body: List,
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
