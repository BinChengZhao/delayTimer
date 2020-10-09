use super::event_handle::{
     SharedHeader,
};
pub(crate) use super::runtime_trace::task_handle::DelayTaskHandlerBox;
use super::runtime_trace::task_handle::DelayTaskHandlerBoxBuilder;
pub(crate) use super::slot::Slot;
pub(crate) use super::task::Task;
pub use crate::delay_timer::get_timestamp;
pub(crate) use smol::channel::{Receiver as AsyncReceiver, Sender as AsyncSender};
use snowflake::SnowflakeIdBucket;

pub(crate) use super::task::TaskMark;
use smol::Timer as SmolTimer;
use std::sync::{
    atomic::{
        
        Ordering::{Relaxed, Release},
    },
};
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_TIMER_SLOT_COUNT: u64 = 3600;

pub(crate) type TimerEventSender = AsyncSender<TimerEvent>;
pub(crate) type TimerEventReceiver = AsyncReceiver<TimerEvent>;
//warning: large size difference between variants
pub(crate) enum TimerEvent {
    StopTimer,
    AddTask(Box<Task>),
    RemoveTask(u64),
    CancelTask(u64, i64),
    StopTask(u64),
    AppendTaskHandle(u64, DelayTaskHandlerBox),
}

pub(crate) struct Timer {
    timer_event_sender: TimerEventSender,
    //TODO:status_report_sender.
    status_report_sender: Option<AsyncSender<i32>>,
    shared_header: SharedHeader,
}

//In any case, the task is not executed in the scheduler,
//and task-Fn determines which runtime to put the internal task in when it is generated.
//just provice api and struct ,less is more.
impl Timer {
    pub(crate) fn new(timer_event_sender: TimerEventSender, shared_header: SharedHeader) -> Self {
        Timer {
            timer_event_sender,
            status_report_sender: None,
            shared_header,
        }
    }

    pub(crate) fn set_status_report_sender(&mut self, sender: AsyncSender<i32>) {
        self.status_report_sender = Some(sender);
    }

    //TODO:features append fn put there.
    pub(crate) fn features_append_fn(&mut self, _sender: AsyncSender<i32>) {
        #[cfg(feature = "status-report")]
        fn report(&mut self, record: i32) {
            // async.sender.send(record);
        }

        #[cfg(feature = "status-report")]
        self.report(1);
    }

    //Offset the current slot by one when reading it,
    //so event_handle can be easily inserted into subsequent slots.
    pub(crate) fn next_position(&mut self) -> u64 {
        self.shared_header
            .second_hand
            .fetch_update(Release, Relaxed, |x| {
                Some((x + 1) % DEFAULT_TIMER_SLOT_COUNT)
            })
            .unwrap_or_else(|e| e)
    }

    pub(crate) async fn async_schedule(&mut self) {
        //not runing 1s ,Duration - runing time
        //sleep  ,then loop
        //if that overtime , i run it not block

        let mut now;
        let mut when;
        let mut second_hand;
        let mut timestamp;

        //TODO:auto-get nodeid and machineid.
        let mut snowflakeid_bucket = SnowflakeIdBucket::new(1, 1);
        loop {
            //TODO: replenish ending single, for stop current jod and thread.

            second_hand = self.next_position();
            now = Instant::now();
            when = now + Duration::from_secs(1);
            timestamp = get_timestamp();
            self.shared_header.global_time.store(timestamp, Release);
            let task_ids;

            {
                let mut slot_mut = self
                    .shared_header
                    .wheel_queue
                    .get_mut(&second_hand)
                    .unwrap();

                task_ids = slot_mut.value_mut().arrival_time_tasks();
            }

            println!("timer-core:Timer-second_hand: {}", second_hand);
            for task_id in task_ids {
                let task_option: Option<Task>;

                {
                    let mut slot_mut = self
                        .shared_header
                        .wheel_queue
                        .get_mut(&second_hand)
                        .unwrap();

                    task_option = slot_mut.value_mut().remove_task(task_id);
                }

                if let Some(task) = task_option {
                    self.maintain_task(task, snowflakeid_bucket.get_id(), timestamp, second_hand)
                        .await;
                }
            }

            SmolTimer::at(when).await;
        }
    }

    #[inline(always)]
    pub(crate) async fn maintain_task(
        &mut self,
        mut task: Task,
        record_id: i64,
        timestamp: u64,
        second_hand: u64,
    ) {
        let task_id = task.task_id;
        let task_handler_box = (task.body)();

        let delay_task_handler_box_builder = DelayTaskHandlerBoxBuilder::default();
        let tmp_task_handler_box = delay_task_handler_box_builder
            .set_task_id(task_id)
            .set_record_id(record_id)
            .set_start_time(timestamp)
            .set_end_time(task.get_maximum_running_time(timestamp))
            .spawn(task_handler_box);

        self.timer_event_sender
            .send(TimerEvent::AppendTaskHandle(task_id, tmp_task_handler_box))
            .await
            .unwrap_or_else(|e| println!("{}", e));

        let task_valid = task.down_count_and_set_vaild();
        if !task_valid {
            return;
        }
        //下一次执行时间
        let task_excute_timestamp = task.get_next_exec_timestamp();

        //时间差+当前的分针
        //比如 时间差是 7260，目前分针再 3599，7260+3599 = 10859
        //， 从 当前 3599 走碰见三次，再第59个格子
        let step = task_excute_timestamp
            .checked_sub(timestamp)
            .unwrap_or_else(|| task.task_id % DEFAULT_TIMER_SLOT_COUNT)
            + second_hand;
        let quan = step / DEFAULT_TIMER_SLOT_COUNT;
        task.set_cylinder_line(quan);
        let slot_seed = step % DEFAULT_TIMER_SLOT_COUNT;

        // println!(
        //     "timer-core:task_id:{}, next_time:{}, slot_seed:{}, quan:{}",
        //     task.task_id, step, slot_seed, quan
        // );

        //FIXME: hidden send error for bench.
        //.unwrap_or_else(|e| println!("{}", e));

        {
            let mut slot_mut = self.shared_header.wheel_queue.get_mut(&slot_seed).unwrap();

            slot_mut.value_mut().add_task(task);
        }

        {
            let mut task_flag_map = self.shared_header.task_flag_map.get_mut(&task_id).unwrap();

            task_flag_map.value_mut().set_slot_mark(slot_seed);
        }
    }
}