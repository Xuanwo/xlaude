use anyhow::Result;
use clap::Args;
use std::io::{self, IsTerminal, Read};

#[derive(Args, Debug, Clone)]
pub struct OpenOptions {
    /// Prompt to send to Claude and execute after it starts (reads from stdin if no value provided, or appends stdin to provided text)
    #[arg(short = 't', long, value_name = "TEXT")]
    pub type_text: Option<Option<String>>,
}

impl OpenOptions {
    /// Get the text to type, either from CLI argument or stdin
    pub fn get_type_text(&self) -> Result<Option<String>> {
        match &self.type_text {
            Some(Some(text)) => {
                // --type-text "some text" was provided
                // Check if there's also piped input to append
                if !io::stdin().is_terminal() {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)?;

                    if buffer.trim().is_empty() {
                        Ok(Some(text.clone()))
                    } else {
                        Ok(Some(format!("{}\n{}", text, buffer.trim_end())))
                    }
                } else {
                    Ok(Some(text.clone()))
                }
            }
            Some(None) => {
                // --type-text was provided without value, read from stdin
                let mut buffer = String::new();
                io::stdin().read_to_string(&mut buffer)?;

                if buffer.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(buffer.trim_end().to_string()))
                }
            }
            None => {
                // --type-text was not provided
                Ok(None)
            }
        }
    }
}
