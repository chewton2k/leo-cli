//! Asking a question on the terminal.

use anyhow::Result;

/// Print a prompt and read one trimmed line.
pub fn ask(prompt: &str) -> Result<String> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().to_string())
}
