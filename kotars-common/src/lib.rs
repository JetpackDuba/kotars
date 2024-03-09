use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub enum Parameter {
    Typed {
        name: String,
        ty: JniType,
    },
    Receiver {
        is_mutable: bool,
    },
}

#[derive(Serialize, Deserialize)]
pub struct Function {
    pub struct_name: String,
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

impl RsStruct {
    pub fn all_fields_are_public(&self) -> bool {
        self.fields
            .iter()
            .any(|p| !p.is_public) // any is used because an empty list of fields is permitted
    }
}

#[derive(Serialize, Deserialize)]
pub struct Field {
    pub is_public: bool,
    pub name: Option<String>,
    pub ty: JniType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JniType {
    Int32,
    Int64,
    String,
    Boolean,
    Receiver(String),
    CustomType(String),
}

impl From<String> for JniType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "i32" => JniType::Int32,
            "i64" => JniType::Int64,
            "String" => JniType::String,
            "bool" => JniType::Boolean,
            _ => JniType::CustomType(value.to_string()),
        }
    }
}

pub fn string_to_camel_case(text: &str) -> String {
    text
        .to_string()
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