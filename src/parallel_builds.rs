//! Runs queued builds, and the work that follows them (tests, reindexing the
//! output channel), with up to `--max-parallel-builds` jobs in flight.
//!
//! With the default of one job this reproduces the serial order exactly: a
//! build, then its tests or reindex, then the next build. With more, builds
//! whose dependencies are available run side by side; a shared jobserver
//! (see [`crate::jobserver`]) bounds the total number of compile jobs.
//!
//! All jobs are futures polled by the calling task. The build loop must never
//! await anything else that could wait on a lock held by one of these jobs
//! (for example the output channel lock), or both would wait forever; that is
//! why tests and reindexing run as jobs too.

use std::{future::Future, path::PathBuf};

use futures::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use miette::IntoDiagnostic;
use rattler_build_core::{
    build::{WorkingDirectoryBehavior, run_build},
    metadata::Output,
    tool_configuration::Configuration,
};

use crate::{build_queue::OutputBuildQueue, jobserver::Jobserver};

/// A finished job.
pub(crate) enum Finished {
    /// A build of `output`, with its result (boxed: an `Output` is large).
    Build {
        output: Box<Output>,
        result: Box<miette::Result<(Output, PathBuf)>>,
    },
    /// Other work started with [`ParallelBuilds::spawn`].
    Task(miette::Result<()>),
}

/// The jobs in flight.
pub(crate) struct ParallelBuilds<'a> {
    configuration: &'a Configuration,
    max_jobs: usize,
    jobs: FuturesUnordered<LocalBoxFuture<'a, Finished>>,
    running_builds: usize,
    jobserver: Option<Jobserver>,
}

impl<'a> ParallelBuilds<'a> {
    pub(crate) fn new(configuration: &'a Configuration) -> Self {
        let max_jobs = configuration.max_parallel_builds.max(1);
        let jobserver = (max_jobs > 1).then(|| {
            Jobserver::new(
                configuration
                    .parallel_build_jobs
                    .unwrap_or_else(num_cpus::get)
                    .max(1),
            )
        });
        Self {
            configuration,
            max_jobs,
            jobs: FuturesUnordered::new(),
            running_builds: 0,
            jobserver: jobserver.flatten(),
        }
    }

    /// Starts the builds of ready outputs while fewer than the maximum number
    /// of jobs are in flight.
    pub(crate) fn start_ready(&mut self, queue: &mut OutputBuildQueue) -> miette::Result<()> {
        while self.jobs.len() < self.max_jobs {
            let Some(output) = queue
                .next_ready_parallel(self.jobs.len())
                .into_diagnostic()?
            else {
                break;
            };
            self.running_builds += 1;
            self.resize_jobserver();
            let configuration = self.configuration;
            self.jobs.push(
                async move {
                    let result = run_build(
                        output.clone(),
                        configuration,
                        WorkingDirectoryBehavior::Cleanup,
                    )
                    .await;
                    Finished::Build {
                        output: Box::new(output),
                        result: Box::new(result),
                    }
                }
                .boxed_local(),
            );
        }
        Ok(())
    }

    /// Runs `task` as a job; it counts against the maximum like a build.
    pub(crate) fn spawn(&mut self, task: impl Future<Output = miette::Result<()>> + 'a) {
        self.jobs.push(task.map(Finished::Task).boxed_local());
    }

    /// Waits for the next job to finish; `None` when no job is in flight.
    pub(crate) async fn next(&mut self) -> Option<Finished> {
        let finished = self.jobs.next().await?;
        if matches!(finished, Finished::Build { .. }) {
            self.running_builds -= 1;
            self.resize_jobserver();
        }
        Some(finished)
    }

    /// Waits for every job in flight to finish, discarding the results. Used
    /// before returning an error, so that no build keeps running after
    /// rattler-build exits (like make's "waiting for unfinished jobs").
    pub(crate) async fn finish_running(&mut self) {
        if !self.jobs.is_empty() {
            tracing::warn!(
                "Waiting for {} running job(s) to finish before stopping",
                self.jobs.len()
            );
        }
        while self.next().await.is_some() {}
    }

    fn resize_jobserver(&mut self) {
        if let Some(jobserver) = &mut self.jobserver {
            jobserver.set_running_builds(self.running_builds);
        }
    }
}
