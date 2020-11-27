use crate::ui::{self};
use crate::state::{State};
use crate::util::{Result};

use crossterm::terminal::{self};
use crossterm::{ExecutableCommand};

use tui::{Terminal};
use tui::backend::{CrosstermBackend};

use std::io::{self, Stdout};

pub struct Renderer<W: std::io::Write> {
    terminal: Terminal<CrosstermBackend<W>>,
}

impl<W: std::io::Write> Renderer<W> {
    pub fn new(mut out: W) -> Result<Renderer<W>> {
        terminal::enable_raw_mode()?;
        //io::stdout().execute(terminal::EnterAlternateScreen)?;
        out.execute(terminal::EnterAlternateScreen)?;

        Ok(Renderer { terminal: Terminal::new(CrosstermBackend::new(out))? })
    }

    pub fn render(&mut self, state: &State) -> Result<()> {
        self.terminal.draw(|frame| ui::draw(frame, state, frame.size()))?;
        Ok(())
    }
}

impl<W: std::io::Write> Drop for Renderer<W> {
    fn drop(&mut self) {
        //        io::stdout().execute(terminal::LeaveAlternateScreen).expect("Could not execute to stdout");
        terminal::disable_raw_mode().expect("Terminal doesn't support to disable raw mode");
        if std::thread::panicking() {
            eprintln!(
                "{}, example: {}",
                "termchat paniced, to log the error you can redirect stderror to a file",
                "termchat 2> termchat_log"
            );
        }
    }
}
