use kotars::{jni_class, jni_data_class, jni_init, jni_interface, jni_struct_impl};
use notify::{Config, Error, ErrorKind, Event, RecommendedWatcher, RecursiveMode, Watcher};

jni_init!("");

fn main() {
    println!("Hello, world!");
}




#[jni_class]
struct FileWatcher {}

#[jni_struct_impl]
impl FileWatcher {
    
    fn test(value: Option<i32>) -> Option<i32> {
        
        match value {
            None => { println!("Received None!") }
            Some(value) => { println!("Received value {value}!") }
        }
        
        return Some(1)
    }


    // fn new() -> FileWatcher {
    //     FileWatcher {
    //     }
    // }
}
