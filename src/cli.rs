use crate::hooks;

/// Run a CLI subcommand. Returns `true` if a command was handled.
pub fn run(command: &str) -> bool {
    match command {
        "hook:context" => hooks::hook_context(),
        "hook:prompt" => hooks::hook_prompt(),
        "hook:pre-read" => hooks::hook_pre_read(),
        "hook:pre-edit" => hooks::hook_pre_edit(),
        "hook:post-edit" => hooks::hook_post_edit(),
        "hook:post-compact" => hooks::hook_post_compact(),
        _ => {
            eprintln!("Unknown command: {command}");
            eprintln!("Usage: cartographer                # Start MCP server");
            eprintln!("       cartographer hook:context    # SessionStart hook");
            eprintln!("       cartographer hook:prompt     # UserPromptSubmit hook");
            eprintln!("       cartographer hook:pre-read   # PreToolUse Read hook");
            eprintln!("       cartographer hook:pre-edit   # PreToolUse Edit/Write hook");
            eprintln!("       cartographer hook:post-edit  # PostToolUse Edit/Write/Bash hook");
            eprintln!("       cartographer hook:post-compact # PostCompact hook");
            std::process::exit(1);
        }
    }
    true
}
