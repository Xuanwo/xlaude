use anyhow::Result;
use clap_complete::Shell;

pub fn handle_completions(shell: Shell) -> Result<()> {
    match shell {
        Shell::Bash => print_bash_completions(),
        Shell::Zsh => print_zsh_completions(),
        Shell::Fish => print_fish_completions(),
        _ => {
            eprintln!("Unsupported shell: {:?}", shell);
            eprintln!("Supported shells: bash, zsh, fish");
        }
    }
    Ok(())
}

fn print_bash_completions() {
    println!(
        r#"#!/bin/bash

_xlaude() {{
    local cur prev words cword
    if type _init_completion &>/dev/null; then
        _init_completion || return
    else
        # Fallback for older bash-completion
        COMPREPLY=()
        cur="${{COMP_WORDS[COMP_CWORD]}}"
        prev="${{COMP_WORDS[COMP_CWORD-1]}}"
        words=("${{COMP_WORDS[@]}}")
        cword=$COMP_CWORD
    fi

    # Main commands
    local commands="create checkout open delete add rename list clean dir completions config dashboard"

    # Complete main commands
    if [[ $cword -eq 1 ]]; then
        COMPREPLY=($(compgen -W "$commands" -- "$cur"))
        return
    fi

    # Complete subcommand arguments
    case "${{words[1]}}" in
        open|dir|delete)
            if [[ $cword -eq 2 ]]; then
                # Get worktree names for completion
                local worktrees=$(xlaude complete-worktrees 2>/dev/null)
                COMPREPLY=($(compgen -W "$worktrees" -- "$cur"))
            fi
            ;;
        rename)
            if [[ $cword -eq 2 ]]; then
                # Complete first argument (old name)
                local worktrees=$(xlaude complete-worktrees 2>/dev/null)
                COMPREPLY=($(compgen -W "$worktrees" -- "$cur"))
            fi
            ;;
        checkout)
            if [[ $cword -eq 2 ]]; then
                # Accept branch name or PR number
                COMPREPLY=()
            fi
            ;;
        completions)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "bash zsh fish" -- "$cur"))
            fi
            ;;
        config|dashboard)
            return
            ;;
    esac
}}

complete -F _xlaude xlaude
"#
    );
}

fn print_zsh_completions() {
    println!(
        r#"#compdef xlaude

_xlaude() {{
    local -a commands
    commands=(
        'create:Create a new git worktree'
        'open:Open an existing worktree and launch Claude'
        'checkout:Checkout a branch or pull request into a worktree'
        'delete:Delete a worktree and clean up'
        'add:Add current worktree to xlaude management'
        'rename:Rename a worktree'
        'list:List all active Claude instances'
        'clean:Clean up invalid worktrees from state'
        'dir:Get the directory path of a worktree'
        'completions:Generate shell completions'
        'config:Open the xlaude state file in $EDITOR'
        'dashboard:Launch the embedded dashboard'
    )

    # Main command completion
    if (( CURRENT == 2 )); then
        _describe 'command' commands
        return
    fi

    # Subcommand argument completion
    case "${{words[2]}}" in
        open|dir|delete)
            if (( CURRENT == 3 )); then
                _xlaude_worktrees
            fi
            ;;
        rename)
            if (( CURRENT == 3 )); then
                _xlaude_worktrees
            elif (( CURRENT == 4 )); then
                _message "new name"
            fi
            ;;
        create|add)
            if (( CURRENT == 3 )); then
                _message "worktree name"
            fi
            ;;
        checkout)
            if (( CURRENT == 3 )); then
                _message "branch name or pull request number"
            fi
            ;;
        completions)
            if (( CURRENT == 3 )); then
                local -a shells
                shells=(bash zsh fish)
                _describe 'shell' shells
            fi
            ;;
        config|dashboard)
            # No additional arguments
            return
            ;;
    esac
}}

_xlaude_worktrees() {{
    local -a worktrees
    local IFS=$'\n'
    
    # Get detailed worktree information (sorted by repo, then by name)
    local worktree_data
    worktree_data=($(xlaude complete-worktrees --format=detailed 2>/dev/null))
    
    if [[ -n "$worktree_data" ]]; then
        for line in $worktree_data; do
            # Parse tab-separated values: name<TAB>repo<TAB>path<TAB>sessions
            local name=$(echo "$line" | cut -f1)
            local repo=$(echo "$line" | cut -f2)
            local sessions=$(echo "$line" | cut -f4)
            
            # Add worktree with clear repo marker and session info
            worktrees+=("$name:[$repo] $sessions")
        done
        
        # Use _describe for better presentation
        # -V flag preserves the order (no sorting)
        if (( ${{#worktrees[@]}} > 0 )); then
            _describe -V -t worktrees 'worktree' worktrees
        fi
    else
        # Fallback to simple completion
        local simple_worktrees
        simple_worktrees=($(xlaude complete-worktrees 2>/dev/null))
        if [[ -n "$simple_worktrees" ]]; then
            compadd -a simple_worktrees
        fi
    fi
}}

_xlaude "$@"
"#
    );
}

fn print_fish_completions() {
    println!(
        r#"# Fish completion for xlaude

# Disable file completions by default
complete -c xlaude -f

# Main commands
complete -c xlaude -n "__fish_use_subcommand" -a create -d "Create a new git worktree"
complete -c xlaude -n "__fish_use_subcommand" -a open -d "Open an existing worktree and launch Claude"
complete -c xlaude -n "__fish_use_subcommand" -a delete -d "Delete a worktree and clean up"
complete -c xlaude -n "__fish_use_subcommand" -a add -d "Add current worktree to xlaude management"
complete -c xlaude -n "__fish_use_subcommand" -a rename -d "Rename a worktree"
complete -c xlaude -n "__fish_use_subcommand" -a list -d "List all active Claude instances"
complete -c xlaude -n "__fish_use_subcommand" -a clean -d "Clean up invalid worktrees from state"
complete -c xlaude -n "__fish_use_subcommand" -a dir -d "Get the directory path of a worktree"
complete -c xlaude -n "__fish_use_subcommand" -a checkout -d "Checkout a branch or pull request into a worktree"
complete -c xlaude -n "__fish_use_subcommand" -a completions -d "Generate shell completions"
complete -c xlaude -n "__fish_use_subcommand" -a config -d "Open the xlaude state file in $EDITOR"
complete -c xlaude -n "__fish_use_subcommand" -a dashboard -d "Launch the embedded dashboard"

# Function to get worktree completions with repo markers
function __xlaude_worktrees
    xlaude complete-worktrees --format=detailed 2>/dev/null | while read -l line
        # Split tab-separated values: name<TAB>repo<TAB>path<TAB>sessions
        set -l parts (string split \t $line)
        if test (count $parts) -ge 4
            set -l name $parts[1]
            set -l repo $parts[2]
            set -l sessions $parts[4]
            echo "$name\t[$repo] $sessions"
        end
    end
end

# Simple worktree names (fallback)
function __xlaude_worktrees_simple
    xlaude complete-worktrees 2>/dev/null
end

# Worktree completions for commands
complete -c xlaude -n "__fish_seen_subcommand_from open dir delete" -a "(__xlaude_worktrees)"
complete -c xlaude -n "__fish_seen_subcommand_from rename" -n "not __fish_seen_argument_from (__xlaude_worktrees_simple)" -a "(__xlaude_worktrees)"

# Shell completions for completions command
complete -c xlaude -n "__fish_seen_subcommand_from completions" -a "bash zsh fish"
"#
    );
}
