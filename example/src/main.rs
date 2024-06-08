use kotars::{jni_class, jni_data_class, jni_init, jni_interface, jni_struct_impl};
use notify::{Config, Error, ErrorKind, Event, RecommendedWatcher, RecursiveMode, Watcher};

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

jni_init!("");

#[jni_interface]
trait TestT {
    fn hello_world(&self, id: i32, a: i32);
}


fn main() {
    println!("Hello, world!");
}




#[jni_class]
struct FileWatcher {}

#[jni_data_class]
struct FileChanged {
    path: String,
}

// #[jni_data_class]
// struct Kebab {
//     path: String,
// }

impl From<String> for FileChanged {
    fn from(value: String) -> Self {
        FileChanged { path: value }
    }
}

#[jni_struct_impl]
impl FileWatcher {
    fn watch(
        &self,
        path: String,
        git_dir_path: String,
        notifier: &impl WatchDirectoryNotifier,
    ) {
        println!("Starting to watch directory {path}");
        watch_directory(path, git_dir_path, notifier);
    }

    fn new() -> FileWatcher {
        FileWatcher {
        }
    }
}



const MIN_TIME_IN_MS_BETWEEN_REFRESHES: u128 = 500;
const WATCH_TIMEOUT: u64 = 500;


pub fn watch_directory(
    path: String,
    git_dir_path: String,
    notifier: &impl WatchDirectoryNotifier,
) {
    // Create a channel to receive the events.
    let (tx, rx) = channel();

    // Create a watcher object, delivering debounced events.
    // The notification back-end is selected based on the platform.
    let config = Config::default();
    config.with_poll_interval(Duration::from_secs(3600));

    let mut watcher =
        RecommendedWatcher::new(tx, config).expect("Init watcher failed");

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher
        .watch(Path::new(path.as_str()), RecursiveMode::Recursive)
        .expect("Start watching failed");

    let mut paths_cached: Vec<String> = Vec::new();

    let mut last_update: u128 = 0;

    while true {
        match rx.recv_timeout(Duration::from_millis(WATCH_TIMEOUT)) {
            Ok(e) => {
                if let Some(paths) = get_paths_from_event_result(&e, &git_dir_path) {
                    let mut paths_without_dirs: Vec<String> = paths
                        .into_iter()
                        .collect();

                    let first_path = paths_without_dirs.first();

                    if let Some(path) = first_path {
                        notifier.detected_change(path.clone().into());
                    }



                    last_update = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("We need a TARDIS to fix this")
                        .as_millis();

                    println!("Event: {e:?}");
                }
            }
            Err(e) => {
                if e != RecvTimeoutError::Timeout {
                    println!("Watch error: {:?}", e);
                }
            }
        }
    }

    watcher
        .unwatch(Path::new(path.as_str()))
        .expect("Unwatch failed");

    // Ok(())
}

pub fn get_paths_from_event_result(event_result: &Result<Event, Error>, git_dir_path: &str) -> Option<Vec<String>> {
    match event_result {
        Ok(event) => {
            let events: Vec<String> = event
                .paths
                .clone()
                .into_iter()
                .filter_map(|path| {
                    // Directories are not tracked by Git so we don't care about them (just about their content)
                    // We won't be able to check if it's a dir if it has been deleted but that's good enough
                    if path.is_dir() {
                        println!("Ignoring directory {path:#?}");
                        None
                    } else {
                        let path_str = path.into_os_string()
                            .into_string()
                            .ok()?;

                        // JGit may create .probe-UUID files for its internal stuff, we don't care about it
                        let probe_prefix = format!("{git_dir_path}.probe-");
                        if path_str.starts_with(probe_prefix.as_str()) {
                            None
                        } else {
                            Some(path_str)
                        }
                    }
                })
                .collect();

            if events.is_empty() {
                None
            } else {
                Some(events)
            }
        }
        Err(err) => {
            println!("{:?}", err);
            None
        }
    }
}

#[jni_interface]
pub trait WatchDirectoryNotifier {
    // fn should_keep_looping(&self) -> bool;
    fn detected_change(&self, path: FileChanged);
}

