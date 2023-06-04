use watchexec::error::RuntimeError;
use watchexec::filter::Filterer;

use watchexec_events::filekind::{self, ModifyKind};
use watchexec_events::{Event, Priority, Tag};

use watchexec_signals::Signal;

#[derive(Debug)]
pub struct ExtenstionsFilter;

#[derive(Debug)]
pub struct ModificationFilter;

pub fn is_process_report(event: &Event) -> bool {
    for tag in event.tags.iter() {
        if matches!(tag, Tag::ProcessCompletion(_)) {
            return true;
        }
    }
    false
}

pub fn is_file_modification(event: &Event) -> bool {
    for tag in event.tags.iter() {
        if matches!(
            tag,
            Tag::FileEventKind(filekind::FileEventKind::Modify(ModifyKind::Data(_)))
                | Tag::FileEventKind(filekind::FileEventKind::Create(_))
        ) {
            return true;
        }
    }
    false
}

pub fn is_cpp_file(event: &Event) -> bool {
    for tag in event.tags.iter() {
        if let Tag::Path { path, .. } = tag {
            let extension = path
                .as_os_str()
                .to_str()
                .map(|x| x.split('.'))
                .and_then(|x| x.last())
                .unwrap_or("");
            if matches!(extension, "c" | "h" | "cpp" | "hpp" | "cc" | "hh") {
                return true;
            }
        }
    }
    false
}

pub fn is_stop_signal(signal: &Signal) -> bool {
    matches!(signal, Signal::Interrupt | Signal::Terminate)
}

impl Filterer for ExtenstionsFilter {
    fn check_event(&self, event: &Event, _priority: Priority) -> Result<bool, RuntimeError> {
        let result =
            is_process_report(event) || (is_file_modification(event) && is_cpp_file(event));
        Ok(result)
    }
}

impl Filterer for ModificationFilter {
    fn check_event(&self, event: &Event, _priority: Priority) -> Result<bool, RuntimeError> {
        let result = is_process_report(event) || is_file_modification(event);
        Ok(result)
    }
}
