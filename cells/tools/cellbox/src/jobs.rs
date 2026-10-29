//! Shell job table — tracks background and foreground jobs.

extern crate alloc;
use alloc::{string::String, vec::Vec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
}

/// One job entry. A pipeline is ONE job: `members` holds one `(stage name,
/// status)` per stage once the pipeline completes, `status` the whole-job status.
struct Job {
    id: usize,
    state: JobState,
    name: String,
    status: Option<i32>,
    members: Vec<(String, i32)>,
}

/// Read-only view of a job for `jobs` listings.
pub struct JobView<'a> {
    pub id: usize,
    pub state: JobState,
    pub name: &'a str,
    pub status: Option<i32>,
    pub members: &'a [(String, i32)],
}

/// Active job registry.
#[derive(Default)]
pub struct Jobs {
    jobs: Vec<Job>,
    next_id: usize,
}

impl Jobs {
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a new running job and return its job ID.
    pub fn add(&mut self, name: &str) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(Job {
            id,
            state: JobState::Running,
            name: String::from(name),
            status: None,
            members: Vec::new(),
        });
        id
    }

    /// Update the state of a job by ID.
    pub fn set_state(&mut self, id: usize, state: JobState) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.state = state;
        }
    }

    /// Mark a job done with its whole-job status and per-stage member statuses.
    pub fn complete(&mut self, id: usize, status: i32, members: Vec<(String, i32)>) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.state = JobState::Done;
            j.status = Some(status);
            j.members = members;
        }
    }

    /// Remove all completed jobs.
    #[cfg(not(feature = "shell_test"))] // reason: reaped by the REPL between prompts
    pub fn reap_done(&mut self) {
        self.jobs.retain(|j| j.state != JobState::Done);
    }

    /// Iterate all jobs with their completion details.
    pub fn list(&self) -> impl Iterator<Item = JobView<'_>> {
        self.jobs.iter().map(|j| JobView {
            id: j.id,
            state: j.state,
            name: j.name.as_str(),
            status: j.status,
            members: &j.members,
        })
    }
}
