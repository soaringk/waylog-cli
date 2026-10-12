use std::io::{self, IsTerminal, Write};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

pub mod init;
pub mod pull;

/// Output handler for user-facing messages
/// Uses Write trait for flexibility and testability
pub struct Output {
    stdout: StandardStream,
    stderr: StandardStream,
    quiet: bool,
    json: bool,
}

impl Output {
    /// Create a new Output instance
    pub fn new(quiet: bool, json: bool) -> Self {
        let color_choice = if std::io::stdout().is_terminal() {
            ColorChoice::Auto
        } else {
            ColorChoice::Never
        };

        Self {
            stdout: StandardStream::stdout(color_choice),
            stderr: StandardStream::stderr(color_choice),
            quiet,
            json,
        }
    }

    /// Print an error message (red, always shown)
    pub fn error(&mut self, msg: impl AsRef<str>) -> io::Result<()> {
        if self.json {
            self.print_json("error", msg.as_ref())?;
        } else {
            self.stderr
                .set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
            writeln!(self.stderr, "✗ {}", msg.as_ref())?;
            self.stderr.reset()?;
        }
        Ok(())
    }

    fn print_json(&mut self, level: &str, message: &str) -> io::Result<()> {
        let json = serde_json::json!({
            "level": level,
            "message": message,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        writeln!(self.stdout, "{}", json)?;
        Ok(())
    }

    pub(crate) fn stdout(&mut self) -> &mut StandardStream {
        &mut self.stdout
    }

    pub(crate) fn stderr(&mut self) -> &mut StandardStream {
        &mut self.stderr
    }

    pub(crate) fn quiet(&self) -> bool {
        self.quiet
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}
