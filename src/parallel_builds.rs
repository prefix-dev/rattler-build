//! Runs queued builds, and the work that follows them (tests, reindexing the
//! output channel), with up to `--parallel-builds` jobs in flight.
//!
//! With the default of one job this reproduces the serial order exactly: a
//! build, then its tests or reindex, then the next build. With more, builds
//! whose dependencies are available run side by side. If requested, a shared
//! jobserver (see [`crate::jobserver`]) bounds the total number of compile
//! jobs.
//!
//! Each build runs on its own tokio task, so a busy build never delays reading
//! another build's script output. The other jobs, and the handles of the build
//! tasks, are futures polled by the calling task. The build loop must never
//! await anything else that could wait on a lock held by one of these jobs
//! (for example the output channel lock), or both would wait forever; that is
//! why tests and reindexing run as jobs too.

use std::{future::Future, path::PathBuf, sync::Arc};

use futures::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use miette::IntoDiagnostic;
use rattler_build_core::{
    build::{WorkingDirectoryBehavior, run_build},
    metadata::Output,
    tool_configuration::{Configuration, JobserverEnv},
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
    /// The configuration of the builds, with the jobserver's `MAKEFLAGS` if
    /// one is active.
    configuration: Arc<Configuration>,
    max_jobs: usize,
    /// Each job, with whether it holds a jobserver slot (builds and tests).
    jobs: FuturesUnordered<LocalBoxFuture<'a, (Finished, bool)>>,
    /// Running builds and test runs. Each holds one implicit jobserver token.
    running_builds: usize,
    jobserver: Option<Jobserver>,
}

impl<'a> ParallelBuilds<'a> {
    pub(crate) fn new(configuration: &Configuration) -> Self {
        let parallel_builds = configuration.parallel_builds;
        let jobserver = parallel_builds
            .jobserver_jobs
            .and_then(|jobs| Jobserver::new(jobs.get()));
        let mut configuration = configuration.clone();
        if let Some(jobserver) = &jobserver {
            let makeflags = jobserver.makeflags();
            // Recipes that pass `-j${CPU_COUNT}` bypass the jobserver; give
            // each build its share of the job slots, unless the user set
            // CPU_COUNT (which build scripts receive as before).
            let cpu_count = std::env::var_os("CPU_COUNT").is_none().then(|| {
                cpu_count_share(
                    parallel_builds.jobserver_jobs.map_or(1, |jobs| jobs.get()),
                    parallel_builds.max_builds.get(),
                )
            });
            tracing::info!(
                "Using a jobserver: MAKEFLAGS='{makeflags}'{}",
                cpu_count.map_or(String::new(), |n| format!(", CPU_COUNT={n}"))
            );
            configuration.jobserver = Some(JobserverEnv {
                makeflags,
                cpu_count,
            });
        }
        Self {
            configuration: Arc::new(configuration),
            max_jobs: parallel_builds.max_builds.get(),
            jobs: FuturesUnordered::new(),
            running_builds: 0,
            jobserver,
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
            self.hold_slot();
            let configuration = self.configuration.clone();
            let build = tokio::spawn(async move {
                let result = run_build(
                    output.clone(),
                    &configuration,
                    WorkingDirectoryBehavior::Cleanup,
                )
                .await;
                Finished::Build {
                    output: Box::new(output),
                    result: Box::new(result),
                }
            });
            self.jobs.push(
                async move {
                    match build.await {
                        Ok(finished) => (finished, true),
                        Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
                        Err(e) => panic!("build task was cancelled: {e}"),
                    }
                }
                .boxed_local(),
            );
        }
        Ok(())
    }

    /// Runs `task` as a job; it counts against the maximum like a build.
    pub(crate) fn spawn(&mut self, task: impl Future<Output = miette::Result<()>> + 'a) {
        self.jobs.push(
            task.map(|result| (Finished::Task(result), false))
                .boxed_local(),
        );
    }

    /// Runs tests as a job. Like a build, it holds a jobserver slot while it
    /// runs, since tests can compile too.
    pub(crate) fn spawn_tests(&mut self, task: impl Future<Output = miette::Result<()>> + 'a) {
        self.hold_slot();
        self.jobs.push(
            task.map(|result| (Finished::Task(result), true))
                .boxed_local(),
        );
    }

    /// The configuration of the builds; tests that run as jobs should use it
    /// too, so they see the jobserver.
    pub(crate) fn configuration(&self) -> Arc<Configuration> {
        self.configuration.clone()
    }

    /// Waits for the next job to finish; `None` when no job is in flight.
    pub(crate) async fn next(&mut self) -> Option<Finished> {
        let (finished, held_slot) = self.jobs.next().await?;
        if held_slot {
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

    fn hold_slot(&mut self) {
        self.running_builds += 1;
        self.resize_jobserver();
    }

    fn resize_jobserver(&mut self) {
        if let Some(jobserver) = &mut self.jobserver {
            jobserver.set_running_builds(self.running_builds);
        }
    }
}

/// Each build's share of `jobs` job slots when `builds` run at the same time.
fn cpu_count_share(jobs: usize, builds: usize) -> usize {
    (jobs / builds.max(1)).max(1)
}

#[cfg(test)]
mod tests {
    use super::cpu_count_share;

    #[test]
    fn cpu_count_is_the_share_per_build_and_at_least_one() {
        assert_eq!(cpu_count_share(20, 4), 5);
        assert_eq!(cpu_count_share(20, 6), 3);
        assert_eq!(cpu_count_share(20, 1), 20);
        assert_eq!(cpu_count_share(2, 4), 1);
    }
}
