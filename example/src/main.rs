use kotars::{jni_init, jni_interface};

jni_init!("com.jetpackduba");

#[jni_interface]
trait TestT {
    fn hello_world(&self, id: i32, a: i32);
}


fn main() {
    println!("Hello, world!");
}
