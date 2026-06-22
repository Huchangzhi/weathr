mod capabilities;

use crate::error::TerminalError;
use capabilities::TerminalCapabilities;
use crossterm::{
    cursor, execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, BufWriter, IsTerminal, Stdout, Write};
use unicode_width::UnicodeWidthChar;

const MIN_TERMINAL_WIDTH: u16 = 70;
const MIN_TERMINAL_HEIGHT: u16 = 20;

const MAX_TERMINAL_WIDTH: u16 = 1000;
const MAX_TERMINAL_HEIGHT: u16 = 500;

fn clamp_terminal_size(width: u16, height: u16) -> (u16, u16) {
    (
        width.min(MAX_TERMINAL_WIDTH),
        height.min(MAX_TERMINAL_HEIGHT),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    character: char,
    color: Color,
    spacer: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            character: ' ',
            color: Color::Reset,
            spacer: false,
        }
    }
}

pub struct TerminalRenderer {
    stdout: BufWriter<Stdout>,
    width: u16,
    height: u16,
    buffer: Vec<Cell>,
    last_buffer: Vec<Cell>,
    capabilities: TerminalCapabilities,
}

impl TerminalRenderer {
    pub fn new() -> Result<Self, TerminalError> {
        if !io::stdout().is_terminal() {
            return Err(TerminalError::NotATty);
        }

        let (width, height) = terminal::size().map_err(TerminalError::SizeError)?;

        if width < MIN_TERMINAL_WIDTH || height < MIN_TERMINAL_HEIGHT {
            return Err(TerminalError::TooSmall {
                width,
                height,
                min_width: MIN_TERMINAL_WIDTH,
                min_height: MIN_TERMINAL_HEIGHT,
            });
        }

        let (width, height) = clamp_terminal_size(width, height);

        let stdout = BufWriter::new(io::stdout());
        let buffer_size = (width as usize) * (height as usize);
        let capabilities = TerminalCapabilities::detect();

        Ok(Self {
            stdout,
            width,
            height,
            buffer: vec![Cell::default(); buffer_size],
            last_buffer: vec![Cell::default(); buffer_size],
            capabilities,
        })
    }

    pub fn init(&mut self) -> Result<(), TerminalError> {
        terminal::enable_raw_mode().map_err(TerminalError::RawModeError)?;
        execute!(self.stdout, EnterAlternateScreen, cursor::Hide)
            .map_err(TerminalError::InitError)?;
        Ok(())
    }

    pub fn cleanup(&mut self) -> io::Result<()> {
        execute!(self.stdout, LeaveAlternateScreen, cursor::Show, ResetColor)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    pub fn manual_resize(&mut self, width: u16, height: u16) -> io::Result<()> {
        let (width, height) = clamp_terminal_size(width, height);
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            let buffer_size = (width as usize) * (height as usize);
            self.buffer = vec![Cell::default(); buffer_size];
            self.last_buffer = vec![Cell::default(); buffer_size];
            execute!(self.stdout, Clear(ClearType::All))?;
        }
        Ok(())
    }

    pub fn get_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.buffer.fill(Cell::default());
        Ok(())
    }

    pub fn render_centered_colored(
        &mut self,
        lines: &[String],
        start_row: u16,
        color: Color,
    ) -> io::Result<()> {
        let max_width = lines
            .iter()
            .map(|l| l.chars().map(|c| c.width().unwrap_or(1)).sum::<usize>())
            .max()
            .unwrap_or(0);
        let adjusted_color = self.capabilities.adjust_color(color);

        for (idx, line) in lines.iter().enumerate() {
            let row = start_row + idx as u16;
            if row < self.height {
                let start_col = if self.width as usize > max_width {
                    (self.width as usize - max_width) / 2
                } else {
                    0
                };
                let mut col = start_col;
                for ch in line.chars() {
                    if col < self.width as usize {
                        let buffer_idx = (row as usize) * (self.width as usize) + col;
                        if buffer_idx < self.buffer.len() {
                            self.buffer[buffer_idx] = Cell {
                                character: ch,
                                color: adjusted_color,
                                spacer: false,
                            };
                        }
                        let w = ch.width().unwrap_or(1);
                        if w > 1 && col + 1 < self.width as usize {
                            let spacer_idx = (row as usize) * (self.width as usize) + col + 1;
                            if spacer_idx < self.buffer.len() {
                                self.buffer[spacer_idx] = Cell {
                                    character: ' ',
                                    color: adjusted_color,
                                    spacer: true,
                                };
                            }
                        }
                    }
                    col += ch.width().unwrap_or(1);
                }
            }
        }

        Ok(())
    }

    pub fn render_line_colored(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        color: Color,
    ) -> io::Result<()> {
        if y >= self.height {
            return Ok(());
        }
        let adjusted_color = self.capabilities.adjust_color(color);

        let mut col = x as usize;
        for ch in text.chars() {
            let w = ch.width().unwrap_or(1);
            if col < self.width as usize {
                let buffer_idx = (y as usize) * (self.width as usize) + col;
                if buffer_idx < self.buffer.len() {
                    self.buffer[buffer_idx] = Cell {
                        character: ch,
                        color: adjusted_color,
                        spacer: false,
                    };
                }
                if w > 1 && col + 1 < self.width as usize {
                    let spacer_idx = (y as usize) * (self.width as usize) + col + 1;
                    if spacer_idx < self.buffer.len() {
                        self.buffer[spacer_idx] = Cell {
                            character: ' ',
                            color: adjusted_color,
                            spacer: true,
                        };
                    }
                }
            }
            col += w;
        }
        Ok(())
    }

    pub fn render_char(&mut self, x: u16, y: u16, ch: char, color: Color) -> io::Result<()> {
        if x < self.width && y < self.height {
            let buffer_idx = (y as usize) * (self.width as usize) + (x as usize);
            if buffer_idx < self.buffer.len() {
                self.buffer[buffer_idx] = Cell {
                    character: ch,
                    color: self.capabilities.adjust_color(color),
                    spacer: false,
                };
            }
        }
        Ok(())
    }

    pub fn flash_screen(&mut self) -> io::Result<()> {
        let flash_color = self.capabilities.adjust_color(Color::White);
        for cell in &mut self.buffer {
            cell.color = flash_color;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let mut current_color = Color::Reset;
        let mut last_pos: Option<(u16, u16)> = None;

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y as usize) * (self.width as usize) + (x as usize);

                if idx >= self.buffer.len() || idx >= self.last_buffer.len() {
                    continue;
                }

                let cell = self.buffer[idx];
                let last_cell = self.last_buffer[idx];

                if cell.spacer {
                    continue;
                }

                if cell != last_cell {
                    let expected_pos = last_pos.map(|(lx, ly)| (lx + 1, ly));
                    if expected_pos != Some((x, y)) {
                        queue!(self.stdout, cursor::MoveTo(x, y))?;
                    }

                    if cell.color != current_color {
                        queue!(self.stdout, SetForegroundColor(cell.color))?;
                        current_color = cell.color;
                    }

                    queue!(self.stdout, Print(cell.character))?;
                    last_pos = Some((x, y));
                }
            }
        }

        if current_color != Color::Reset {
            queue!(self.stdout, ResetColor)?;
        }

        self.stdout.flush()?;
        self.last_buffer.copy_from_slice(&self.buffer);
        Ok(())
    }
}

impl Drop for TerminalRenderer {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
