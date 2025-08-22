use anyhow::Result;
use atty::Stream;
use dialoguer::{Confirm, Select};
use once_cell::sync::Lazy;
use std::io::{self, BufRead, BufReader};
use std::sync::Mutex;

/// Check if stdin is piped (not a terminal)
pub fn is_piped_input() -> bool {
    !atty::is(Stream::Stdin)
}

/// Piped input reader that supports reading multiple lines
pub struct PipedInputReader {
    reader: BufReader<io::Stdin>,
    buffer: Vec<String>,
}

impl PipedInputReader {
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
            buffer: Vec::new(),
        }
    }

    /// Read the next line of input
    pub fn read_line(&mut self) -> Result<Option<String>> {
        // Use buffered input first if available
        if !self.buffer.is_empty() {
            return Ok(Some(self.buffer.remove(0)));
        }

        let mut line = String::new();
        match self.reader.read_line(&mut line)? {
            0 => Ok(None), // EOF
            _ => Ok(Some(line.trim().to_string())),
        }
    }

    /// Read all available input lines (non-blocking)
    #[allow(dead_code)]
    pub fn read_all_available(&mut self) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        let mut line = String::new();

        while self.reader.read_line(&mut line)? > 0 {
            lines.push(line.trim().to_string());
            line.clear();
        }

        Ok(lines)
    }
}

/// Global piped input reader (singleton)
static PIPED_INPUT: Lazy<Mutex<Option<PipedInputReader>>> = Lazy::new(|| {
    if is_piped_input() {
        Mutex::new(Some(PipedInputReader::new()))
    } else {
        Mutex::new(None)
    }
});

/// Read a single line from piped input
pub fn read_piped_line() -> Result<Option<String>> {
    let mut reader = PIPED_INPUT.lock().unwrap();
    match reader.as_mut() {
        Some(r) => r.read_line(),
        None => Ok(None),
    }
}

/// Smart confirmation that supports piped input (yes/no)
pub fn smart_confirm(prompt: &str, default: bool) -> Result<bool> {
    // 1. Check for force-yes environment variable
    if std::env::var("XLAUDE_YES").is_ok() {
        return Ok(true);
    }

    // 2. Check for piped input
    if let Some(input) = read_piped_line()? {
        let input = input.to_lowercase();
        return Ok(input == "y" || input == "yes");
    }

    // 3. Non-interactive mode uses default value
    if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
        return Ok(default);
    }

    // 4. Interactive confirmation
    Confirm::new()
        .with_prompt(prompt)
        .default(default)
        .interact()
        .map_err(Into::into)
}

/// Smart selection that supports piped input
pub fn smart_select<T>(
    prompt: &str,
    items: &[T],
    display_fn: impl Fn(&T) -> String,
) -> Result<Option<usize>>
where
    T: Clone,
{
    // 1. Check for piped input
    if let Some(input) = read_piped_line()? {
        // Try to parse as index
        if let Ok(index) = input.parse::<usize>()
            && index < items.len()
        {
            return Ok(Some(index));
        }

        // Try to match display text
        for (i, item) in items.iter().enumerate() {
            if display_fn(item) == input {
                return Ok(Some(i));
            }
        }

        anyhow::bail!("Invalid selection: {}", input);
    }

    // 2. Non-interactive mode returns None
    if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
        return Ok(None);
    }

    // 3. Interactive selection
    let display_items: Vec<String> = items.iter().map(display_fn).collect();
    let selection = Select::new()
        .with_prompt(prompt)
        .items(&display_items)
        .interact()?;

    Ok(Some(selection))
}

/// Get command argument with pipe input support
/// Priority: CLI argument > piped input > None
pub fn get_command_arg(arg: Option<String>) -> Result<Option<String>> {
    // 1. CLI argument takes priority
    if arg.is_some() {
        return Ok(arg);
    }

    // 2. Check piped input (skip yes/no confirmation words)
    // Only try to read once to avoid getting stuck with infinite streams like 'yes'
    if let Some(input) = read_piped_line()? {
        let lower = input.to_lowercase();
        // Skip confirmation words that might be in the pipe
        // These are likely from tools like 'yes' and not meant as actual input
        if lower != "y" && lower != "yes" && lower != "n" && lower != "no" {
            return Ok(Some(input));
        }
        // If it's a confirmation word, return None to let the command use defaults
    }

    Ok(None)
}

/// Drain any remaining piped input to prevent it from being passed to child processes
/// Note: We don't actually drain because tools like 'yes' provide infinite input.
/// Instead, we'll just ensure stdin is not inherited by child processes.
pub fn drain_stdin() -> Result<()> {
    // We used to try to drain all input here, but that causes problems with
    // tools like 'yes' that provide infinite input. The actual solution is
    // to not inherit stdin in child processes (using Stdio::null()).
    Ok(())
}

/// Get command argument or prompt for selection
/// Priority: CLI argument > piped input > interactive selection
#[allow(dead_code)]
pub fn get_arg_or_select<T>(
    arg: Option<String>,
    items: &[T],
    prompt: &str,
    display_fn: impl Fn(&T) -> String,
    match_fn: impl Fn(&T, &str) -> bool,
) -> Result<Option<T>>
where
    T: Clone,
{
    // 1. Try to get argument from CLI or pipe
    if let Some(name) = get_command_arg(arg)? {
        // Find matching item
        for item in items {
            if match_fn(item, &name) {
                return Ok(Some(item.clone()));
            }
        }
        anyhow::bail!("Item '{}' not found", name);
    }

    // 2. Interactive selection or None
    if let Some(index) = smart_select(prompt, items, display_fn)? {
        return Ok(Some(items[index].clone()));
    }

    Ok(None)
}
