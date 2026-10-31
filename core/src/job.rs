use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};
use crate::model::{
    Event, GameStatus, JobKind, JobProgress, JobSnapshot, JobStatus, ProtocolErrorData,
};
use crate::store::Store;

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<JobInner>,
}

struct JobInner {
    jobs: Mutex<HashMap<Uuid, JobControl>>,
    store: Arc<Store>,
    events: broadcast::Sender<Event>,
    sequence: AtomicU64,
    semaphore: Semaphore,
}

struct JobControl {
    snapshot: JobSnapshot,
    cancellation: CancellationToken,
}

#[derive(Clone)]
pub struct JobContext {
    id: Uuid,
    inner: Arc<JobInner>,
    cancellation: CancellationToken,
}

impl JobContext {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn check_cancelled(&self) -> CoreResult<()> {
        if self.cancellation.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    pub fn progress(&self, progress: JobProgress) -> CoreResult<()> {
        self.check_cancelled()?;
        self.inner.update(self.id, |job| {
            job.progress = progress;
        })
    }

    pub fn download_progress(&self, progress: JobProgress) -> CoreResult<()> {
        self.check_cancelled()?;
        self.inner.update(self.id, |job| {
            job.progress.stage = progress.stage;
            job.progress.completed = progress.completed;
            job.progress.total = progress.total;
            job.progress.current_item = progress.current_item;
        })
    }

    pub fn checkpoint(&self, checkpoint: Value) -> CoreResult<()> {
        self.inner.update(self.id, |job| {
            job.checkpoint = Some(checkpoint);
        })
    }

    pub fn update_checkpoint(
        &self,
        update: impl FnOnce(&mut serde_json::Map<String, Value>),
    ) -> CoreResult<()> {
        self.inner.update(self.id, |job| {
            let checkpoint = job
                .checkpoint
                .get_or_insert_with(|| Value::Object(Default::default()))
                .as_object_mut()
                .expect("job checkpoint must be an object");
            update(checkpoint);
        })
    }
}

impl JobInner {
    fn update(&self, id: Uuid, update: impl FnOnce(&mut JobSnapshot)) -> CoreResult<()> {
        let snapshot = {
            let mut jobs = self.jobs.lock();
            let control = jobs
                .get_mut(&id)
                .ok_or_else(|| CoreError::NotFound(format!("job {id}")))?;
            update(&mut control.snapshot);
            control.snapshot.clone()
        };
        self.store.save_job(&snapshot)?;
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.events.send(Event::Job {
            sequence,
            job: Box::new(snapshot),
        });
        Ok(())
    }
}

impl JobManager {
    pub fn new(store: Arc<Store>, max_concurrent: usize) -> CoreResult<Self> {
        store.recover_jobs()?;
        let jobs = store
            .jobs()?
            .into_iter()
            .map(|snapshot| {
                (
                    snapshot.id,
                    JobControl {
                        snapshot,
                        cancellation: CancellationToken::new(),
                    },
                )
            })
            .collect();
        Ok(Self {
            inner: Arc::new(JobInner {
                jobs: Mutex::new(jobs),
                store,
                events: broadcast::channel(1024).0,
                sequence: AtomicU64::new(0),
                semaphore: Semaphore::new(max_concurrent),
            }),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn submit<F, Fut>(
        &self,
        kind: JobKind,
        target: Option<Uuid>,
        idempotency_key: Option<String>,
        operation: F,
    ) -> CoreResult<JobSnapshot>
    where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = CoreResult<Value>> + Send + 'static,
    {
        if let Some(key) = idempotency_key.as_deref() {
            if key.is_empty() || key.len() > 200 {
                return Err(CoreError::InvalidInput(
                    "idempotency key must contain 1 to 200 characters".into(),
                ));
            }
            if let Some(existing) = self.inner.store.job_by_idempotency_key(key)? {
                return Ok(existing);
            }
        }
        let snapshot = JobSnapshot {
            id: Uuid::new_v4(),
            kind,
            status: JobStatus::Queued,
            target,
            idempotency_key,
            progress: JobProgress {
                stage: "queued".into(),
                ..JobProgress::default()
            },
            result: None,
            error: None,
            checkpoint: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
        };
        self.inner.store.save_job(&snapshot)?;
        let cancellation = CancellationToken::new();
        self.inner.jobs.lock().insert(
            snapshot.id,
            JobControl {
                snapshot: snapshot.clone(),
                cancellation: cancellation.clone(),
            },
        );
        let manager = self.clone();
        let context = JobContext {
            id: snapshot.id,
            inner: self.inner.clone(),
            cancellation,
        };
        tokio::spawn(async move {
            manager.run(context, operation).await;
        });
        Ok(snapshot)
    }

    async fn run<F, Fut>(&self, context: JobContext, operation: F)
    where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = CoreResult<Value>> + Send + 'static,
    {
        let permit = self.inner.semaphore.acquire().await;
        if permit.is_err() {
            return;
        }
        let _permit = permit.unwrap();
        let result = match context.check_cancelled() {
            Ok(()) => {
                let _ = self.inner.update(context.id, |job| {
                    job.status = JobStatus::Running;
                    job.started_at = Some(Utc::now());
                    job.progress.stage = "running".into();
                });
                operation(context.clone()).await
            }
            Err(error) => Err(error),
        };
        let _ = self.inner.update(context.id, |job| {
            job.finished_at = Some(Utc::now());
            match &result {
                Ok(value) => {
                    job.status = JobStatus::Succeeded;
                    job.progress.stage = "succeeded".into();
                    job.result = Some(value.clone());
                }
                Err(error) => {
                    job.status = if error.is_cancelled() {
                        JobStatus::Cancelled
                    } else {
                        JobStatus::Failed
                    };
                    job.progress.stage = if error.is_cancelled() {
                        "cancelled".into()
                    } else {
                        "failed".into()
                    };
                    job.error = Some(ProtocolErrorData {
                        code: error.code().into(),
                        message: error.to_string(),
                        message_key: error.message_key().into(),
                        retryable: error.retryable(),
                        cancelled: error.is_cancelled(),
                    });
                }
            }
        });
    }

    pub fn get(&self, id: Uuid) -> CoreResult<JobSnapshot> {
        self.inner
            .jobs
            .lock()
            .get(&id)
            .map(|control| control.snapshot.clone())
            .ok_or_else(|| CoreError::NotFound(format!("job {id}")))
    }

    pub fn list(&self) -> Vec<JobSnapshot> {
        let mut jobs = self
            .inner
            .jobs
            .lock()
            .values()
            .map(|control| control.snapshot.clone())
            .collect::<Vec<_>>();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at));
        jobs
    }

    pub fn emit_game(&self, instance_id: Uuid, status: GameStatus) {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.inner.events.send(Event::Game {
            sequence,
            instance_id,
            status,
        });
    }

    pub fn emit_log(&self, instance_id: Uuid, line: String) {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.inner.events.send(Event::Log {
            sequence,
            instance_id,
            line,
        });
    }

    pub async fn cancel_for_instance(&self, instance_id: Uuid) -> CoreResult<()> {
        let mut events = self.inner.events.subscribe();
        loop {
            let jobs = self.active_instance_jobs(instance_id);
            if jobs.is_empty() {
                return Ok(());
            }
            for id in jobs {
                self.cancel(id)?;
            }
            if self.active_instance_jobs(instance_id).is_empty() {
                return Ok(());
            }
            let _ = events.recv().await;
        }
    }

    fn active_instance_jobs(&self, instance_id: Uuid) -> Vec<Uuid> {
        let instance_id = instance_id.to_string();
        self.inner
            .jobs
            .lock()
            .values()
            .filter(|control| {
                let job = &control.snapshot;
                !job.status.is_terminal()
                    && (job.target.is_some_and(|id| id.to_string() == instance_id)
                        || job
                            .checkpoint
                            .as_ref()
                            .and_then(|checkpoint| checkpoint.get("instanceId"))
                            .and_then(Value::as_str)
                            == Some(instance_id.as_str()))
            })
            .map(|control| control.snapshot.id)
            .collect()
    }

    pub fn cancel(&self, id: Uuid) -> CoreResult<JobSnapshot> {
        let control = self.inner.jobs.lock().get(&id).map(|control| {
            control.cancellation.cancel();
            control.snapshot.clone()
        });
        if control.is_none() {
            return Err(CoreError::NotFound(format!("job {id}")));
        }
        self.inner.update(id, |job| {
            if !job.status.is_terminal() {
                job.status = JobStatus::Cancelling;
            }
        })?;
        self.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_for_instance_waits_for_job_to_finish() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::paths::AppPaths::new(temp.path().join("data"));
        paths.initialize().unwrap();
        let store = Arc::new(Store::open(&paths).unwrap());
        let manager = JobManager::new(store, 1).unwrap();
        let instance_id = Uuid::new_v4();
        let job = manager
            .submit(
                JobKind::Install,
                Some(instance_id),
                None,
                |context| async move {
                    context.cancelled().await;
                    Err(CoreError::Cancelled)
                },
            )
            .unwrap();
        manager.cancel_for_instance(instance_id).await.unwrap();
        assert_eq!(manager.get(job.id).unwrap().status, JobStatus::Cancelled);
    }
}
