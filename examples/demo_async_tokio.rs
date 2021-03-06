use delay_timer::prelude::*;

use anyhow::Result;

use smol::Timer;
use std::time::Duration;

// You can replace the 66 line with the command you expect to execute.
#[tokio::main]
async fn main() -> Result<()> {
    // In addition to the mixed (smol & tokio) runtime
    // You can also share a tokio runtime with delayTimer, please see api `DelayTimerBuilder::tokio_runtime` for details.

    // Build an DelayTimer that uses the default configuration of the Smol runtime internally.
    let delay_timer = DelayTimerBuilder::default().build();

    // Develop a print job that runs in an asynchronous cycle.
    let task_instance_chain = delay_timer.insert_task(build_task_async_print()?)?;

    // Develop a php script shell-task that runs in an asynchronous cycle.
    let shell_task_instance_chain = delay_timer.insert_task(build_task_async_execute_process()?)?;

    // Get the running instance of task 1.
    let task_instance = task_instance_chain.next_with_async_wait().await?;

    // Cancel running task instances.
    task_instance.cancel_with_async_wait().await?;

    // Cancel running shell-task instances.
    // Probably already finished running, no need to cancel.
    let _ = shell_task_instance_chain
        .next_with_async_wait()
        .await?
        .cancel_with_async_wait()
        .await?;

    // Remove task which id is 1.
    delay_timer.remove_task(1)?;

    // No new tasks are accepted; running tasks are not affected.
    Ok(delay_timer.stop_delay_timer()?)
}

fn build_task_async_print() -> Result<Task, TaskError> {
    let mut task_builder = TaskBuilder::default();

    let body = create_async_fn_body!({
        println!("create_async_fn_body!");

        Timer::after(Duration::from_secs(3)).await;

        println!("create_async_fn_body:i'success");
    });

    task_builder
        .set_task_id(1)
        .set_frequency(Frequency::Repeated("*/6 * * * * * *"))
        .set_maximum_parallel_runnable_num(2)
        .spawn(body)
}

fn build_task_async_execute_process() -> Result<Task, TaskError> {
    let mut task_builder = TaskBuilder::default();

    let body = unblock_process_task_fn("php /home/open/project/rust/repo/myself/delay_timer/examples/try_spawn.php >> ./try_spawn.txt".into());
    task_builder
        .set_frequency_by_candy(CandyFrequency::Repeated(CandyCron::Secondly))
        .set_task_id(3)
        .set_maximum_running_time(10)
        .set_maximum_parallel_runnable_num(1)
        .spawn(body)
}
