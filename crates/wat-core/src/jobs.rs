//! Job table for interactive job control (Tier 3).
//!
//! Each foreground PTY child that is stopped via Ctrl-Z, or launched in
//! the background with `&`, gets an entry here. The REPL reads the table
//! to print notifications and to hand jobs off to `fg`/`bg`.
//!
//! Gated on `native-pty` — the WASM build never sees this code.

use crate::pty::PtyChild;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Done(i32),
}

/// The PTY handles for a job that is still alive and managed by wat-cli.
pub struct JobPty {
    pub child: Box<dyn PtyChild>,
    pub reader: Option<Box<dyn std::io::Read + Send>>,
    pub writer: Option<Box<dyn std::io::Write + Send>>,
}

pub struct Job {
    pub id: u32,
    pub pgid: i32,
    pub cmd: String,
    pub state: JobState,
    pub pty: Option<JobPty>,
}

pub struct JobTable {
    jobs: Vec<Job>,
}

impl JobTable {
    pub fn new() -> Self {
        Self { jobs: Vec::new() }
    }

    /// Add a new job; returns the assigned job id.
    pub fn add(&mut self, cmd: String, pgid: i32, pty: JobPty) -> u32 {
        // Find the smallest positive integer not currently in use.
        let mut id = 1u32;
        loop {
            if !self.jobs.iter().any(|j| j.id == id) {
                break;
            }
            id += 1;
        }
        self.jobs.push(Job {
            id,
            pgid,
            cmd,
            state: JobState::Stopped,
            pty: Some(pty),
        });
        id
    }

    pub fn get(&self, id: u32) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    /// The id of the most recently added job, if any.
    pub fn most_recent(&self) -> Option<u32> {
        self.jobs.last().map(|j| j.id)
    }

    pub fn remove(&mut self, id: u32) -> Option<Job> {
        if let Some(pos) = self.jobs.iter().position(|j| j.id == id) {
            Some(self.jobs.remove(pos))
        } else {
            None
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter()
    }
}

impl Default for JobTable {
    fn default() -> Self {
        Self::new()
    }
}
