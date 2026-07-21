use super::Output;
use std::io::{self, Write};

fn runnable_agents() -> impl Iterator<Item = &'static str> {
    crate::providers::list_providers()
        .into_iter()
        .filter(|name| {
            crate::providers::get_provider(name)
                .is_ok_and(|provider| provider.run_command().is_some())
        })
}

fn write_runnable_agents(output: &mut Output) -> io::Result<()> {
    for provider in runnable_agents() {
        writeln!(output.stderr(), "- {}", provider)?;
    }
    Ok(())
}

impl Output {
    /// Print missing agent error
    pub fn missing_agent(&mut self) -> io::Result<()> {
        self.error("Missing required argument <AGENT>")?;
        writeln!(self.stderr(), "\nUsage: waylog run <AGENT> [ARGS]...\n")?;
        writeln!(self.stderr(), "Available agents:")?;
        write_runnable_agents(self)?;
        writeln!(self.stderr(), "\nExample:\n  waylog run claude")?;
        Ok(())
    }

    /// Print unknown agent error
    pub fn unknown_agent(&mut self, name: &str) -> io::Result<()> {
        self.error(format!("'{}' is not a recognized agent.", name))?;
        writeln!(self.stderr(), "\nAvailable agents:")?;
        write_runnable_agents(self)?;
        writeln!(self.stderr(), "\nDid you mean to run 'waylog pull'?")?;
        Ok(())
    }

    /// Print agent not installed error
    pub fn agent_not_installed(&mut self, command: &str) -> io::Result<()> {
        self.error(format!("{} is not installed or not in PATH", command))?;
        writeln!(
            self.stderr(),
            "Please install it first before using waylog."
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_only_providers_are_not_advertised_as_runnable() {
        let agents = runnable_agents().collect::<Vec<_>>();

        assert!(agents.contains(&"codex"));
        assert!(!agents.contains(&"qoder"));
        assert!(!agents.contains(&"qoderwork"));
    }
}
