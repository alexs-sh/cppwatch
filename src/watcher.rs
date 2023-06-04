use std::io::{Error, ErrorKind};
use std::mem;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;
use tokio::task::{self, JoinHandle};
use watchexec::action::{Action, Outcome, PreSpawn};
use watchexec::command::Command;
use watchexec::config::{InitConfig, RuntimeConfig};
use watchexec::Watchexec;
use watchexec_events::{Event, ProcessEnd, Tag};

use crate::event::{ExecutorEvent, StepData};
use crate::filters;

#[derive(Clone)]
pub struct Config {
    pub watch_dir: String,
    pub build_dir: String,
    pub build_command: String,
    pub test_command: String,
    pub delay: Option<Duration>,
    pub tx: Sender<ExecutorEvent>,
}

impl Config {
    fn get_build_dir(&self) -> String {
        if self.build_dir.is_empty() {
            self.watch_dir.clone()
        } else {
            // silly check of absolute / relative path
            if self.build_dir.starts_with('/') {
                self.build_dir.clone()
            } else {
                // concat watch dir with relative build dir
                format!("{}/{}", self.watch_dir, self.build_dir)
            }
        }
    }

    fn get_commands(&self) -> Vec<Command> {
        let mut cmds = vec![parse_command(&self.build_command).unwrap()];
        if let Some(test_cmd) = parse_command(&self.test_command) {
            cmds.push(test_cmd);
        }
        cmds
    }
}

struct Context {
    config: Config,
    steps: Vec<StepData>,
    task_num: u64,
    steps_finished: usize,
    steps_limit: usize,
}

impl Context {
    fn new(config: Config) -> Context {
        let steps_limit =
            !config.build_command.is_empty() as usize + !config.test_command.is_empty() as usize;

        Context {
            config,
            steps: Vec::new(),
            steps_finished: 0,
            task_num: 0,
            steps_limit,
        }
    }

    fn get_step_name(&self) -> String {
        match self.steps.len() {
            0 => "Build",
            1 => "Test",
            _ => "Unknown",
        }
        .to_owned()
    }

    fn start_step(&mut self) {
        if self.steps.is_empty() {
            self.task_num += 1;
        }
        let now = Instant::now();
        let step = StepData {
            status: false,
            start_at: now,
            stop_at: now,
            name: self.get_step_name(),
        };
        self.steps.push(step);
    }

    fn finish_step(&mut self, status: bool) {
        if self.steps.is_empty() {
            return;
        }

        if let Some(data) = self.steps.get_mut(self.steps_finished) {
            data.stop_at = Instant::now();
            data.status = status;
        };

        self.steps_finished += 1;
        if status {
            self.on_success();
        } else {
            self.on_fail();
        }
    }

    fn on_success(&mut self) {
        if self.steps_finished == self.steps_limit {
            let id = self.task_num;
            let payload = self.take_steps();
            let message = ExecutorEvent::Success((id, payload));
            self.config.tx.try_send(message).unwrap();
            self.reset();
        }
    }

    fn on_fail(&mut self) {
        let id = self.task_num;
        let payload = self.take_steps();
        let message = ExecutorEvent::Fail((id, payload));
        self.config.tx.try_send(message).unwrap();
        self.reset();
    }

    fn take_steps(&mut self) -> Vec<StepData> {
        let mut out = Vec::new();
        mem::swap(&mut self.steps, &mut out);
        out
    }

    fn reset(&mut self) {
        self.steps.clear();
        self.steps_finished = 0;
    }
}

fn get_command_result(event: &Event) -> Option<bool> {
    for tag in event.tags.iter() {
        if let Tag::ProcessCompletion(res) = tag {
            let report = match res {
                Some(ProcessEnd::Success) => Some(true),
                Some(ProcessEnd::ExitError(_)) => Some(false),
                Some(_) => Some(false),
                None => None,
            };
            return report;
        };
    }
    None
}

fn parse_command(input: &str) -> Option<Command> {
    let mut splitted: Vec<String> = input.split(' ').map(|x| x.to_owned()).collect();
    if !splitted.is_empty() {
        let prog = splitted.remove(0);
        let args = splitted;
        Some(Command::Exec { prog, args })
    } else {
        None
    }
}

fn is_dir_exists(path: &str) -> bool {
    std::fs::read_dir(path).is_ok()
}

fn not_found_err(txt: &str) -> Error {
    Error::new(ErrorKind::NotFound, txt)
}

fn check_dirs(config: &Config) -> Result<(), Error> {
    if !is_dir_exists(&config.watch_dir) {
        Err(not_found_err("invalid watch directory"))
    } else if !is_dir_exists(&config.build_dir) {
        Err(not_found_err("invalid build directory"))
    } else {
        Ok(())
    }
}

async fn on_update(
    context: Arc<Mutex<Context>>,
    action: Action,
    delay: Option<Duration>,
) -> Result<(), Error> {
    let mut event_stop = false;
    let mut event_mods = false;
    //let mut statuses = Vec::new();
    let mut process_status = None;
    for event in action.events.iter() {
        event_stop |= event.signals().filter(filters::is_stop_signal).count() > 0;
        event_mods |= event.paths().count() > 0;
        if let Some(value) = get_command_result(event) {
            let exist = process_status.get_or_insert(value);
            process_status = Some(*exist & value);
        }
    }

    if event_stop {
        action.outcome(Outcome::Exit);
    } else if event_mods {
        if let Some(delay) = delay {
            let task = [Outcome::Clear, Outcome::Sleep(delay), Outcome::Start].into_iter();
            action.outcome(Outcome::if_running(
                Outcome::DoNothing,
                Outcome::sequence(task),
            ));
        } else {
            let task = [Outcome::Clear, Outcome::Start].into_iter();
            action.outcome(Outcome::if_running(
                Outcome::DoNothing,
                Outcome::sequence(task),
            ));
        };
    } else if let Some(status) = process_status {
        if !status {
            action.outcome(Outcome::Stop);
        }
        let mut context = context.lock().unwrap();
        context.finish_step(status);
    }

    Ok::<(), Error>(())
}

async fn on_start(context: Arc<Mutex<Context>>, prespawn: PreSpawn) -> Result<(), Error> {
    let mut command = prespawn.command().await.unwrap();
    {
        let mut lock = context.lock().unwrap();
        lock.start_step();
        command.current_dir(&lock.config.watch_dir);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok::<(), Error>(())
}

pub fn run(mut config: Config) -> Result<JoinHandle<()>, Error> {
    config.build_dir = config.get_build_dir();
    check_dirs(&config)?;

    let watch_dir = config.watch_dir.clone();
    let delay = config.delay;

    let mut runtime = RuntimeConfig::default();
    runtime.pathset([watch_dir]);
    runtime.commands(config.get_commands());

    let filter = Arc::new(filters::ExtenstionsFilter);
    runtime.filterer(filter);

    let context = Arc::new(Mutex::new(Context::new(config)));
    let local = context.clone();
    runtime.on_pre_spawn(move |prespawn: PreSpawn| on_start(local.clone(), prespawn));
    runtime.on_action(move |action: Action| on_update(context.clone(), action, delay));

    let task = task::spawn(async move {
        let watcher = Watchexec::new(InitConfig::default(), runtime).unwrap();
        watcher.main().await.unwrap().unwrap();
    });

    Ok(task)
}
