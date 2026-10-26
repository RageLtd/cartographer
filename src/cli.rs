use crate::{grammars, hooks, parse_cli};

/// Run a CLI subcommand (`args[0]`) with its arguments. Returns `true` if a
/// command was handled.
pub fn run(args: &[String]) -> bool {
    let command = args.first().map(String::as_str).unwrap_or_default();
    match command {
        "grammars" => grammars::run_cli(&grammars::load_registry()),
        "parse" => std::process::exit(parse_cli::run_cli(&args[1..])),
        "hook:context" => hooks::hook_context(),
        "hook:prompt" => hooks::hook_prompt(),
        "hook:pre-read" => hooks::hook_pre_read(),
        "hook:pre-edit" => hooks::hook_pre_edit(),
        "hook:post-edit" => hooks::hook_post_edit(),
        "hook:post-compact" => hooks::hook_post_compact(),
        _ => {
            eprintln!("Unknown command: {command}");
            eprintln!("Usage: cartographer                  # Start MCP server (full mode, needs SurrealDB)");
            eprintln!("       cartographer --parse-only      # Start MCP server (parse-only, no DB required)");
            eprintln!("       cartographer grammars           # List built-in and host grammars; validate ~/.cartographer/grammars");
            eprintln!("       cartographer parse <file>       # Parse one file and print its imports and symbols as JSON");
            eprintln!("       cartographer hook:context       # SessionStart hook");
            eprintln!("       cartographer hook:prompt        # UserPromptSubmit hook");
            eprintln!("       cartographer hook:pre-read      # PreToolUse Read hook");
            eprintln!("       cartographer hook:pre-edit      # PreToolUse Edit/Write hook");
            eprintln!("       cartographer hook:post-edit     # PostToolUse Edit/Write/Bash hook");
            eprintln!("       cartographer hook:post-compact  # PostCompact hook");
            std::process::exit(1);
        }
    }
    true
}
