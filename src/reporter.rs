use colored::{self, ColoredString, Colorize};
use notify_rust::{Notification, Timeout};
use std::collections::HashMap;
use std::io::Result;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

use crate::event::ExecutorEvent;

const APP_NAME: &str = "CppWatch";
const SHOW_TIMEOUT: u32 = 3000;

struct HistoricalData {
    time_total: Duration,
}

type History = HashMap<String, HistoricalData>;
struct Context {
    pass_total: u64,
    fail_total: u64,
    history: History,
}

impl Context {
    fn new() -> Context {
        Context {
            pass_total: 0,
            fail_total: 0,
            history: History::new(),
        }
    }

    fn update(&mut self, event: &ExecutorEvent) {
        let (success, (.., steps)) = match event {
            ExecutorEvent::Fail(msg) => (false, msg),
            ExecutorEvent::Success(msg) => (true, msg),
        };

        self.pass_total += success as u64;
        self.fail_total += !success as u64;

        if success {
            for step in steps {
                let duration = step.get_duration();
                self.history
                    .entry(step.name.clone())
                    .and_modify(|data| data.time_total += duration)
                    .or_insert(HistoricalData {
                        time_total: duration,
                    });
            }
        }
    }

    fn get_duration_avg(&self, name: &str) -> Option<Duration> {
        let total = std::cmp::max(1, self.pass_total);
        self.history.get(name).map(|data| {
            let ms = data.time_total.as_millis() / total as u128;
            Duration::from_millis(ms as u64)
        })
    }

    fn get_ratio(&self) -> u64 {
        let total = std::cmp::max(self.pass_total + self.fail_total, 1);
        self.pass_total * 100 / total
    }
}

fn status_as_str(status: bool) -> &'static str {
    match status {
        true => "done",
        false => "failed",
    }
}

fn status_to_color_str(status: bool) -> ColoredString {
    let txt = status_as_str(status);
    if status {
        txt.bright_green().bold()
    } else {
        txt.bright_red().bold()
    }
}

fn ratio_to_color_str(ratio: u64) -> ColoredString {
    match ratio {
        0..=49 => format!("{}", ratio).bright_red(),
        50..=79 => format!("{}", ratio).bright_yellow(),
        80..=100 => format!("{}", ratio).bright_green(),
        _ => "Invalid".bright_red(),
    }
}

fn duration_diff_as_millis(current: Duration, expect: Duration) -> i64 {
    if current > expect {
        (current - expect).as_millis() as i64
    } else {
        -((expect - current).as_millis() as i64)
    }
}

fn get_icon_name(status: bool) -> &'static str {
    if status {
        "emblem-checked"
    } else {
        "emblem-error"
    }
}

fn print_step_report(name: &str, duration: Duration, duration_avg: Duration) {
    let diff = duration_diff_as_millis(duration, duration_avg);
    let txtdiff = format!("{}", diff);
    let txtdiff = if diff <= 0 {
        txtdiff.bright_green()
    } else {
        txtdiff.bright_yellow()
    };
    let prefix = format!("{} duration:", name);
    let field = format!("{: <24}", prefix);
    println!("{} {} ms", field, duration.as_millis());
    let prefix = format!("{} duration avg:", name);
    let field = format!("{: <24}", prefix);
    println!("{} {} ms", field, duration_avg.as_millis());

    let prefix = format!("{} duration delta:", name);
    let field = format!("{: <24}", prefix);
    println!("{} {} ms", field, txtdiff);
}

fn print_line() {
    println!("========================================");
}

fn print_report(context: &Context, event: &ExecutorEvent) {
    let (success, (id, steps)) = match event {
        ExecutorEvent::Fail(msg) => (false, msg),
        ExecutorEvent::Success(msg) => (true, msg),
    };

    print_line();
    println!("Build {}", id);
    print_line();
    for step in steps.iter() {
        let duration_avg = context
            .get_duration_avg(&step.name)
            .unwrap_or(Duration::from_millis(0));
        let duration = step.get_duration();
        print_step_report(&step.name, duration, duration_avg);
        println!();
    }
    let ratio_txt = ratio_to_color_str(context.get_ratio());

    println!(
        "{: <24} {} % [{}/{}]",
        "Pass ratio:",
        ratio_txt,
        context.pass_total,
        context.pass_total + context.fail_total
    );
    print_line();
    println!("Status: {}", status_to_color_str(success));
    print_line();
}

fn show_notification(event: &ExecutorEvent) {
    let (success, (id, steps)) = match event {
        ExecutorEvent::Fail(msg) => (false, msg),
        ExecutorEvent::Success(msg) => (true, msg),
    };
    let mut total_dur = Duration::from_millis(0);
    for step in steps.iter() {
        total_dur += step.get_duration();
    }
    let txt = format!(
        "Build {} {} after {} sec.",
        id,
        status_as_str(success),
        total_dur.as_secs()
    );
    Notification::new()
        .summary(APP_NAME)
        .icon(get_icon_name(success))
        .body(&txt)
        .timeout(Timeout::Milliseconds(SHOW_TIMEOUT)) //milliseconds
        .show()
        .unwrap();
}

fn process_event(context: Arc<Mutex<Context>>, event: &ExecutorEvent) {
    let mut context = context.lock().unwrap();
    context.update(event);
    print_report(&context, event);
    show_notification(event);
}

pub fn run(mut rx: Receiver<ExecutorEvent>) -> Result<JoinHandle<()>> {
    let context = Arc::new(Mutex::new(Context::new()));
    let task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            process_event(context.clone(), &event);
        }
    });

    Ok(task)
}
