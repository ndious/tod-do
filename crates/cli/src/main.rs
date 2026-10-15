use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod skill;

/// tod — a git-native todo CLI with planning support.
///
/// Each git branch is a context. Each task is a single file, named by
/// a sha1 identifier that you can abbreviate, just like git commits.
/// Complex tasks get a plan (micro todo list) and a spec markdown,
/// the brief you hand to a development agent.
#[derive(Parser)]
#[command(name = "tod", version, about = "A git-native todo CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the tod storage in the current git repository
    Init {
        /// Install the agent skill without asking
        #[arg(long, conflicts_with_all = ["no_skill", "agent"])]
        skill: bool,
        /// Do not install the agent skill
        #[arg(long)]
        no_skill: bool,
        /// Install the skill for this agent, without asking
        /// (claude | cursor | generic)
        #[arg(long, value_name = "AGENT", conflicts_with_all = ["skill", "no_skill"])]
        agent: Option<String>,
    },
    /// Install (or remove) the agent skill of this repository
    Skill {
        /// Remove the skill instead of installing it
        #[arg(short, long)]
        remove: bool,
        /// Target agent: claude | cursor | generic (default: ask)
        #[arg(long, value_name = "AGENT")]
        agent: Option<String>,
    },
    /// Create a new todo in the current context, or in the backlog (-b)
    Add {
        /// Create the todo in the backlog instead of the current context
        #[arg(short, long)]
        backlog: bool,
        /// Description of the todo
        description: String,
    },
    /// Create a task with a plan and its spec markdown
    Plan {
        /// Create the task in the backlog instead of the current context
        #[arg(short, long)]
        backlog: bool,
        /// Description of the task
        description: String,
    },
    /// Add a step (micro todo) to a task's plan
    Sub {
        /// Full or partial task identifier
        identifier: String,
        /// The step description
        step: String,
    },
    /// Toggle a step of a task's plan (1-based)
    Check {
        /// Full or partial task identifier
        identifier: String,
        /// Step number (1-based)
        number: usize,
    },
    /// (Re)generate and print the spec markdown of a task
    Brief {
        /// Full or partial task identifier
        identifier: String,
    },
    /// Project overview: backlog and contexts, with progress
    Status,
    /// List todos of the current context, or of the backlog (-b)
    Ls {
        /// Show the completed tasks too
        #[arg(short, long)]
        all: bool,
        /// List the backlog instead of the current context
        #[arg(short, long)]
        backlog: bool,
        /// List every context (and the backlog), not only the current one
        #[arg(short = 'A', long)]
        all_contexts: bool,
    },
    /// Mark a task as complete (current context only)
    X {
        /// Full or partial task identifier
        identifier: String,
    },
    /// Delete a task
    Rm {
        /// Delete from the backlog instead of the current context
        #[arg(short, long)]
        backlog: bool,
        /// Full or partial task identifier
        identifier: String,
    },
    /// Move a task between the backlog and the current context
    Mv {
        /// Move from the current context to the backlog
        #[arg(short, long)]
        backlog: bool,
        /// Full or partial task identifier
        identifier: String,
    },
    /// Start a task: create a working branch for it and move it there
    Start {
        /// Full or partial task identifier
        identifier: String,
        /// Branch name (default: derived from the task description)
        #[arg(short, long, value_name = "NAME")]
        branch: Option<String>,
    },
    /// List contexts (branches) that still hold todos
    Ctx,
    /// Close a context: delete all its todos
    Resolve {
        /// Context (branch) name
        context: String,
    },
    /// Export the whole tod database as a JSON file
    Export {
        /// Output file path
        output: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { skill, no_skill, agent } => commands::init(skill, no_skill, agent),
        Commands::Skill { remove, agent } => commands::skill(remove, agent),
        Commands::Add { backlog, description } => commands::add(&description, backlog),
        Commands::Plan { backlog, description } => commands::plan(&description, backlog),
        Commands::Sub { identifier, step } => commands::sub(&identifier, &step),
        Commands::Check { identifier, number } => commands::check(&identifier, number),
        Commands::Brief { identifier } => commands::brief(&identifier),
        Commands::Status => commands::status(),
        Commands::Ls { backlog, all, all_contexts } => commands::ls(backlog, all, all_contexts),
        Commands::X { identifier } => commands::done(&identifier),
        Commands::Rm { backlog, identifier } => commands::rm(&identifier, backlog),
        Commands::Mv { backlog, identifier } => commands::mv(&identifier, backlog),
        Commands::Start { identifier, branch } => commands::start(&identifier, branch),
        Commands::Ctx => commands::ctx(),
        Commands::Resolve { context } => commands::resolve(&context),
        Commands::Export { output } => commands::export(&output),
    }
}
