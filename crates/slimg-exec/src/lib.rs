use std::sync::{Arc, mpsc};
use std::thread;

use slimg_core::Format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    BalancedDesktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadClass {
    SingleThreadDominant,
    InternallyThreaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingLane {
    SingleThread,
    Threaded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec<T> {
    pub input: T,
    pub workload_class: WorkloadClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTask<T> {
    pub input: T,
    pub workload_class: WorkloadClass,
    pub lane: SchedulingLane,
    pub thread_budget: usize,
    pub slot_cost: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan<T> {
    pub mode: ExecutionMode,
    pub usable_cores: usize,
    pub max_concurrent_files: usize,
    pub tasks: Vec<PlannedTask<T>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport<R> {
    pub results: Vec<Option<R>>,
    pub started_count: usize,
    pub canceled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostResources {
    pub available_parallelism: usize,
    pub usable_cores: usize,
}

impl HostResources {
    pub fn current() -> Self {
        let available_parallelism = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        Self::from_available_parallelism(available_parallelism)
    }

    pub fn from_available_parallelism(available_parallelism: usize) -> Self {
        let available_parallelism = available_parallelism.max(1);
        Self {
            usable_cores: available_parallelism.saturating_sub(1).max(1),
            available_parallelism,
        }
    }
}

pub fn classify_format(format: Format) -> WorkloadClass {
    match format {
        Format::Avif | Format::Jxl => WorkloadClass::InternallyThreaded,
        Format::Jpeg | Format::Png | Format::WebP | Format::Qoi => {
            WorkloadClass::SingleThreadDominant
        }
    }
}

pub fn plan_tasks<T>(mode: ExecutionMode, tasks: Vec<TaskSpec<T>>) -> ExecutionPlan<T> {
    let resources = HostResources::current();
    plan_tasks_with_resources(mode, resources, tasks)
}

pub fn plan_tasks_with_resources<T>(
    mode: ExecutionMode,
    resources: HostResources,
    tasks: Vec<TaskSpec<T>>,
) -> ExecutionPlan<T> {
    let usable_cores = resources.usable_cores.max(1);
    let tasks = tasks
        .into_iter()
        .map(|task| {
            let (lane, thread_budget) = match task.workload_class {
                WorkloadClass::SingleThreadDominant => (SchedulingLane::SingleThread, 1),
                WorkloadClass::InternallyThreaded => {
                    (SchedulingLane::Threaded, usable_cores.min(4))
                }
            };
            let slot_cost = match task.workload_class {
                WorkloadClass::SingleThreadDominant => 1,
                WorkloadClass::InternallyThreaded => thread_budget.max(1),
            };
            PlannedTask {
                input: task.input,
                workload_class: task.workload_class,
                lane,
                thread_budget,
                slot_cost,
            }
        })
        .collect();

    ExecutionPlan {
        mode,
        usable_cores,
        max_concurrent_files: usable_cores,
        tasks,
    }
}

pub struct BatchExecutor;

impl BatchExecutor {
    pub fn execute<T, R, Run, IsCancelled, OnStarted, OnFinished>(
        plan: ExecutionPlan<T>,
        is_cancelled: IsCancelled,
        mut on_started: OnStarted,
        mut on_finished: OnFinished,
        run: Run,
    ) -> ExecutionReport<R>
    where
        T: Send + 'static,
        R: Send + 'static,
        Run: Fn(T, usize) -> R + Send + Sync + 'static,
        IsCancelled: Fn() -> bool,
        OnStarted: FnMut(usize),
        OnFinished: FnMut(usize, &R),
    {
        let total = plan.tasks.len();
        let mut results = std::iter::repeat_with(|| None)
            .take(total)
            .collect::<Vec<_>>();
        let mut pending = plan.tasks.into_iter().map(Some).collect::<Vec<_>>();
        let mut pending_count = pending.len();
        let mut started_count = 0usize;
        let mut completed_count = 0usize;
        let mut used_cores = 0usize;
        let mut threaded_running = false;
        let mut canceled = false;

        let (tx, rx) = mpsc::channel::<(usize, WorkloadClass, usize, R)>();
        let run = Arc::new(run);

        while completed_count < started_count || pending_count > 0 {
            if !canceled && is_cancelled() {
                canceled = true;
            }

            if !canceled {
                while let Some(index) = next_runnable_task_index(
                    &pending,
                    plan.usable_cores,
                    used_cores,
                    threaded_running,
                ) {
                    let task = pending[index]
                        .take()
                        .expect("runnable task should still be pending");
                    pending_count -= 1;
                    started_count += 1;
                    used_cores += task.slot_cost;
                    if task.workload_class == WorkloadClass::InternallyThreaded {
                        threaded_running = true;
                    }
                    on_started(index);

                    let tx = tx.clone();
                    let run = Arc::clone(&run);
                    thread::spawn(move || {
                        let result = run(task.input, task.thread_budget);
                        let _ = tx.send((index, task.workload_class, task.slot_cost, result));
                    });
                }
            }

            if completed_count == started_count {
                if pending_count == 0 || canceled {
                    break;
                }
                continue;
            }

            let (index, workload_class, slot_cost, result) = rx
                .recv()
                .expect("worker should send a result before exiting");
            completed_count += 1;
            used_cores = used_cores.saturating_sub(slot_cost);
            if workload_class == WorkloadClass::InternallyThreaded {
                threaded_running = false;
            }
            on_finished(index, &result);
            results[index] = Some(result);
        }

        ExecutionReport {
            results,
            started_count,
            canceled,
        }
    }
}

fn next_runnable_task_index<T>(
    pending: &[Option<PlannedTask<T>>],
    usable_cores: usize,
    used_cores: usize,
    threaded_running: bool,
) -> Option<usize> {
    let remaining_cores = usable_cores.saturating_sub(used_cores);
    pending.iter().position(|task| {
        let Some(task) = task.as_ref() else {
            return false;
        };
        if task.slot_cost > remaining_cores {
            return false;
        }
        !(threaded_running && task.workload_class == WorkloadClass::InternallyThreaded)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[test]
    fn balanced_desktop_plan_assigns_expected_thread_budgets() {
        let plan = plan_tasks_with_resources(
            ExecutionMode::BalancedDesktop,
            HostResources::from_available_parallelism(8),
            vec![
                TaskSpec {
                    input: "jpeg",
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
                TaskSpec {
                    input: "avif",
                    workload_class: WorkloadClass::InternallyThreaded,
                },
            ],
        );

        assert_eq!(plan.usable_cores, 7);
        assert_eq!(plan.tasks[0].thread_budget, 1);
        assert_eq!(plan.tasks[0].lane, SchedulingLane::SingleThread);
        assert_eq!(plan.tasks[1].thread_budget, 4);
        assert_eq!(plan.tasks[1].lane, SchedulingLane::Threaded);
    }

    #[test]
    fn classify_format_marks_avif_and_jxl_as_threaded() {
        assert_eq!(
            classify_format(Format::Avif),
            WorkloadClass::InternallyThreaded
        );
        assert_eq!(
            classify_format(Format::Jxl),
            WorkloadClass::InternallyThreaded
        );
        assert_eq!(
            classify_format(Format::Jpeg),
            WorkloadClass::SingleThreadDominant
        );
    }

    #[test]
    fn batch_executor_preserves_result_order() {
        let plan = plan_tasks_with_resources(
            ExecutionMode::BalancedDesktop,
            HostResources::from_available_parallelism(6),
            vec![
                TaskSpec {
                    input: 0usize,
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
                TaskSpec {
                    input: 1usize,
                    workload_class: WorkloadClass::InternallyThreaded,
                },
                TaskSpec {
                    input: 2usize,
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
            ],
        );

        let report = BatchExecutor::execute(
            plan,
            || false,
            |_| {},
            |_, _| {},
            |input, _threads| {
                if input == 1 {
                    thread::sleep(Duration::from_millis(40));
                }
                input * 10
            },
        );

        let ordered = report
            .results
            .into_iter()
            .map(|item| item.expect("all tasks should complete"))
            .collect::<Vec<_>>();
        assert_eq!(ordered, vec![0, 10, 20]);
    }

    #[test]
    fn batch_executor_allows_single_threaded_tasks_alongside_one_threaded_task() {
        let plan = plan_tasks_with_resources(
            ExecutionMode::BalancedDesktop,
            HostResources::from_available_parallelism(8),
            vec![
                TaskSpec {
                    input: 0usize,
                    workload_class: WorkloadClass::InternallyThreaded,
                },
                TaskSpec {
                    input: 1usize,
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
                TaskSpec {
                    input: 2usize,
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
                TaskSpec {
                    input: 3usize,
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
            ],
        );

        let active_single = Arc::new(AtomicUsize::new(0));
        let saw_parallel_single = Arc::new(AtomicUsize::new(0));

        let active_single_run = Arc::clone(&active_single);
        let saw_parallel_single_run = Arc::clone(&saw_parallel_single);

        let report = BatchExecutor::execute(
            plan,
            || false,
            |_| {},
            |_, _| {},
            move |input, _threads| {
                if input == 0 {
                    thread::sleep(Duration::from_millis(50));
                    return input;
                }

                let active = active_single_run.fetch_add(1, Ordering::SeqCst) + 1;
                if active >= 2 {
                    saw_parallel_single_run.store(1, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(20));
                active_single_run.fetch_sub(1, Ordering::SeqCst);
                input
            },
        );

        assert_eq!(report.started_count, 4);
        assert_eq!(saw_parallel_single.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn batch_executor_stops_scheduling_when_canceled() {
        let cancel_after_first_start = Arc::new(AtomicUsize::new(0));
        let cancel_after_first_start_check = Arc::clone(&cancel_after_first_start);

        let plan = plan_tasks_with_resources(
            ExecutionMode::BalancedDesktop,
            HostResources::from_available_parallelism(4),
            vec![
                TaskSpec {
                    input: 0usize,
                    workload_class: WorkloadClass::InternallyThreaded,
                },
                TaskSpec {
                    input: 1usize,
                    workload_class: WorkloadClass::InternallyThreaded,
                },
                TaskSpec {
                    input: 2usize,
                    workload_class: WorkloadClass::SingleThreadDominant,
                },
            ],
        );

        let report = BatchExecutor::execute(
            plan,
            move || cancel_after_first_start_check.load(Ordering::SeqCst) >= 1,
            |_| {
                cancel_after_first_start.fetch_add(1, Ordering::SeqCst);
            },
            |_, _| {},
            |input, _threads| {
                thread::sleep(Duration::from_millis(30));
                input
            },
        );

        assert!(report.canceled);
        assert_eq!(report.started_count, 1);
        assert_eq!(
            report.results.into_iter().flatten().collect::<Vec<_>>(),
            vec![0]
        );
    }
}
