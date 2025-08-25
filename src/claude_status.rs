use std::time::{Duration, Instant};

/// Claude session status based on output analysis
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeStatus {
    /// Waiting for human input (shows prompt or cursor)
    WaitingForInput,
    /// Processing/thinking (executing commands or generating response)
    Processing,
    /// Error occurred
    Error,
    /// No activity detected
    Idle,
    /// Session not running
    NotRunning,
}

impl ClaudeStatus {
    pub fn display_text(&self) -> &str {
        match self {
            ClaudeStatus::WaitingForInput => "Waiting",
            ClaudeStatus::Processing => "Processing",
            ClaudeStatus::Error => "Error",
            ClaudeStatus::Idle => "Idle",
            ClaudeStatus::NotRunning => "Not Running",
        }
    }

    pub fn display_icon(&self) -> &str {
        match self {
            ClaudeStatus::WaitingForInput => "⏸",
            ClaudeStatus::Processing => "⚡",
            ClaudeStatus::Error => "⚠",
            ClaudeStatus::Idle => "💤",
            ClaudeStatus::NotRunning => "◌",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            ClaudeStatus::WaitingForInput => Color::Yellow,
            ClaudeStatus::Processing => Color::Green,
            ClaudeStatus::Error => Color::Red,
            ClaudeStatus::Idle => Color::DarkGray,
            ClaudeStatus::NotRunning => Color::DarkGray,
        }
    }
}

pub struct ClaudeStatusDetector {
    last_analysis: Option<Instant>,
    cache: Option<(String, ClaudeStatus)>,
}

impl ClaudeStatusDetector {
    pub fn new() -> Self {
        Self {
            last_analysis: None,
            cache: None,
        }
    }

    /// Analyze Claude output to determine current status
    pub fn analyze_output(&mut self, output: &str) -> ClaudeStatus {
        // Check cache (avoid re-analyzing within 500ms)
        if let Some(ref last) = self.last_analysis
            && last.elapsed() < Duration::from_millis(500)
            && let Some((ref cached_output, ref status)) = self.cache
            && cached_output == output
        {
            return status.clone();
        }

        let status = self.detect_status_from_output(output);

        // Update cache
        self.last_analysis = Some(Instant::now());
        self.cache = Some((output.to_string(), status.clone()));

        status
    }

    fn detect_status_from_output(&self, output: &str) -> ClaudeStatus {
        // Clean the output
        let lines: Vec<&str> = output.lines().collect();

        // Check last few lines for patterns
        let last_lines = lines.iter().rev().take(10).collect::<Vec<_>>();

        // Pattern 1: Waiting for input - cursor or Human: prompt
        if self.is_waiting_for_input(&last_lines, output) {
            return ClaudeStatus::WaitingForInput;
        }

        // Pattern 2: Error messages
        if self.has_error_pattern(&last_lines) {
            return ClaudeStatus::Error;
        }

        // Pattern 3: Processing - tool usage or thinking
        if self.is_processing(&last_lines) {
            return ClaudeStatus::Processing;
        }

        // Pattern 4: Check if truly idle (no recent activity)
        if output.trim().is_empty() || lines.len() < 3 {
            return ClaudeStatus::Idle;
        }

        // Default to waiting if we see typical Claude output
        ClaudeStatus::WaitingForInput
    }

    fn is_waiting_for_input(&self, last_lines: &[&&str], full_output: &str) -> bool {
        // Check for cursor at the end
        if full_output.ends_with("▌") || full_output.ends_with("█") {
            return true;
        }

        // Check for Human: prompt
        for line in last_lines {
            let trimmed = line.trim();
            if trimmed == "Human:" || trimmed.starts_with("Human: ") {
                return true;
            }
            // Check for input prompt patterns
            if trimmed.ends_with(":") && trimmed.len() < 20 {
                return true;
            }
        }

        // Check for common Claude waiting patterns
        if let Some(last) = last_lines.first() {
            let last_trimmed = last.trim();
            // Empty line after Assistant response often means waiting
            if last_trimmed.is_empty()
                && last_lines.len() > 1
                && let Some(second_last) = last_lines.get(1)
                && (second_last.starts_with("Assistant:")
                    || second_last.contains("I'll")
                    || second_last.contains("Let me"))
            {
                return true;
            }
        }

        false
    }

    fn has_error_pattern(&self, last_lines: &[&&str]) -> bool {
        for line in last_lines {
            let lower = line.to_lowercase();
            if lower.contains("error:")
                || lower.contains("failed")
                || lower.contains("exception")
                || lower.contains("traceback")
                || lower.contains("permission denied")
                || lower.contains("not found") && !lower.contains("file not found")
            {
                return true;
            }
        }
        false
    }

    fn is_processing(&self, last_lines: &[&&str]) -> bool {
        for line in last_lines {
            let trimmed = line.trim();

            // Tool execution patterns
            if trimmed.contains("Running")
                || trimmed.contains("Executing")
                || trimmed.contains("Processing")
                || trimmed.contains("Building")
                || trimmed.contains("Compiling")
                || trimmed.contains("Installing")
            {
                return true;
            }

            // Claude thinking patterns
            if trimmed.contains("Thinking...")
                || trimmed.contains("Analyzing")
                || trimmed.contains("Let me")
                || trimmed.contains("I'll")
                || trimmed.contains("Looking at")
                || trimmed.contains("Checking")
            {
                // But make sure it's recent (not followed by completion)
                if last_lines.first().is_some_and(|l| l.trim() == trimmed) {
                    return true;
                }
            }

            // Command output patterns (active output)
            if trimmed.starts_with("+")
                || trimmed.starts_with(">>>")
                || trimmed.starts_with("$") && trimmed.len() > 2
            {
                return true;
            }
        }

        // Check for streaming output (multiple lines of similar content)
        if last_lines.len() >= 3 {
            let non_empty: Vec<_> = last_lines.iter().filter(|l| !l.trim().is_empty()).collect();

            // If we have many lines of output, likely processing
            if non_empty.len() >= 3 {
                // Check if lines look like command output
                let looks_like_output = non_empty.iter().any(|l| {
                    let t = l.trim();
                    t.starts_with("  ") || // Indented output
                    t.contains("...") ||   // Progress indicators
                    t.chars().filter(|c| c.is_ascii_punctuation()).count() > t.len() / 3 // Code/data
                });

                if looks_like_output {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_waiting_for_input() {
        let mut detector = ClaudeStatusDetector::new();

        // Test cursor pattern
        let output = "Assistant: I've completed the task.\n\nHuman: ▌";
        assert_eq!(
            detector.analyze_output(output),
            ClaudeStatus::WaitingForInput
        );

        // Test Human: prompt
        let output = "Some previous output\n\nHuman:";
        assert_eq!(
            detector.analyze_output(output),
            ClaudeStatus::WaitingForInput
        );
    }

    #[test]
    fn test_error_detection() {
        let mut detector = ClaudeStatusDetector::new();

        let output = "Running command...\nError: Command failed\nPlease fix the issue";
        assert_eq!(detector.analyze_output(output), ClaudeStatus::Error);
    }

    #[test]
    fn test_processing_detection() {
        let mut detector = ClaudeStatusDetector::new();

        let output = "Let me analyze this code...\nChecking the files\nProcessing...";
        assert_eq!(detector.analyze_output(output), ClaudeStatus::Processing);
    }
}
