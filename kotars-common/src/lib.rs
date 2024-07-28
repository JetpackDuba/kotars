use std::fmt::format;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub enum Parameter {
    Typed {
        name: String,
        ty: JniType,
        is_borrow: bool,
        is_mutable: bool,
    }, // TODO replace field with single type tuple
    Receiver { is_mutable: bool },
}

#[derive(Serialize, Deserialize)]
pub struct Function {
    pub owner_name: String,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<JniType>,
}

impl Function {
    pub fn is_static(&self) -> bool {
        self.parameters
            .iter()
            .any(|p| matches!(p, Parameter::Receiver { .. }))
    }
}

#[derive(Serialize, Deserialize)]
pub struct RsStruct {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Serialize, Deserialize)]
pub struct RsInterface {
    pub name: String,
    pub functions: Vec<Function>,
}

#[derive(Serialize, Deserialize)]
pub struct RsTrait {
    pub functions: Vec<Function>,
}


#[derive(Serialize, Deserialize)]
pub struct RsStructImpl {
    pub functions: Vec<Function>,
}

impl RsStruct {
    pub fn all_fields_are_public(&self) -> bool {
        self.fields.iter().any(|p| !p.is_public) // any is used because an empty list of fields is permitted
    }
}

#[derive(Serialize, Deserialize)]
pub struct Field {
    pub is_public: bool,
    pub name: Option<String>,
    pub ty: JniType,
}

impl Field {
    pub fn safe_name(&self, index: &usize) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("param{index}"),
        }
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JniType {
    Int32,
    Int64,
    UInt64,
    Float32,
    Float64,
    String,
    Boolean,
    Receiver(String),
    CustomType(String),
    Interface(String),
    Option(Box<JniType>),
    ByteArray,
    Vec(Box<JniType>),
    Void,
}

impl From<String> for JniType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "i32" => JniType::Int32,
            "i64" => JniType::Int64,
            "u64" => JniType::UInt64,
            "f32" => JniType::Float32,
            "f64" => JniType::Float64,
            "String" => JniType::String,
            "bool" => JniType::Boolean,
            _ => {
                let interface_prefix = "impl ";
                let option_prefix = "Option";
                let value_without_spaces = value.replace(' ', "");

                if value_without_spaces == "Vec<u8>" {
                    JniType::ByteArray
                } else if value_without_spaces.starts_with("Vec<") && value_without_spaces.ends_with(">") {
                    let inner_ty = value_without_spaces.strip_prefix("Vec<").unwrap().strip_suffix(">").unwrap().to_string();
                    let inner_ty = Box::new(JniType::from(inner_ty));
                    JniType::Vec(inner_ty)
                } else if value.starts_with(option_prefix) {

                    let ty = value.strip_prefix("Option < ").expect("Removing option prefix failed").strip_suffix(" >").expect("Removing option suffix failed");

                    JniType::Option(Box::new(JniType::from(ty.to_string())))
                } else if value.starts_with(interface_prefix) {
                    let range_start = interface_prefix.len();
                    let range_end = value.len();
                    let interface_name = &value[range_start..range_end];

                    JniType::Interface(interface_name.to_string())
                } else {
                    JniType::CustomType(value.to_string())
                }
            }
        }
    }
}

pub fn string_to_camel_case(text: &str) -> String {
    text.to_string()
        .split(['_', ' '])
        .enumerate()
        .map(|(index, word)| {
            let word = word.to_string();
            if index == 0 || word.is_empty() {
                word
            } else {
                let mut letters = word.chars().collect::<Vec<char>>();

                letters[0] = letters[0]
                    .to_uppercase()
                    .to_string()
                    .chars()
                    .collect::<Vec<char>>()[0];

                letters.iter().collect::<String>()
            }
        })
        .collect::<Vec<String>>()
        .join("")
}
