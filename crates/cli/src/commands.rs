use std::fs;
use std::io::{IsTerminal, Write};

use anyhow::{bail, Result};

use dot::brief;
use dot::git;
use dot::plan::Project;
use dot::store::Store;
use dot::task::Task;

pub fn init(skill: bool, no_skill: bool, agent: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    store.init()?;
    println!(
        "Initialized tod: branch '{}' checked out in {}",
        dot::store::TOD_BRANCH,
        store.tod_dir().display()
    );
    println!("Your todos are versioned. Share them with: git push origin {}", dot::store::TOD_BRANCH);

    // --agent <name>: install directly. --skill: install, asking
    // which agent when interactive, generic otherwise. --no-skill:
    // skip. No flag: ask (default yes) when interactive, skip in
    // scripts.
    let install_for = if let Some(flag) = agent {
        Some(parse_agent(&flag)?)
    } else if skill {
        Some(choose_agent_interactive()?)
    } else if no_skill {
        None
    } else if std::io::stdin().is_terminal() && ask_install_skill()? {
        Some(choose_agent_interactive()?)
    } else {
        None
    };
    if let Some(agent) = install_for {
        install_skill(&store, agent)?;
    }
    Ok(())
}

fn parse_agent(flag: &str) -> Result<crate::skill::Agent> {
    crate::skill::Agent::from_flag(flag)
        .ok_or_else(|| anyhow::anyhow!("unknown agent '{flag}' (claude | cursor | generic)"))
}

fn ask_install_skill() -> Result<bool> {
    print!("\nInstall the tod skill for your AI agent? [Y/n] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    let answer = answer.trim().to_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

/// Which agent do you use? The skill folder differs per agent.
fn choose_agent_interactive() -> Result<crate::skill::Agent> {
    println!("\nWhich agent do you use?");
    println!("  1) Claude Code    (.claude/skills/tod/)");
    println!("  2) Cursor         (.cursor/skills/tod/)");
    println!("  3) Mistral Vibe   (.vibe/skills/tod/)");
    println!("  4) other / standard (.agents/skills/tod/)");
    print!("Choose [1-4, default 4]: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(match answer.trim() {
        "1" => crate::skill::Agent::Claude,
        "2" => crate::skill::Agent::Cursor,
        "3" => crate::skill::Agent::Vibe,
        _ => crate::skill::Agent::Generic,
    })
}

pub fn skill(remove: bool, agent: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    if remove {
        if let Some(flag) = agent {
            let agent = parse_agent(&flag)?;
            if crate::skill::remove(&store, agent)? {
                println!("Removed the {} skill", agent.label());
            } else {
                println!("No skill installed for {}", agent.label());
            }
        } else {
            let removed = crate::skill::remove_all(&store)?;
            println!("Removed {removed} skill(s)");
        }
    } else {
        let agent = match agent {
            Some(flag) => parse_agent(&flag)?,
            None => choose_agent_interactive()?,
        };
        install_skill(&store, agent)?;
    }
    Ok(())
}

fn install_skill(store: &Store, agent: crate::skill::Agent) -> Result<()> {
    let path = crate::skill::install(store, agent)?;
    println!("Installed the {} skill: {}", agent.label(), path.display());
    println!("Commit it so your agent picks it up in every checkout.");
    Ok(())
}

pub fn add(description: &str, backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let author = git::user_name();
    let task = store.add(description, &author, backlog)?;
    println!("Added #{} {}", &task.uid[..5], task.description);
    Ok(())
}

pub fn plan(description: &str, backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let author = git::user_name();
    let task = store.add(description, &author, backlog)?;
    let context = if backlog {
        "backlog".to_string()
    } else {
        store.branch()?
    };

    let mut project = Project::load(&store)?;
    project.ensure_plan(&task.uid);
    project.save(&store)?;

    let plan = project.plan(&task.uid).cloned().unwrap_or_default();
    let path = brief::write_brief(&store, &task, &context, &plan)?;
    println!("Planned #{} {} — {}", &task.uid[..5], task.description, path.display());
    Ok(())
}

pub fn sub(identifier: &str, step: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (context, task) = store.find_task(identifier)?;

    let mut project = Project::load(&store)?;
    let number = project.add_step(&task.uid, step);
    project.save(&store)?;
    refresh_brief(&store, &task, &context, &project)?;

    println!("Added step {number} to #{}: {step}", &task.uid[..5]);
    Ok(())
}

pub fn check(identifier: &str, number: usize) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (context, task) = store.find_task(identifier)?;

    let mut project = Project::load(&store)?;
    let step = project.check_step(&task.uid, number)?;
    project.save(&store)?;
    refresh_brief(&store, &task, &context, &project)?;

    let mark = if step.done { 'x' } else { ' ' };
    println!("[{mark}] step {number} of #{}: {}", &task.uid[..5], step.text);
    Ok(())
}

pub fn brief(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (context, task) = store.find_task(identifier)?;
    let project = Project::load(&store)?;
    let plan = project.plan(&task.uid).cloned().unwrap_or_default();

    let path = brief::write_brief(&store, &task, &context, &plan)?;
    let content = fs::read_to_string(&path)?;
    println!("{content}");
    Ok(())
}

pub fn status() -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let project = Project::load(&store)?;

    let print = |label: &str, tasks: &[Task]| {
        println!("{label}:");
        if tasks.is_empty() {
            println!("  (none)");
            return;
        }
        for task in tasks {
            let mark = if task.done { 'x' } else { ' ' };
            let progress = match project.plan(&task.uid) {
                Some(plan) if !plan.is_empty() => {
                    format!(" [{} {}]", plan.done_count(), plan.steps.len())
                }
                _ => String::new(),
            };
            println!("  [{mark}] #{} {}{progress}", &task.uid[..5], task.description);
        }
    };

    print("Backlog", &store.load_backlog()?);
    for (name, tasks) in store.contexts()? {
        print(&name, &tasks);
    }
    Ok(())
}

/// List the todos. Only the tasks left to do are shown, unless
/// 'all' is set. 'all_contexts' lists the backlog plus every
/// context instead of the current one.
pub fn ls(backlog: bool, all: bool, all_contexts: bool) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let visible = |tasks: &[Task]| -> Vec<Task> {
        tasks
            .iter()
            .filter(|t| all || !t.done)
            .cloned()
            .collect()
    };

    let print_list = |tasks: &[Task]| {
        for task in tasks {
            let mark = if task.done { "[x]" } else { "[ ]" };
            println!("{mark} #{}  {}", &task.uid[..5], task.description);
        }
    };

    if all_contexts {
        let backlog_tasks = store.load_backlog()?;
        let shown = visible(&backlog_tasks);
        println!("Backlog:");
        if shown.is_empty() {
            println!("  (none{})", if backlog_tasks.is_empty() { "" } else { " left to do" });
        } else {
            for task in &shown {
                let created = task.created.format("%Y-%m-%d");
                let mark = if task.done { "[x]" } else { "[ ]" };
                println!("{mark} #{}  {}  ({})", &task.uid[..5], task.description, created);
            }
        }
        for (name, tasks) in store.contexts()? {
            let shown = visible(&tasks);
            println!("{name}:");
            if shown.is_empty() {
                if tasks.is_empty() {
                    println!("  (none)");
                } else {
                    println!("  (all done)");
                }
            } else {
                print_list(&shown);
            }
        }
        return Ok(());
    }

    if backlog {
        let tasks = store.load_backlog()?;
        let shown = visible(&tasks);
        if shown.is_empty() {
            if tasks.is_empty() {
                println!("No backlog todos available");
            } else {
                println!("Backlog: everything is done ('tod ls -b -a' to show them)");
            }
            return Ok(());
        }
        println!("Backlog:");
        for task in &shown {
            let created = task.created.format("%Y-%m-%d");
            let mark = if task.done { "[x]" } else { "[ ]" };
            println!("{mark} #{}  {}  ({})", &task.uid[..5], task.description, created);
        }
    } else {
        let branch = store.branch()?;
        let tasks = store.load_context(&branch)?;
        let shown = visible(&tasks);
        if shown.is_empty() {
            if tasks.is_empty() {
                println!("No todos available in context '{branch}'");
            } else {
                println!("No todos left in context '{branch}' — all done ('tod ls -a' to show them)");
            }
            return Ok(());
        }
        print_list(&shown);
    }
    Ok(())
}

pub fn done(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    let task = store.complete(identifier)?;
    println!("Completed #{} {}", &task.uid[..5], task.description);
    Ok(())
}

pub fn rm(identifier: &str, backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let task = store.remove(identifier, backlog)?;
    forget_task(&store, &[task.uid.clone()])?;
    println!("Deleted #{} {}", &task.uid[..5], task.description);
    Ok(())
}

pub fn mv(identifier: &str, to_backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let task = store.move_task(identifier, to_backlog)?;
    let destination = if to_backlog {
        "backlog".to_string()
    } else {
        format!("context '{}'", store.branch()?)
    };
    println!("Moved #{} {} to {destination}", &task.uid[..5], task.description);
    Ok(())
}

/// Start a task: create a working branch named after it, switch to
/// it, and make the task belong to that branch.
pub fn start(identifier: &str, branch: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    start_in(&store, identifier, branch)
}

fn start_in(store: &Store, identifier: &str, branch: Option<String>) -> Result<()> {
    store.ensure_init()?;
    let (_context, task) = store.find_task(identifier)?;

    let branch = match branch {
        Some(name) => name,
        None => {
            let slug = git::branch_slug(&task.description);
            if slug.is_empty() {
                format!("tod-{}", &task.uid[..7])
            } else {
                slug
            }
        }
    };
    git::create_branch(store.root(), &branch)?;
    store.move_task_to(identifier, &branch)?;

    println!(
        "Started #{} {} on branch '{branch}'",
        &task.uid[..5], task.description
    );
    Ok(())
}

pub fn ctx() -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let contexts = store.contexts()?;
    if contexts.is_empty() {
        println!("No contexts with todos");
        return Ok(());
    }
    for (name, tasks) in &contexts {
        println!("{}: {} todo(s)", name, tasks.len());
    }
    Ok(())
}

pub fn resolve(context: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let tasks = store.load_context(context)?;
    let count = store.resolve(context)?;
    let uids: Vec<String> = tasks.iter().map(|t| t.uid.clone()).collect();
    forget_task(&store, &uids)?;
    println!("Resolved context '{context}': {count} todo(s) deleted");
    Ok(())
}

pub fn export(output: &str) -> Result<()> {
    let store = Store::discover()?;
    let (backlog, contexts) = store.export(output)?;
    println!("Exported {backlog} backlog todo(s) and {contexts} context(s) to {output}");
    Ok(())
}

/// Remove the plan entries and spec markdowns of deleted tasks.
fn forget_task(store: &Store, uids: &[String]) -> Result<()> {
    let mut project = Project::load(store)?;
    let mut changed = false;
    for uid in uids {
        project.remove(uid);
        changed = true;
        let _ = fs::remove_file(store.md_dir().join(format!("{uid}.md")));
    }
    if changed {
        project.save(store)?;
    }
    Ok(())
}

/// Regenerate the spec markdown, but only if it already exists:
/// 'tod brief' is the explicit way to (re)generate a brief.
fn refresh_brief(store: &Store, task: &Task, context: &str, project: &Project) -> Result<()> {
    let path = store.md_dir().join(format!("{}.md", task.uid));
    if path.exists() {
        let plan = project.plan(&task.uid).cloned().unwrap_or_default();
        brief::write_brief(store, task, context, &plan)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A temp directory with a real git repository and a store on it.
    fn git_repo() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        assert!(out.status.success(), "git init failed");
        let store = Store::new(dir.path());
        store.init().unwrap();
        (dir, store)
    }

    #[test]
    fn start_creates_a_branch_and_moves_the_task_into_it() {
        let (dir, store) = git_repo();
        let task = store.add_in(None, "Fix the login bug", "tester").unwrap();

        // the branch name comes from the task description
        start_in(&store, &task.uid[..7], None).unwrap();
        assert_eq!(
            git::current_branch_in(dir.path()).unwrap(),
            "fix-the-login-bug"
        );
        assert!(store.load_backlog().unwrap().is_empty());
        assert_eq!(store.load_context("fix-the-login-bug").unwrap().len(), 1);

        // an explicit branch name overrides the slug
        let task2 = store.add_in(None, "Another one", "tester").unwrap();
        start_in(&store, &task2.uid[..7], Some("custom".into())).unwrap();
        assert_eq!(git::current_branch_in(dir.path()).unwrap(), "custom");
        assert_eq!(store.load_context("custom").unwrap().len(), 1);

        // a useless description falls back to the uid prefix
        let task3 = store.add_in(None, "###", "tester").unwrap();
        start_in(&store, &task3.uid[..7], None).unwrap();
        assert_eq!(
            git::current_branch_in(dir.path()).unwrap(),
            format!("tod-{}", &task3.uid[..7])
        );
    }

    #[test]
    fn start_fails_when_the_branch_already_exists() {
        let (dir, store) = git_repo();
        let task = store.add_in(None, "Fix the login bug", "tester").unwrap();
        start_in(&store, &task.uid[..7], None).unwrap();

        let task2 = store.add_in(None, "Fix the login bug again", "tester").unwrap();
        // 'fix-the-login-bug-again' is free, but an existing name must fail
        assert!(start_in(&store, &task2.uid[..7], Some("fix-the-login-bug".into())).is_err());
        // nothing moved: the second task is still in the backlog
        assert_eq!(store.load_backlog().unwrap().len(), 1);
        assert_eq!(git::current_branch_in(dir.path()).unwrap(), "fix-the-login-bug");
    }
}

/// Print the shell completion script. The installation one-liner:
/// `eval "$(tod completions <shell>)"`.
pub fn completions(shell: &str) -> Result<()> {
    let shell = shell.to_lowercase();
    if !matches!(shell.as_str(), "bash" | "zsh" | "fish" | "powershell" | "elvish") {
        bail!("unknown shell '{shell}' (bash | zsh | fish | powershell | elvish)");
    }
    // the registration script is printed by the binary itself when
    // invoked with COMPLETE=<shell> and no arguments
    let exe = std::env::current_exe()?;
    let out = std::process::Command::new(exe)
        .env("COMPLETE", &shell)
        .output()
        .map_err(|e| anyhow::anyhow!("cannot run the completion generator: {e}"))?;
    print!("{}", String::from_utf8_lossy(&out.stdout));
    Ok(())
}
