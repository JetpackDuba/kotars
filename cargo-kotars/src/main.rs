use std::fmt::format;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use clap::Parser;
use kotars_common::{Field, Function, JniType, Parameter, RsStruct, string_to_camel_case};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path where the Kotlin source code files will be copied
    #[arg(short, long)]
    kotlin_output: String,
}


fn main() {
    let args = Args::parse();

    let mut command = Command::new("cargo");
    command.arg("rustc");
    command.arg("--profile");
    command.arg("check");
    command.arg("--");
    command.arg("-Zunpretty=expanded");

    let dir = Path::new(&args.kotlin_output);

    let package_name_line_prefix = "pub const JNI_PACKAGE_NAME: &str = \"";
    create_base_files(dir);

    let res = command.output().expect("Output read failed");
    let out_text = String::from_utf8_lossy(&res.stdout);
    let err = String::from_utf8_lossy(&res.stderr);
    println!("Err: {err}");
    let text = out_text.to_string();
    let lines = text.split('\n');

    let package_name = lines.clone()
        .filter_map(|line| {
            if line.contains(package_name_line_prefix) {
                let range_start = line.find(package_name_line_prefix).expect("Package name not found in line") + package_name_line_prefix.len();
                let range_end = line.len() - 2;
                let package_name = &line[range_start..range_end];
                Some(package_name.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<String>>()
        .first()
        .expect("Package name not found in source")
        .clone();

    let functions = lines.clone() // todo do not clone
        .filter_map(|line| {
            if line.contains("JNI_FN_DATA") {
                // let json = line.
                let prefix_to_remove = "JNI_FN_DATA ";
                let range_start = line.find(prefix_to_remove).expect("JNI_FN_DATA not found.") + prefix_to_remove.len();
                let range_end = line.len() - 2;
                let json_line = &line[range_start..range_end].replace('\\', "");
                println!("{json_line}");
                let func: Function = serde_json::from_str(json_line).expect(format!("Unable to deserialize function {json_line}").as_str());
                Some(func)
            } else {
                None
            }
        })
        .collect::<Vec<Function>>();

    let classes = lines.clone() // todo do not clone
        .filter_map(|line| {
            if line.contains("JNI_CLASS") {
                let prefix_to_remove = "JNI_CLASS ";
                let range_start = line.find(prefix_to_remove).expect("JNI_CLASS not found.") + prefix_to_remove.len();
                let range_end = line.len() - 2;
                let json_line = &line[range_start..range_end].replace('\\', "");
                let struc: RsStruct = serde_json::from_str(json_line).expect(format!("Unable to deserialize class {json_line}").as_str());
                let functions = functions
                    .iter()
                    .filter(|func| func.struct_name == struc.name)
                    .collect::<Vec<&Function>>();

                Some((struc, functions))
            } else {
                None
            }
        })
        .collect::<Vec<(RsStruct, Vec<&Function>)>>();

    let data_classes = lines
        .filter_map(|line| {
            if line.contains("JNI_DATA_CLASS") {
                // let json = line.
                let prefix_to_remove = "JNI_DATA_CLASS ";
                let range_start = line.find(prefix_to_remove).expect("JNI_DATA_CLASS not found.") + prefix_to_remove.len();
                let range_end = line.len() - 2;
                let json_line = &line[range_start..range_end].replace('\\', "");
                let struc: RsStruct = serde_json::from_str(json_line).expect(format!("Unable to deserialize data class {json_line}").as_str());
                Some(struc)
            } else {
                None
            }
        })
        .collect::<Vec<RsStruct>>();

    for data_class in data_classes {
        create_data_class(dir, &data_class, package_name.as_str());
    }

    for (class, functions) in classes {
        create_class(dir, class, package_name.as_str(), functions)
    }
    // println!("Abs path of file is: {abs_path:?}");
}

fn create_base_files(dir: &Path) {
    let content = include_str!("AutoCloseThread.kt");
    let file = dir.join("AutoCloseThread.kt");
    let file = file.as_path();
    let mut file = File::create(file).expect("Creating AutoCloseThread.kt failed.");

    file.write_all(content.as_bytes()).expect("Writing to AutoCloseThread.kt failed.");
}

fn create_class(dir: &Path, rs_struct: RsStruct, package_name: &str, functions: Vec<&Function>) {
    println!("Dir is {dir:?}");
    let class_name = &rs_struct.name;
    let file_name = format!("{class_name}.kt");
    let file_path = Path::new(file_name.as_str());
    let file_path = PathBuf::from(dir).join(file_path);

    let mut file = File::create(file_path).expect("File creation failed");

    let functions_formatted: String = functions.iter()
        .map(|func| format_function(func))
        .collect::<Vec<String>>()
        .join("\n\n");

    let member_functions_mapping_formatted: String = functions.iter()
        .filter_map(|func| {
            let has_receiver_parameter = func.parameters
                .iter()
                .any(|param| matches!(param, Parameter::Receiver { .. }));

            if has_receiver_parameter {
                let formatted_function = format_function_mapping(func, false);
                Some(formatted_function)
            } else {
                None
            }
        })
        .collect::<Vec<String>>()
        .join("");


    let static_functions_mapping_formatted: String = functions.iter()
        .filter_map(|func| {
            let has_receiver_parameter = func.parameters
                .iter()
                .any(|param| matches!(param, Parameter::Receiver { .. }));

            if has_receiver_parameter {
                None
            } else {
                let formatted_function = format_function_mapping(func, true);
                Some(formatted_function)
            }
        })
        .collect::<Vec<String>>()
        .join("");

    let content = format!(r#"
//package {package_name}

class {class_name} private constructor(private val pointer: Long) : AutoCloseable {{
    private val resource: NativeResource = thread.addObject(this, pointer) {{ {class_name}Obj.destroy(it) }}

{member_functions_mapping_formatted}

    override fun close() {{
        resource.close()
        thread.remove(resource)
    }}

    companion object {{
    {static_functions_mapping_formatted}
    }}
}}

private object {class_name}Obj {{
    {functions_formatted}

    external fun destroy(pointer: Long)
}}
"#);

    file.write_all(content.as_bytes()).expect("Writing Kotlin source code failed");
    file.flush().unwrap();
}

fn format_function(func: &Function) -> String {
    let name = string_to_camel_case(&func.name);

    let mut parameters_formatted = format_func_parameters(&func.parameters, true);
    let return_ty = formatted_return_ty(&func.return_type);
    if !parameters_formatted.is_empty() && !parameters_formatted.ends_with("\n") {
        parameters_formatted = format!("\n        {parameters_formatted}\n    ");
    };

    format!("    external fun {name}({parameters_formatted}){return_ty}")
}

fn format_function_mapping(func: &Function, is_static: bool) -> String {
    let struct_name = &func.struct_name;
    let name = string_to_camel_case(&func.name);

    let mut parameters_formatted = format_func_parameters(&func.parameters, is_static);
    let return_ty = formatted_return_ty(&func.return_type);
    if !parameters_formatted.is_empty() && !parameters_formatted.ends_with("\n") {
        parameters_formatted = format!("\n        {parameters_formatted}\n    ");
    };

    let params_as_args = func.parameters.iter()
        .map(|param| {
            match param {
                Parameter::Typed { name, ty } => name,
                Parameter::Receiver { .. } => "this.pointer"
            }
        })
        .collect::<Vec<&str>>()
        .join(", ");

    format!(
        r#"
    fun {name}({parameters_formatted}){return_ty} =
        {struct_name}Obj.{name}({params_as_args})
    "#)
}

fn create_data_class(dir: &Path, rs_struct: &RsStruct, package_name: &str) {
    let class_name = &rs_struct.name;
    let file_name = format!("{class_name}.kt");

    let file_path = Path::new(file_name.as_str());
    let file_path = PathBuf::from(dir).join(file_path);

    let mut file = File::create(file_path).expect("File creation failed");

    let fields = rs_struct.fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let alternative_name = format!("param{index}");
            let field_name = field.name.as_ref().unwrap_or(&alternative_name);
            let ty = jni_to_kotlin_type(&field.ty);
            format!("val {field_name}: {ty},")
        })
        .collect::<Vec<String>>()
        .join("\n    ");

    let content = format!(r#"
//package {package_name}

data class {class_name} (
    {fields}
)
"#);

    file.write_all(content.as_bytes()).expect("Writing Kotlin source code failed");
    file.flush().unwrap();
}

fn formatted_return_ty(return_ty: &Option<JniType>) -> String {
    match return_ty.as_ref() {
        None => { String::new() }
        Some(ty) => {
            let ty = jni_to_kotlin_type(ty);
            format!(": {ty}")
        }
    }
}

fn format_func_parameters(params: &[Parameter],  is_static: bool) -> String {
    params.iter()
        .map(|param| {
            match param {
                Parameter::Typed { name, ty } => {
                    let kotlin_ty = jni_to_kotlin_type(ty);
                    format!("{name}: {kotlin_ty},")
                }
                Parameter::Receiver { .. } => { if is_static {"pointer: Long,".to_string()} else { String::new() } }
            }
        })
        .filter(|it| !it.is_empty())
        .collect::<Vec<String>>()
        .join("\n        ")
}

fn format_member_func_parameters(params: &[Parameter]) -> String {
    params.iter()
        .map(|param| {
            match param {
                Parameter::Typed { name, ty } => {
                    let kotlin_ty = jni_to_kotlin_type(ty);
                    format!("{name}: {kotlin_ty},")
                }
                Parameter::Receiver { .. } => { String::new() }
            }
        })
        .filter(|it| !it.is_empty())
        .collect::<Vec<String>>()
        .join("\n        ")
}

fn jni_to_kotlin_type(ty: &JniType) -> String {
    match ty {
        JniType::Int32 => "Int".to_string(),
        JniType::Int64 => "Long".to_string(),
        JniType::String => "String".to_string(),
        JniType::Boolean => "Boolean".to_string(),
        JniType::CustomType(name) => name.clone(),
        JniType::Receiver(_) => todo!(),
    }
}

enum KotlinTypes {
    DataClass {
        name: String,
        fields: Vec<Field>,
    },
    Class {
        name: String,
        functions: Function,
    },
}