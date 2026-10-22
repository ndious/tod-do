use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};
use clap_complete::CompleteEnv;

mod commands;
mod skill;

/// The ids of every task (backlog + contexts), for the shell
/// completion of the commands that take a task identifier.
fn task_id_candidates() -> Vec<CompletionCandidate> {
    let Ok(store) = dot::store::Store::discover() else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    let mut push = |tasks: Vec<dot::task::Task>, context: &str| {
        for task in tasks {
            candidates.push(CompletionCandidate::new(task.uid).help(Some(
                format!("[{context}] {}", task.description).into(),
            )));
        }
    };
    if let Ok(backlog) = store.load_backlog() {
        push(backlog, "backlog");
    }
    if let Ok(contexts) = store.contexts() {
        for (name, tasks) in contexts {
            push(tasks, &name);
        }
    }
    candidates
}

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
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
        /// The step description
        step: String,
    },
    /// Toggle a step of a task's plan (1-based)
    Check {
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
        /// Step number (1-based)
        number: usize,
    },
    /// (Re)generate and print the spec markdown of a task
    Brief {
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
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
        /// Keep only the tasks carrying this tag
        #[arg(short = 't', long, value_name = "TAG")]
        tag: Option<String>,
    },
    /// Replace the tags of a task (no tags clears them)
    Tag {
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
        /// The tags (comma or space separated)
        tags: Vec<String>,
    },
    /// Mark a task as complete, wherever it lives
    X {
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
    },
    /// Mark a task as ready for review and push its branch
    Rfr {
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
    },
    /// Delete a task
    Rm {
        /// Delete from the backlog instead of the current context
        #[arg(short, long)]
        backlog: bool,
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
    },
    /// Move a task between the backlog and the current context
    Mv {
        /// Move from the current context to the backlog
        #[arg(short, long)]
        backlog: bool,
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
    },
    /// Start a task: create a working branch for it and move it there
    Start {
        /// Full or partial task identifier
        #[arg(add = ArgValueCandidates::new(task_id_candidates))]
        identifier: String,
        /// Branch name (default: derived from the task description)
        #[arg(short, long, value_name = "NAME")]
        branch: Option<String>,
        /// Branch created from this branch instead of main
        #[arg(long, value_name = "BRANCH")]
        base: Option<String>,
    },
    /// Finish the current task: rebase it, merge it into main,
    /// delete the branch and close the context
    Finish {
        /// The base branch to merge into
        #[arg(short, long, value_name = "BRANCH", default_value = "main")]
        base: String,
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
    /// Print the shell completion script (bash | zsh | fish | powershell | elvish)
    Completions {
        /// The target shell
        shell: String,
    },
}

fn main() -> Result<()> {
    // answer the shell completion requests before anything else
    CompleteEnv::with_factory(|| Cli::command().bin_name("tod")).complete();
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
        Commands::Ls { backlog, all, all_contexts, tag } => commands::ls(backlog, all, all_contexts, tag),
        Commands::Tag { identifier, tags } => commands::tag(&identifier, tags),
        Commands::X { identifier } => commands::done(&identifier),
        Commands::Rfr { identifier } => commands::rfr(&identifier),
        Commands::Rm { backlog, identifier } => commands::rm(&identifier, backlog),
        Commands::Mv { backlog, identifier } => commands::mv(&identifier, backlog),
        Commands::Start { identifier, branch, base } => commands::start(&identifier, branch, base.as_deref()),
        Commands::Finish { base } => commands::finish(&base),
        Commands::Ctx => commands::ctx(),
        Commands::Resolve { context } => commands::resolve(&context),
        Commands::Export { output } => commands::export(&output),
        Commands::Completions { shell } => commands::completions(&shell),
    }
}
