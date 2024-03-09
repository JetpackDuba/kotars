use kotars::{jni_class, jni_init, jni_struct_impl};
jni_init!("");

fn main() {
    println!("Hello, world!");
}

#[jni_class]
struct Kebab;


#[jni_struct_impl]
impl Kebab {
    fn is_yummy() -> bool {
        true
    }
}